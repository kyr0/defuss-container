# defuss-container

[![CI](https://github.com/kyr0/defuss-container/actions/workflows/verify.yml/badge.svg)](https://github.com/kyr0/defuss-container/actions/workflows/verify.yml)
[![License](https://img.shields.io/github/license/kyr0/defuss-container)](LICENSE)

Run each of your Bun applications in its own Linux microVM on your own computer: built from a Git commit, deployed with a health check, and managed through a local daemon, a CLI and a desktop app.

## TL;DR

You want to run your own Bun services on your machine. Their install and build scripts should not run on the host, a release that fails its health check should not stay deployed, and the data should survive deploys. You also do not want to install Docker or Podman for it.

defuss-container builds an HTTPS Git repository inside a disposable microVM and runs each application in a persistent smolvm microVM, on KVM (Linux), Hypervisor.framework (macOS) or Windows Hypervisor Platform. A user-level daemon keeps applications, releases and jobs in SQLite. The CLI and the desktop app are clients of its loopback API.

- **Repository code runs only in VMs.** On the host, Git only fetches and archives. Dependency installs and `scripts.build` run in a builder VM that cannot reach private networks, and the host checks every path, link, type and size of the result before packaging it.
- **One persistent VM per application**, with its own CPU, memory and disk limits from the app spec. `./var` lives on the VM's disk with full flush durability and survives stop, start, deploy and rollback.
- **Health-gated deploys.** A release that fails its readiness check is replaced by the previous release, which restarts if it was running. `rollback` switches code back on request.
- **Backups and restore.** Backups archive `./var` while the workload is stopped. Restore verifies the checksum and takes a safety backup first.
- **Durable jobs.** Every action returns a job at once. Jobs outlive the CLI and the desktop app, and a daemon restart marks unfinished jobs interrupted and brings each app back to its desired state.
- **Local only.** The API listens on `127.0.0.1:63705`, rejects browser origins and rotates its 256-bit token on every start. The token never reaches JavaScript or a guest.

Status: release 0.1.0, the first source release. The full end-to-end suite (`make e2e`) passes on macOS 26.7.1, Apple Silicon. The CI jobs for Linux KVM and Windows WHP have not passed yet. There are no signed or notarized binaries. Details: [docs/VERIFICATION.md](docs/VERIFICATION.md#platform-status).

## Quick start

| Host | Needs |
|---|---|
| macOS, Apple Silicon | Hypervisor.framework; Xcode Command Line Tools |
| Linux x86_64 or arm64, glibc | `/dev/kvm` readable and writable by your user |
| Windows x86_64 | Windows Hypervisor Platform and hardware virtualization enabled |

Building from source needs Rust (rustup installs the 1.99.1 toolchain pinned in `rust-toolchain.toml`), a C compiler, Git and Python 3.11 or newer. The desktop app and the tests also need Bun 1.4, Node 24 and the [Tauri OS prerequisites](https://v2.tauri.app/start/prerequisites/). Applications get their Bun from the image inside their VM, so host Bun is a development tool only.

Try it from a checkout. `make start` builds the release binaries, downloads and checksums the pinned engine into `tmp/e2e/install`, and runs the daemon in the background on the state directory `var/home`:

```sh
git clone https://github.com/kyr0/defuss-container && cd defuss-container
make start
make start-cli ARGS="doctor"
```

When it works, `doctor` reports the engine and a ready hypervisor (an excerpt, on macOS):

```json
{
  "engine_error": null,
  "expected_engine": "1.32.4",
  "hypervisor_error": null,
  "hypervisor_ready": true,
  "provider": "macos-hvf",
  "version": "smolvm 1.32.4"
}
```

`make stop` stops the daemon.

For daily use, install into your user's data directory and register a login service. The service registration has not been run on any platform yet:

```sh
python3 scripts/install.py --service
```

The installer downloads the engine bundle and checks it against the SHA-256 pin embedded in the installer, compiles both Rust binaries and registers the service: systemd user unit `defuss-container` on Linux, LaunchAgent `org.defuss.container` on macOS, scheduled task `Defuss Container` on Windows. It does not install Rust or Git, enable hypervisors, change KVM permissions, remove quarantine attributes or elevate privileges. Add the `bin` directory inside the data directory to `PATH`:

| OS | Data directory |
|---|---|
| Linux | `$XDG_DATA_HOME/defuss-container`, else `~/.local/share/defuss-container` |
| macOS | `~/Library/Application Support/defuss-container` |
| Windows | `%LOCALAPPDATA%\defuss-container` |

The services run in your login session. Running before login needs administrator-managed lingering on Linux and a separately provisioned service account on Windows; the installer configures neither.

## Usage

### Deploy an application

Commit your app (or `examples/hello`) to an HTTPS Git repository and describe it in `app.json`. `mounts` is an absolute host directory that receives the `./input`, `./output` and `./etc` shares:

```json
{
  "name": "hello",
  "repository": "https://github.com/your-org/your-bun-app.git",
  "git_ref": "main",
  "mounts": "/absolute/path/to/hello-shares",
  "ports": [{"host": 8080, "guest": 3000}],
  "env": {"PORT": "3000"},
  "health": {"port": 3000, "path": "/health"}
}
```

Register, build and deploy it. Each action prints a job; `wait` blocks until it ends and exits nonzero when the job failed or was interrupted. The finished build job's `result` holds the release id:

```sh
defuss-container create app.json
defuss-container build hello            # prints a job with its id
defuss-container wait JOB_ID            # the result holds the release id
defuss-container deploy hello RELEASE_ID
defuss-container wait DEPLOY_JOB_ID
defuss-container show hello             # desired and observed state
curl http://127.0.0.1:8080/health
```

`examples/hello` answers `/health`, a SQLite counter on `/counter`, the host alias on `/host`, the shares on `/shares` and `/output`, and its version on `/version`.

### Operate it

```sh
defuss-container logs hello             # last 200 workload log lines
defuss-container stop hello
defuss-container start hello
defuss-container backup hello
defuss-container backups hello
defuss-container restore hello BACKUP_ID
defuss-container rollback hello         # previous code release; data stays
defuss-container delete hello --confirm hello
```

| Command | Does |
|---|---|
| `doctor` | Engine version, provider and hypervisor checks |
| `list`, `show NAME` | Registered apps; one app with its observed engine state |
| `create FILE` | Register an app spec |
| `build NAME` | Fetch, build and store a new release |
| `deploy NAME RELEASE` | Start a release behind the health check; on failure the previous release returns |
| `start`, `stop NAME` | Set the desired state and act on it |
| `rollback NAME` | Switch to the previous code release |
| `backup NAME`, `backups NAME`, `restore NAME BACKUP` | Archive `./var`, list archives, restore one |
| `delete NAME --confirm NAME` | Remove the app and its VM disk; host shares, releases and backups stay |
| `releases NAME`, `logs NAME` | Successful builds; last 200 workload log lines |
| `jobs`, `job ID`, `job-log ID`, `wait ID [--timeout S]` | Recent jobs, one job, its log, wait for it (default 1800 s) |
| `api METHOD PATH [--body JSON]` | Any API route through the client the desktop bridge uses |

Rollback switches code only and does not undo database migrations. CPU, memory, mounts and ports are fixed at registration in this release. To change them: back up, delete, register again with the same name, deploy, restore. An existing machine refuses a release built on a different Bun base image digest; move the app to a fresh machine the same way.

### Daemon and HTTP API

The daemon runs as the login service (`install.py --service`) or, in a checkout, as `make start`. The CLI, the desktop app and your scripts reach it the same way.

```sh
make start          # development daemon on ./var/home, logs in var/log
make status         # running, dead (stale pid) or stopped
make log            # last daemon log lines
make stop           # SIGTERM: running jobs finish first; SIGKILL after 60 s
systemctl --user status defuss-container                 # installed service, Linux
launchctl print gui/$(id -u)/org.defuss.container        # installed service, macOS
defuss-containerd --home DIR --engine PATH --port 63705  # run the daemon yourself
```

On Windows the service is the scheduled task `Defuss Container` (`Start-ScheduledTask -TaskName 'Defuss Container'`). Only the CLI's commands and the `make` verbs above have run in tests; the login service has not.

The HTTP API listens on `127.0.0.1` only. Every request needs the bearer token from `connection.json` in the state directory, which rotates on each daemon start, and requests with a browser `Origin` are rejected:

```sh
conn=./var/home/connection.json
token=$(jq -r .token $conn)
api=$(jq -r .address $conn)
curl -s -H "Authorization: Bearer $token" $api/v1/apps
curl -s -X POST -H "Authorization: Bearer $token" -H 'Content-Type: application/json' \
  -d '{"action":"start"}' $api/v1/apps/hello/actions
defuss-container api GET /v1/jobs   # the same through the CLI's client
```

[docs/API.md](docs/API.md) lists every route, action body and error.

### Desktop app

```sh
make gui         # build the native app for this OS into desktop/dist-tauri-build/release
make start-gui   # open it against the `make start` daemon, starting the daemon if it is down
```

The app shows apps, releases, logs, jobs and backups, registers apps and runs every app action: build, deploy, start, stop, rollback, backup, restore and delete. Its window is a resizable layout built from [defuss-shadcn](https://shadcn.defuss.org) components: a sidebar, a grid of apps, the selected app with its releases and backups as a tree, and recent jobs beside a filterable log viewer. It never starts the daemon or invokes the engine itself, and closing it leaves the daemon and the machines running. While the daemon is unreachable, the app retries after 1, 2 and 4 s, doubling up to 30 s, and its header shows a Reconnect button that retries at once. Its Rust extension makes the authenticated requests, so the token never enters JavaScript.

### Application contract

The selected Git tree needs:

- `package.json` with nonempty `scripts.start` and `scripts.stop`, and runtime dependencies in `dependencies`.
- A committed `bun.lock` or `bun.lockb` that Bun 1.4.4 accepts.
- A start script that stays in the foreground and binds published ports on `0.0.0.0`, not guest loopback.
- A stop script that finishes promptly, and a process that closes its databases on SIGTERM. The supervisor runs `scripts.stop`, then sends SIGTERM to the start script's process group.
- Optional `scripts.build`, run when the spec sets `"build": true`.

Scripts run with `bun exec` (Bun Shell) and `node_modules/.bin` on `PATH`; pre and post lifecycle scripts do not run. The distroless image has no system shell, package manager, compiler or Unix tools, so a script that calls `bash`, `curl`, `python` or `make` fails there. Native packages need prebuilt Linux binaries. Dependency lifecycle scripts are disabled. Git submodules, Git LFS, foreign-architecture builds and private npm registries are not supported in 0.1.0.

The top-level paths `./var`, `./tmp`, `./input`, `./output` and `./etc` are reserved. They are left out of the release and linked to these locations:

| Path | Backing | Lifecycle |
|---|---|---|
| `./var` | Directory on the VM's ext4 disk | Survives stop, start, deploy and rollback; in backups |
| `./tmp` | Directory inside the VM | Emptied before every start |
| `./input` | `<mounts>/input`, read-only host share | Written by you on the host |
| `./output` | `<mounts>/output`, writable host share | Visible on the host |
| `./etc` | `<mounts>/etc`, read-only host share | Configuration you provide |

`.git`, `node_modules`, `.npmrc` and `.env*` files other than `.env*.example` are left out of the release; put environment values in the spec. Inside the VM, the name `host` resolves to `100.96.0.1`, the virtio-net gateway, which the engine maps to the host's loopback. Running apps have outbound network access and can reach host services through `host`; builds cannot reach private networks.

## Configuration

### App spec

| Field | Default | Allowed |
|---|---|---|
| `name` | required | `[a-z][a-z0-9-]{0,47}` |
| `repository` | required | HTTPS Git URL without credentials, query or fragment; use Git credential configuration |
| `git_ref` | `HEAD` | a ref `git fetch` accepts, such as a branch or tag; no whitespace, no leading `-` |
| `subdirectory` | `""` | relative path without `..` |
| `mounts` | required | absolute host directory without symlinked components, outside the daemon's state directory |
| `cpus` | 2 | 1 to 64 |
| `memory_mib` | 1024 | 256 to 262144 |
| `storage_gib` | 20 | 2 to 1024 |
| `ports` | `[]` | up to 16 `{"host", "guest"}` pairs, unique, host port 1024 or higher |
| `env` | `{}` | up to 128 keys; `DEFUSS_*` is reserved for the supervisor. `BUN_OPTIONS` and the like reach only the app's processes |
| `build` | `false` | `true` runs `scripts.build` in the builder VM |
| `health` | none | `{"port", "path"}`; the port must be a published guest port |
| `stop_timeout_seconds` | 30 | 5 to 240 |

Unknown fields are rejected. On Windows, write `mounts` as a JSON-escaped drive path, for example `C:\\Users\\you\\defuss-shares\\hello`. On macOS, `/tmp`, `/var` and `/etc` are symlinks into `/private`, so put shares under your home directory.

### Environment

The Makefile exports a gitignored `.env` to every recipe; [.env.example](.env.example) lists every key.

| Key | Meaning | Default |
|---|---|---|
| `DEFUSS_CONTAINER_HOME` | State directory of daemon, CLI and desktop app; use the same value for all three | OS data directory (see above); `.env.example` sets `var/home` for development |
| `DEFUSS_CONTAINER_ENGINE` | Engine executable the daemon runs; an empty value stops the daemon from starting | `engine/smolvm` (`smolvm.exe` on Windows) in the state directory |
| `DEFUSS_CONTAINER_PORT` | Daemon API port; `0` lets the OS choose. Clients read the address from `connection.json` | `63705` |
| `RUST_LOG` | Daemon log filter | `info` |
| `SDKROOT` | macOS: an older SDK when the linker rejects the newest one (`unknown architecture arm64e.x1-macos`) | default SDK |
| `XDG_DATA_HOME`, `LOCALAPPDATA` | OS base directories for the default state directory | OS default |
| `BUN_EXECUTABLE` | Bun used by `tests/build.test.mjs` | `bun` |
| `CHROME_EXECUTABLE`, `UI_SCREENSHOT` | UI tests: a Chromium binary instead of Playwright's; a screenshot path | unset |
| `E2E_BROWSER` | Browser for `desktop/tests/e2e.test.mjs` run alone | `chromium` |
| `PORT`, `HOST_TEST_PORT`, `DEFUSS_APP`, `NODE_ENV` | Read by `examples/hello` inside the guest; the app spec sets the first two, the supervisor the others | `3000`, `8123`, `hello`, `production` |

The engine is pinned to smolvm 1.32.4. Its VM disks stay in smolvm's own per-user storage (`smolvm machine data-dir --name NAME`), so moving the state directory does not move VM data.

## How it works

The daemon fetches the Git commit, verifies the `oven/bun:1.4.4-distroless` base image by digest, builds in a disposable VM and stores the release as a checksummed OCI image. Each app owns one named machine; a supervisor inside it verifies the selected release, links the reserved paths and runs `scripts.start`. Deploys switch releases without replacing the VM or `./var`. [ARCH.md](ARCH.md) describes the build pipeline, the runtime and the design decisions; [docs/API.md](docs/API.md) the HTTP API; [docs/SECURITY.md](docs/SECURITY.md) the trust boundaries.

## Development

```bash
make setup && make verify
```

`make` lists every verb. The gate verbs and the metrics report:

| Command | Runs |
|---|---|
| `make format` | rustfmt, Prettier and Ruff rewrite the Rust, JS/TS/CSS/HTML/JSON and Python files |
| `make lint` | the same formatters as a check; Clippy, oxlint and Ruff with warnings denied; desktop type check |
| `make test` | Rust, guest helper and builder tests, daemon and CLI smoke run, UI interaction tests; no VM |
| `make coverage` | Rust line coverage including the smoke run; the gate needs 60% |
| `make e2e` | the installed artifact against a real VM, every CLI command, then the built UI in Playwright Chromium and WebKit |
| `make verify` | lint, test, coverage and e2e, as CI runs them |
| `make metrics` | peak RSS and disk use from the last e2e run |

The development daemon and its clients share the state directory `var/home`:

```sh
make start                        # build, install into tmp/e2e/install, run the daemon (logs: make log)
make start-cli ARGS="show hello"  # the CLI against that daemon (default ARGS=list)
make gui && make start-gui        # the desktop app against it
make bench APP=hello              # guest-local vs shared-output I/O of a deployed app
make site                         # the project site from docs/ on http://127.0.0.1:63706 (stop: make stop NAME=site)
make stop
```

`make e2e` needs a hypervisor and network access to Docker Hub and the npm registry. It serves a fixture repository over local HTTPS and drives the installed CLI through build, deploy, guest wiring, SQLite persistence, backup and restore, redeploy, rollback, a crashing release, a daemon restart and delete. Results, logs, screenshots, peak RSS and disk use go to `output/e2e/`. `cargo test --test registry -- --ignored --nocapture` pulls and verifies the real Bun image.

Start reading at [ARCH.md](ARCH.md), then [desktop/ARCH.md](desktop/ARCH.md) for the UI and [desktop/native/README.md](desktop/native/README.md) for its native bridge.

## License

MIT, © 2026 Aron Homberg. The engine bundle, crates and npm dependencies are third-party software under their own licenses.
