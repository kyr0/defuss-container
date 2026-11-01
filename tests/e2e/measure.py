"""Resource measurement for the e2e suite: per-process peak RSS, VM host-process RSS, guest memory, disk use, job time.

WHY per-process peak via wait4 instead of sampling: sampling misses short peaks of the CLI and daemon; the kernel
tracks the high-water mark exactly. VMs are not our children, so their host processes are sampled instead.
"""
from __future__ import annotations

import ctypes
import datetime
import json
import os
import pathlib
import re
import subprocess
import sys
import threading
import time
from dataclasses import dataclass, field
from typing import Any

WINDOWS = os.name == "nt"


@dataclass
class Result:
    code: int
    stdout: str
    stderr: str
    peak_rss: int | None  # bytes; None where the platform cannot report it


def _maxrss_bytes(ru_maxrss: int) -> int:
    return ru_maxrss if sys.platform == "darwin" else ru_maxrss * 1024  # macOS bytes, Linux KiB


def _windows_peak(handle: int) -> int | None:
    class Counters(ctypes.Structure):
        _fields_ = [("cb", ctypes.c_ulong), ("PageFaultCount", ctypes.c_ulong)] + [
            (n, ctypes.c_size_t) for n in ("PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage",
                                           "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage", "QuotaNonPagedPoolUsage",
                                           "PagefileUsage", "PeakPagefileUsage")]
    c = Counters()
    c.cb = ctypes.sizeof(c)
    ok = ctypes.windll.psapi.GetProcessMemoryInfo(ctypes.c_void_p(handle), ctypes.byref(c), c.cb)  # type: ignore[attr-defined]
    return int(c.PeakWorkingSetSize) if ok else None


def reap(proc: subprocess.Popen) -> int | None:
    """Wait for `proc` and return its peak RSS in bytes (the process must not have been waited for yet)."""
    if WINDOWS:
        proc.wait()
        return _windows_peak(int(proc._handle))  # type: ignore[attr-defined]
    _, status, usage = os.wait4(proc.pid, 0)
    proc.returncode = os.waitstatus_to_exitcode(status)
    return _maxrss_bytes(usage.ru_maxrss)


def run(argv: list[str], env: dict[str, str] | None = None, timeout: float = 1800) -> Result:
    proc = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
    out: list[bytes] = []
    err: list[bytes] = []
    readers = [threading.Thread(target=lambda s=s, b=b: b.append(s.read()), daemon=True)
               for s, b in ((proc.stdout, out), (proc.stderr, err))]
    for r in readers:
        r.start()
    timer = threading.Timer(timeout, proc.kill)
    timer.start()
    try:
        peak = reap(proc)
    finally:
        timer.cancel()
    for r in readers:
        r.join()
    return Result(proc.returncode, b"".join(out).decode(errors="replace"), b"".join(err).decode(errors="replace"), peak)


def process_rss(pid: int) -> int | None:
    """Current RSS of a process we did not start, in bytes."""
    try:
        if WINDOWS:
            out = subprocess.run(["tasklist", "/FI", f"PID eq {pid}", "/FO", "CSV", "/NH"], capture_output=True, text=True).stdout
            fields = [f.strip('"') for f in out.strip().split('","')]
            return int(fields[-1].replace(",", "").replace(".", "").split()[0]) * 1024 if len(fields) >= 5 else None
        out = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True).stdout.strip()
        return int(out) * 1024 if out else None
    except (OSError, ValueError):
        return None


@dataclass
class MachineSampler:
    """Polls the engine for machines created after `start()` and records peak host RSS and guest memory use."""

    engine: str
    interval: float = 1.0
    baseline: set[str] = field(default_factory=set)
    peaks: dict[str, dict[str, int]] = field(default_factory=dict)
    _stop: threading.Event = field(default_factory=threading.Event)
    _thread: threading.Thread | None = None

    def _engine(self, *args: str) -> str | None:
        # Short timeouts: a busy VM can stall `stats`, and one slow call must not hide a short-lived builder VM.
        try:
            p = subprocess.run([self.engine, "machine", *args], capture_output=True, text=True, timeout=5)
        except subprocess.TimeoutExpired:
            return None
        return p.stdout if p.returncode == 0 else None

    def machines(self) -> list[dict]:
        try:
            return json.loads(self._engine("ls", "--json") or "[]")
        except ValueError:
            return []

    def start(self) -> None:
        self.baseline = {m["name"] for m in self.machines()}
        self._thread = threading.Thread(target=self._loop, daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        if self._thread:
            self._thread.join(timeout=30)

    def _record(self, name: str, key: str, value: int | None) -> None:
        if value is not None:
            entry = self.peaks.setdefault(name, {})
            entry[key] = max(entry.get(key, 0), value)

    def _loop(self) -> None:
        while not self._stop.is_set():
            for m in self.machines():
                name = m.get("name", "")
                if name in self.baseline or m.get("state") != "running":
                    continue
                if m.get("pid"):
                    self._record(name, "host_rss_bytes", process_rss(int(m["pid"])))
                try:
                    memory = json.loads(self._engine("stats", "--name", name, "--json") or "")["memory"]
                    self._record(name, "guest_used_bytes", int(memory["used_bytes"]))
                    self._record(name, "guest_total_bytes", int(memory["total_bytes"]))
                except (ValueError, KeyError, TypeError):
                    pass
            self._stop.wait(self.interval)


def disk_usage(path: pathlib.Path) -> dict[str, int]:
    """Apparent and allocated bytes below `path`. Allocated counts only written blocks of sparse VM disks
    (POSIX st_blocks); Windows reports apparent size for both."""
    apparent = allocated = 0
    if not path.exists():
        return {"apparent_bytes": 0, "allocated_bytes": 0}
    files = [path] if path.is_file() else [p for p in path.rglob("*") if p.is_file() and not p.is_symlink()]
    for f in files:
        try:
            st = f.lstat()
        except OSError:
            continue
        apparent += st.st_size
        allocated += getattr(st, "st_blocks", 0) * 512 if not WINDOWS else st.st_size
    return {"apparent_bytes": apparent, "allocated_bytes": allocated}


def mib(n: int | None) -> str:
    return "n/a" if n is None else f"{n / 1048576:.1f} MiB"


def now() -> float:
    return time.monotonic()


# One daemon log line per job event, ISO-8601 UTC timestamp first (src/service.rs).
_JOB_EVENT = re.compile(r"^(\S+Z)\s+\w+\s+\S+ job (started|succeeded|failed) job=(\S+) app=(\S+)(?: action=(\w+))?")


def job_durations(log: str) -> list[dict[str, Any]]:
    """Each job's duration as the daemon saw it, in start order.

    WHY the daemon log instead of timing `defuss-container wait`: the CLI polls, so its wall time adds up to one poll
    interval; the daemon stamps start and end in microseconds. VERIFIED: the log lines carry `job started ... action=`
    and `job succeeded|failed job=` (output/e2e/daemon.log).
    """
    started: dict[str, tuple[datetime.datetime, str, str]] = {}
    jobs: list[dict[str, Any]] = []
    for line in log.splitlines():
        m = _JOB_EVENT.match(line)
        if not m:
            continue
        at, event, job, app, action = m.groups()
        when = datetime.datetime.fromisoformat(at.replace("Z", "+00:00"))
        if event == "started":
            started[job] = (when, app, action or "")
        elif job in started:
            begin, app, action = started.pop(job)
            jobs.append({"app": app, "action": action, "ok": event == "succeeded", "seconds": round((when - begin).total_seconds(), 3), "start": begin.isoformat()})
    return sorted(jobs, key=lambda j: j["start"])
