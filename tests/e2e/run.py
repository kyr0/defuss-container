#!/usr/bin/env python3
"""End-to-end gate: the installed artifact (CLI, daemon, engine) drives a real VM; then the built desktop UI drives
the same daemon in Playwright. Every CLI command and the full guest lifecycle run at least once.

Writes output/e2e/: report.json (steps), resources.json + summary.md (peak RSS, disk, job durations), daemon.log, gui/.
Requires: a hypervisor, network access (Docker Hub base image, npm registry for the fixture's dependency), Git,
openssl, Node with the desktop dependencies installed.
"""
from __future__ import annotations

import argparse
import contextlib
import json
import os
import pathlib
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import traceback
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import measure  # noqa: E402
from gitserver import GitServer  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parents[2]
EXE = ".exe" if os.name == "nt" else ""
APP = "hello"
HOST_MARKER = "defuss-physical-host"


class Failure(Exception):
    pass


def check(condition: bool, message: str) -> None:
    if not condition:
        raise Failure(message)


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class HostService(BaseHTTPRequestHandler):
    """Listens on host loopback; the guest reaches it through the `host` alias."""

    def do_GET(self) -> None:
        self.send_response(200)
        self.send_header("Content-Length", str(len(HOST_MARKER)))
        self.end_headers()
        self.wfile.write(HOST_MARKER.encode())

    def log_message(self, *args: object) -> None:
        pass


class Report:
    def __init__(self, out: pathlib.Path) -> None:
        self.out = out
        self.steps: list[dict[str, Any]] = []
        self.started = time.time()

    @contextlib.contextmanager
    def step(self, name: str):
        t = time.monotonic()
        print(f"{time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())} INFO e2e step={name}", flush=True)
        try:
            yield
        except Exception as e:
            self.steps.append({"step": name, "ok": False, "seconds": round(time.monotonic() - t, 2), "error": f"{type(e).__name__}: {e}"})
            raise
        self.steps.append({"step": name, "ok": True, "seconds": round(time.monotonic() - t, 2)})

    def write(self, ok: bool, extra: dict[str, Any]) -> None:
        (self.out / "report.json").write_text(json.dumps({"ok": ok, "seconds": round(time.time() - self.started, 1),
                                                          "steps": self.steps, **extra}, indent=2) + "\n")


class Cli:
    """The installed CLI. Records each invocation's peak RSS."""

    def __init__(self, exe: pathlib.Path, home: pathlib.Path) -> None:
        self.exe, self.home, self.peak, self.calls = exe, home, 0, 0
        self.used: set[str] = set()

    def raw(self, *args: str) -> measure.Result:
        r = measure.run([str(self.exe), "--home", str(self.home), *args])
        self.calls += 1
        self.used.add(args[0])
        self.peak = max(self.peak, r.peak_rss or 0)
        return r

    def __call__(self, *args: str) -> Any:
        r = self.raw(*args)
        check(r.code == 0, f"defuss-container {' '.join(args)} exited {r.code}: {r.stderr.strip()[-2000:]}")
        try:
            return json.loads(r.stdout)
        except ValueError:
            return r.stdout

    def fails(self, *args: str) -> str:
        r = self.raw(*args)
        check(r.code != 0, f"defuss-container {' '.join(args)} unexpectedly succeeded")
        return r.stderr

    def op(self, *args: str, timeout: int = 900) -> Any:
        job = self(*args)
        done = self.raw("wait", job["id"], "--timeout", str(timeout))
        if done.code != 0:
            raise Failure(f"{' '.join(args)} job {job['id']} failed: {done.stderr.strip()[-3000:]}\n"
                          f"job log tail:\n{self('job-log', job['id'])[-3000:]}")
        return json.loads(done.stdout)["result"]


class Daemon:
    def __init__(self, exe: pathlib.Path, home: pathlib.Path, engine: pathlib.Path, env: dict[str, str], log: pathlib.Path) -> None:
        self.args = [str(exe), "--home", str(home), "--engine", str(engine), "--port", "0"]
        self.home, self.env, self.log = home, env, log
        self.proc: subprocess.Popen | None = None
        self.peaks: list[int] = []

    def start(self) -> None:
        old = (self.home / "connection.json").read_text() if (self.home / "connection.json").exists() else None
        log = self.log.open("ab")
        self.proc = subprocess.Popen(self.args, stdout=log, stderr=subprocess.STDOUT, env=self.env)
        log.close()
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            check(self.proc.poll() is None, f"daemon exited; see {self.log}")
            try:
                current = (self.home / "connection.json").read_text()
                if current != old and json.loads(current)["token"]:
                    return
            except (FileNotFoundError, ValueError, KeyError):
                pass
            time.sleep(0.05)
        raise Failure("daemon did not publish connection.json within 30 s")

    def stop(self) -> None:
        if not self.proc or self.proc.returncode is not None:
            return
        if os.name == "nt":
            self.proc.terminate()  # no SIGTERM on Windows; TerminateProcess
        else:
            self.proc.send_signal(signal.SIGTERM)
        timer = threading.Timer(120, self.proc.kill)
        timer.start()
        try:
            self.peaks.append(measure.reap(self.proc) or 0)
        finally:
            timer.cancel()


def http(port: int, path: str, method: str = "GET", timeout: float = 10) -> str:
    request = urllib.request.Request(f"http://127.0.0.1:{port}{path}", method=method)
    with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(request, timeout=timeout) as r:
        return r.read().decode()


def unreachable(port: int) -> bool:
    try:
        http(port, "/health", timeout=3)
        return False
    except (urllib.error.URLError, OSError):
        return True


class Fixture:
    """A Git repository holding examples/hello at apps/hello, pushed to the HTTPS server after each change."""

    def __init__(self, work: pathlib.Path) -> None:
        self.src, self.bare = work / "src", work / "srv" / "fixture.git"
        self.app = self.src / "apps" / APP
        self.bare.parent.mkdir(parents=True)
        shutil.copytree(ROOT / "examples" / APP, self.app, ignore=shutil.ignore_patterns("node_modules"))
        subprocess.run(["git", "init", "-q", "--bare", str(self.bare)], check=True)
        self.git("init", "-q", "-b", "main")

    def git(self, *args: str) -> None:
        subprocess.run(["git", "-c", "user.name=e2e", "-c", "user.email=e2e@localhost", "-c", "core.autocrlf=false", *args],
                       cwd=self.src, check=True, capture_output=True)

    def publish(self, version: str, message: str) -> None:
        (self.app / "version.txt").write_text(version + "\n")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", message)
        self.git("push", "-q", "--force", str(self.bare), "main")


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--install", type=pathlib.Path, required=True, help="directory scripts/install.py installed into (bin/, engine/)")
    p.add_argument("--dist", type=pathlib.Path, default=ROOT / "desktop" / "dist", help="built desktop UI")
    p.add_argument("--output", type=pathlib.Path, default=ROOT / "output" / "e2e")
    p.add_argument("--browsers", default="chromium,webkit", help="Playwright engines for the GUI phase; empty skips it")
    a = p.parse_args()
    install = a.install.resolve()
    cli_exe, daemon_exe = install / "bin" / f"defuss-container{EXE}", install / "bin" / f"defuss-containerd{EXE}"
    engine = install / "engine" / f"smolvm{EXE}"
    for f in (cli_exe, daemon_exe, engine):
        check(f.is_file(), f"missing installed artifact {f}; run scripts/install.py --home {install}")
    out = a.output.resolve()
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    report = Report(out)
    # Under the repository's tmp/: a stable, symlink-free path on every OS (macOS /tmp and /var are symlinks,
    # which the daemon refuses for shares).
    base = ROOT / "tmp" / "e2e"
    base.mkdir(parents=True, exist_ok=True)
    work = pathlib.Path(tempfile.mkdtemp(prefix="run-", dir=base))
    home, shares = work / "state", work / "shares"
    for d in ("input", "etc"):
        (shares / d).mkdir(parents=True)
    (shares / "input" / "hello.txt").write_text("from host input\n")
    (shares / "etc" / "config.txt").write_text("from host etc\n")
    sampler = measure.MachineSampler(str(engine))
    cli = Cli(cli_exe, home)
    resources: dict[str, Any] = {}
    port, second_port = free_port(), free_port()
    host_server = ThreadingHTTPServer(("127.0.0.1", 0), HostService)
    threading.Thread(target=host_server.serve_forever, daemon=True).start()
    fixture = Fixture(work)
    fixture.publish("1", "v1")
    ok, daemon, machine = False, None, None
    engine_cache = None
    try:
        with GitServer(work / "srv", work) as git:
            env = {**os.environ, "GIT_SSL_CAINFO": str(git.cert), "RUST_LOG": "info"}
            if os.name == "nt":  # Git for Windows defaults to Schannel, which ignores GIT_SSL_CAINFO
                env.update({"GIT_CONFIG_COUNT": "1", "GIT_CONFIG_KEY_0": "http.sslBackend", "GIT_CONFIG_VALUE_0": "openssl"})
            daemon = Daemon(daemon_exe, home, engine, env, out / "daemon.log")
            spec = {"name": APP, "repository": git.url("fixture"), "git_ref": "main", "subdirectory": f"apps/{APP}",
                    "mounts": str(shares), "cpus": 2, "memory_mib": 1024, "storage_gib": 20,
                    "ports": [{"host": port, "guest": 3000}], "env": {"PORT": "3000", "HOST_TEST_PORT": str(host_server.server_port)},
                    "build": False, "health": {"port": 3000, "path": "/health"}, "stop_timeout_seconds": 30}
            spec_file = work / "app.json"
            spec_file.write_text(json.dumps(spec))

            with report.step("daemon start"):
                daemon.start()
                sampler.start()
            with report.step("doctor: engine and hypervisor ready"):
                doctor = cli("doctor")
                check(doctor["version"] == "smolvm 1.32.4", f"engine version {doctor['version']!r}")
                check(doctor["hypervisor_ready"], f"hypervisor not ready: {doctor['hypervisor_error']}")
                resources["provider"] = doctor["provider"]
            with report.step("register app; duplicate and invalid specs rejected"):
                check(cli("list") == [], "fresh daemon lists apps")
                created = cli("create", str(spec_file))
                check(created["spec"]["name"] == APP and created["active_release"] is None, f"create returned {created}")
                cli.fails("create", str(spec_file))
                bad = work / "bad.json"
                bad.write_text(json.dumps({**spec, "name": "Bad Name"}))
                cli.fails("create", str(bad))
                check(len(cli("list")) == 1, "one app registered")
                check(cli("releases", APP) == [] and cli("backups", APP) == [], "new app has no releases or backups")
                machine = cli("show", APP)["application"]["machine"]
            with report.step("build v1 in a builder VM"):
                job = cli("build", APP)
                check("wait timed out" in cli.fails("wait", job["id"], "--timeout", "0"), "wait --timeout 0 reports a timeout")
                done = cli("wait", job["id"], "--timeout", "900")
                v1 = done["result"]
                check(len(v1["commit"]) == 40 and v1["base_digest"].startswith("sha256:"), f"release record {v1}")
                check(any(j["id"] == job["id"] and j["state"] == "succeeded" for j in cli("jobs")), "jobs lists the build")
                check(cli("job", job["id"])["state"] == "succeeded", "job shows the build")
                check(f'"release":"{v1["id"]}"' in cli("job-log", job["id"]), "job-log carries builder output")
                check([r["id"] for r in cli("releases", APP)] == [v1["id"]], "releases lists v1")
            with report.step("deploy v1: health-gated start"):
                cli.op("deploy", APP, v1["id"])
                check(json.loads(http(port, "/health"))["ok"], "health endpoint")
                check(http(port, "/version") == "1", "v1 serves")
                shown = cli("show", APP)
                check(shown["application"]["active_release"] == v1["id"] and shown["application"]["desired_running"], f"show {shown}")
                check(str(shown["observed"].get("state", "")).lower() == "running", f"observed {shown['observed']}")
                logs = cli("logs", APP)
                check("defuss-supervisor started" in logs and "hello listening" in logs, f"logs: {logs[-500:]}")
            with report.step("guest wiring: shares, read-only input, tmp, env, host alias"):
                s = json.loads(http(port, "/shares"))
                check(s["input"] == "from host input\n" and s["etc"] == "from host etc\n", f"shares {s}")
                check(s["input_writable"] is False, "input share is read-only")
                check(s["tmp_empty_at_start"] is True and s["app"] == APP and s["node_env"] == "production", f"guest env {s}")
                check(http(port, "/host") == HOST_MARKER, "host alias reaches host loopback")
                http(port, "/output")
                check((shares / "output" / "hello.txt").read_text() == "hello from Bun\n", "output share is writable and host-visible")
            with report.step("SQLite persistence across stop/start; backup"):
                http(port, "/counter", "POST")
                before = json.loads(http(port, "/counter", "POST"))["value"]
                backup = cli.op("backup", APP)
                check(json.loads(http(port, "/counter"))["value"] == before, "backup resumed the app")
                http(port, "/counter", "POST")
                cli.op("stop", APP)
                check(unreachable(port), "stopped app is unreachable")
                check(cli("show", APP)["application"]["desired_running"] is False, "desired state stopped")
                cli.op("start", APP)
                check(json.loads(http(port, "/counter"))["value"] == before + 1, "counter persisted across stop/start")
                check(json.loads(http(port, "/shares"))["tmp_empty_at_start"], "tmp emptied on restart")
            with report.step("restore backup with safety backup"):
                restored = cli.op("restore", APP, backup["id"])
                check(json.loads(http(port, "/counter"))["value"] == before, "restore rewound the data")
                ids = {b["id"] for b in cli("backups", APP)}
                check(ids == {backup["id"], restored["pre_restore_backup"]["id"]}, f"backups {ids}")
            with report.step("build + deploy v2, data kept"):
                fixture.publish("2", "v2")
                v2 = cli.op("build", APP)
                cli.op("deploy", APP, v2["id"])
                check(http(port, "/version") == "2" and json.loads(http(port, "/counter"))["value"] == before, "v2 with data")
            with report.step("rollback to v1, data kept"):
                cli.op("rollback", APP)
                check(http(port, "/version") == "1" and json.loads(http(port, "/counter"))["value"] == before, "v1 after rollback")
            with report.step("failing release: deploy refused, previous release restored"):
                pkg = json.loads((fixture.app / "package.json").read_text())
                good = pkg["scripts"]["start"]
                pkg["scripts"]["start"] = 'bun -e "process.exit(3)"'
                (fixture.app / "package.json").write_text(json.dumps(pkg, indent=2) + "\n")
                fixture.publish("3", "v3 crashes on start")
                v3 = cli.op("build", APP)
                job = cli("deploy", APP, v3["id"])
                err = cli.fails("wait", job["id"], "--timeout", "600")
                check("exited" in err, f"deploy failure names the exit: {err[-500:]}")
                check(http(port, "/version") == "1", "previous release serves again")
                app_state = cli("show", APP)["application"]
                check(app_state["active_release"] == v1["id"] and app_state["last_error"], f"state after failed deploy {app_state}")
                pkg["scripts"]["start"] = good
                (fixture.app / "package.json").write_text(json.dumps(pkg, indent=2) + "\n")
            with report.step("daemon restart: credentials rotate, desired state reconciled"):
                old_token = json.loads((home / "connection.json").read_text())["token"]
                old_jobs = {j["id"] for j in cli("jobs")}
                daemon.stop()
                check(json.loads(http(port, "/health"))["ok"], "VM keeps serving while the daemon is down")
                daemon.start()
                check(json.loads((home / "connection.json").read_text())["token"] != old_token, "token rotated")
                deadline = time.monotonic() + 120
                while time.monotonic() < deadline:
                    starts = [j for j in cli("jobs") if j["id"] not in old_jobs and j["app"] == APP and j["action"]["action"] == "start"]
                    if starts and starts[0]["state"] in ("succeeded", "failed", "interrupted"):
                        break
                    time.sleep(1)
                check(starts and starts[0]["state"] == "succeeded", f"reconciliation start job {starts[:1]}")
            with report.step("api command: raw request through the shared client"):
                check(cli("api", "GET", "/v1/health")["api"] == 1, "api GET")
                check(isinstance(cli("api", "GET", f"/v1/apps/{APP}/logs"), str), "text endpoint returns a JSON string")
                cli.fails("api", "GET", "/v2/escape")
            browsers = [b for b in a.browsers.split(",") if b]
            for browser in browsers:
                with report.step(f"GUI ({browser}): built desktop UI against the live daemon"):
                    gui_env = {**os.environ, "E2E_CLI": str(cli_exe), "E2E_HOME": str(home), "E2E_APP": APP,
                               "E2E_APP_PORT": str(port), "E2E_SECOND_PORT": str(second_port), "E2E_REPOSITORY": git.url("fixture"),
                               "E2E_SHARES": str(work / "gui-shares"), "E2E_BROWSER": browser, "E2E_DIST": str(a.dist.resolve()),
                               "E2E_OUTPUT": str(out / "gui" / browser)}
                    r = measure.run(["node", "--test", "--test-reporter=spec", str(ROOT / "desktop" / "tests" / "e2e.test.mjs")], env=gui_env)
                    (out / f"gui-{browser}.log").write_text(r.stdout + r.stderr)
                    check(r.code == 0, f"GUI e2e failed; see output/e2e/gui-{browser}.log:\n{(r.stdout + r.stderr)[-3000:]}")
                    check(http(port, "/version") == "1", "GUI phase left the app serving")
            with report.step("measure disk before delete"):
                data_dir = pathlib.Path(subprocess.run([str(engine), "machine", "data-dir", "--name", machine], capture_output=True, text=True, check=True).stdout.strip())
                engine_cache = data_dir.parent.parent
                releases = home / "apps" / APP / "releases"
                resources["disk"] = {
                    "installed_bin": measure.disk_usage(install / "bin"),
                    "installed_engine": measure.disk_usage(install / "engine"),
                    "daemon_state_total": measure.disk_usage(home),
                    "daemon_base_image_cache": measure.disk_usage(home / "cache"),
                    "release_each": {r.name: {f.name: f.stat().st_size for f in r.iterdir() if f.is_file()} for r in releases.iterdir()},
                    "backups": measure.disk_usage(home / "apps" / APP / "backups"),
                    "app_vm_disk": measure.disk_usage(data_dir),
                    "engine_image_seeds": measure.disk_usage(engine_cache / "image-seeds"),
                }
                seeds = engine_cache / "image-seeds"
                added = [e for e in seeds.iterdir() if e.stat().st_mtime >= report.started] if seeds.is_dir() else []
                totals = [measure.disk_usage(e) for e in added]
                resources["disk"]["engine_image_seeds_added_this_run"] = {
                    k: sum(t[k] for t in totals) for k in ("apparent_bytes", "allocated_bytes")}
            with report.step("delete: confirmation required, VM removed, artifacts kept"):
                cli.fails("delete", APP, "--confirm", "wrong")
                cli.op("delete", APP, "--confirm", APP)
                check(cli("list") == [], "app deleted")
                check(machine not in {m["name"] for m in sampler.machines()}, "machine removed from the engine")
                check((home / "apps" / APP / "backups").is_dir(), "backups retained")
            with report.step("every CLI command exercised"):
                # From --help, so a new subcommand fails this step until the e2e exercises it.
                text = subprocess.run([str(cli_exe), "--help"], capture_output=True, text=True, check=True).stdout
                section = text.split("Commands:", 1)[1].split("Options:", 1)[0]
                commands = {line.split()[0] for line in section.splitlines() if line.strip()} - {"help"}
                check(commands <= cli.used, f"unexercised: {sorted(commands - cli.used)}")
        ok = True
    except Exception as e:  # noqa: BLE001 - the report records any failure with its traceback
        traceback.print_exc()
        (out / "failure.txt").write_text(f"{e}\n\n{traceback.format_exc()}")
    finally:
        sampler.stop()
        if daemon and daemon.proc and daemon.proc.returncode is None:
            if not ok:
                with contextlib.suppress(Exception):  # leave no VM behind after a failed run
                    cli.op("delete", APP, "--confirm", APP, timeout=300)
            daemon.stop()
        host_server.shutdown()
        for log in (home / "jobs").glob("*.log") if (home / "jobs").is_dir() else []:
            shutil.copy2(log, out / f"job-{log.name}")
        resources["peak_rss_bytes"] = {"daemon": max(daemon.peaks) if daemon and daemon.peaks else None, "cli": cli.peak or None}
        resources["vm"] = sampler.peaks
        resources["cli_calls"] = cli.calls
        daemon_log = out / "daemon.log"
        resources["jobs"] = [j for j in measure.job_durations(daemon_log.read_text()) if j["app"] == APP] if daemon_log.exists() else []
        (out / "resources.json").write_text(json.dumps(resources, indent=2) + "\n")
        (out / "summary.md").write_text(summary(resources, ok))
        report.write(ok, {"provider": resources.get("provider"), "platform": sys.platform})
        shutil.rmtree(work, ignore_errors=True)
    print((out / "summary.md").read_text())
    return 0 if ok else 1


def summary(r: dict[str, Any], ok: bool) -> str:
    rss, disk = r.get("peak_rss_bytes", {}), r.get("disk", {})
    lines = [f"# e2e {'passed' if ok else 'FAILED'} ({r.get('provider', 'unknown provider')})", "",
             "| Process | Peak RSS |", "|---|---|",
             f"| defuss-containerd | {measure.mib(rss.get('daemon'))} |", f"| defuss-container (max of {r.get('cli_calls')} calls) | {measure.mib(rss.get('cli'))} |"]
    for name, peak in sorted(r.get("vm", {}).items()):
        kind = "builder VM" if name.startswith("defuss-build-") else "engine image-seed helper VM" if name.startswith("image-seed-") else "app VM"
        lines.append(f"| {kind} host process | {measure.mib(peak.get('host_rss_bytes'))} (guest used {measure.mib(peak.get('guest_used_bytes'))} of {measure.mib(peak.get('guest_total_bytes'))}) |")
    lines += ["", "| Disk | Allocated | Apparent |", "|---|---|---|"]
    for key, value in disk.items():
        if isinstance(value, dict) and "allocated_bytes" in value:
            lines.append(f"| {key} | {measure.mib(value['allocated_bytes'])} | {measure.mib(value['apparent_bytes'])} |")
    for rel, files in disk.get("release_each", {}).items():
        lines.append(f"| release {rel[:8]} | " + ", ".join(f"{k} {measure.mib(v)}" for k, v in files.items()) + " | |")
    jobs = r.get("jobs", [])
    if jobs:
        # The first deploy creates the machine from the release image: the app's start from scratch.
        lines += ["", f"| {APP} job (daemon log) | Seconds |", "|---|---|"]
        first_deploy = next((i for i, j in enumerate(jobs) if j["action"] == "Deploy"), None)
        for i, j in enumerate(jobs):
            note = " (from scratch: machine created, booted, healthy)" if i == first_deploy else "" if j["ok"] else " (failed, as the e2e intends)"
            lines.append(f"| {j['action']}{note} | {j['seconds']:.2f} |")
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    sys.exit(main())
