# defuss-container 0.1.0

A local Bun application manager: Rust CLI + authenticated HTTP daemon + a defuss UI packaged with defuss-tauri. Builds an HTTPS Git repository into a Bun distroless OCI image without Docker or Podman. Runs each application in a persistent smolvm Linux microVM using KVM, Hypervisor.framework, or Windows Hypervisor Platform.

This is the first source release. Core tests and UI build are automated and pass on Linux x86_64 and macOS arm64; full VM and native desktop execution require the hardware gates in [docs/VERIFICATION.md](docs/VERIFICATION.md). It is not a signed, notarized, cross-platform-certified binary release.

## Included

- `defuss-containerd`: loopback-only HTTP API, private bearer credentials, durable SQLite metadata, bounded background jobs, per-application operation serialization, restart reconciliation.
- `defuss-container`: HTTP client for create, list, inspect, build, deploy, start, stop, rollback, backup, restore, logs and jobs.
- Desktop: defuss frontend + defuss-tauri 0.3.0 static-assets host. A small Rust extension reads local connection credentials and forwards HTTP requests. Closing the GUI does not stop the daemon or machines.
- Linux KVM, macOS HVF and Windows WHP provider preflights, sharing a pinned engine CLI adapter.
- Builds fetch over HTTPS Git, pin an immutable commit, archive the source safely, build in a disposable Bun VM and produce a digest-verified OCI archive. Repository scripts never execute on the host.
- Full-durability guest storage; read-only input/config shares; writable output share; guest hostname `host` maps to the physical host through the virtio-net gateway.
- Graceful `stop` script + SIGTERM + bounded forced shutdown; health-gated deployment with application-code rollback.
- Consistent application-data backups while the workload is stopped, checksummed restore, pre-restore safety backup.
- Engine installer with embedded SHA-256 pins, per-user OS service registration, example SQLite backend, real-VM acceptance runner and filesystem benchmark.

## Prerequisites

| Host | Runtime prerequisite |
|---|---|
| Linux x86_64 / arm64, glibc | `/dev/kvm` available and accessible to the user; vendor smolvm libraries |
| macOS Apple Silicon | Hypervisor.framework; the pinned smolvm bundle (ad-hoc signed with the hypervisor entitlement) installed by `scripts/install.py`, not a browser download. See [macOS notes](docs/VERIFICATION.md#macos-notes) |
| Windows x86_64 | Windows Hypervisor Platform enabled, hardware virtualization enabled, reboot if required |

Development: Rust 1.99.1 (pinned in `rust-toolchain.toml`), C compiler (Xcode Command Line Tools on macOS), Git, Python 3.11+. Desktop and tests: Bun 1.4 (package manager, builder tests), Node 24 (Playwright tests) and [Tauri OS prerequisites](https://v2.tauri.app/start/prerequisites/). The installed application manager builds and runs Bun inside its machines; host Bun is a development tool only.

The engine is pinned to **smolvm 1.32.4**. This release uses its CLI directly: the Rust smolmachines SDK currently documents local Linux/macOS support, while the engine provides the Windows path too. Do not silently substitute another engine version. Engine bundles contain third-party libraries and a Linux guest: this is not third-party-free virtualization.

## Install from source

From this directory:

```sh
python3 scripts/install.py --service
```

This downloads and checksum-verifies the complete platform engine bundle, compiles both Rust binaries, installs under the current user's data directory, and registers/starts a login service. It does not install Rust/Git, enable hypervisors, change KVM permissions, remove quarantine, disable Gatekeeper/SmartScreen, or elevate privileges. macOS/Windows distribution signing remains a release responsibility.

Default state/install directories:

- Linux: `$XDG_DATA_HOME/defuss-container`, otherwise `~/.local/share/defuss-container`.
- macOS: `~/Library/Application Support/defuss-container`.
- Windows: `%LOCALAPPDATA%\defuss-container`.

The daemon listens on `127.0.0.1:63705`; `--port` / `DEFUSS_CONTAINER_PORT` selects another port, and `0` lets the OS pick one. Clients read the address from `connection.json`, so they need no port setting.

Add its `bin` directory to PATH. `--home` / `DEFUSS_CONTAINER_HOME` selects a different directory; use the same value for daemon, CLI and GUI. The GUI inherits the environment of the process launching it. The engine's VM disks remain in **smolvm's own per-user storage**, discoverable through `smolvm machine data-dir --name NAME`; moving the defuss directory alone does not move VM data.

Without service installation:

```sh
python3 scripts/install.py
# Start the installed defuss-containerd binary, or for development:
cargo run --bin defuss-containerd -- --engine /absolute/path/to/smolvm
# A second terminal:
cargo run --bin defuss-container -- doctor
```

Services: Linux `systemctl --user status defuss-container`; macOS `launchctl print gui/$(id -u)/org.defuss.container`; Windows Task Scheduler, task `Defuss Container`. These run in the user's session. Linux operation before login requires administrator-managed lingering; Windows unattended boot requires a separately provisioned service account. Neither is silently configured.

## Application contract

The selected Git tree must contain:

- `package.json` with nonempty `scripts.start` and `scripts.stop`.
- A committed `bun.lock` or `bun.lockb` compatible with Bun **1.4.4**.
- Optional `scripts.build`, enabled in the app specification.
- Runtime dependencies in `dependencies`, not solely in `devDependencies`.
- A start script that stays in the foreground. Bind HTTP to `0.0.0.0`, not guest loopback, for published ports.
- A stop script that completes promptly. The supervisor invokes it, then signals the entire start process group. The app must handle SIGTERM and close databases.

`scripts.start`, `scripts.stop` and `scripts.build` run with `bun exec` (Bun Shell) and `node_modules/.bin` on `PATH`; pre/post lifecycle scripts are not run. Distroless has no system shell, package manager, compiler, or arbitrary Unix tools. A script invoking `bash`, `curl`, `python`, `make`, etc. is outside this image contract. Native packages need compatible prebuilt Linux binaries. Dependency lifecycle scripts are disabled; an explicitly enabled package build script runs inside the builder VM. No emulated foreign architecture builds, Git submodules or Git LFS materialization in v0.1.

Reserved top-level application paths are `var`, `tmp`, `input`, `output`, `etc`; they are excluded from the code artifact and become links to `/app/<name>`. `.git`, `node_modules` from the source tree, `.npmrc`, and `.env*` other than `.env*.example` are excluded. Supply environment values in the app spec. Git credentials configured for the daemon user can authenticate HTTPS fetches; they are not forwarded into the builder. Private npm registry authentication is not implemented.

| App-relative path | Backing | Lifecycle |
|---|---|---|
| `var` | Persistent guest ext4 directory | Survives stop/start, deploy, rollback; included in backups |
| `tmp` | Guest-local directory | Cleared before each application start |
| `input` | `<mounts>/input`, read-only host share | Host-controlled input |
| `output` | `<mounts>/output`, writable host share | Host-visible output |
| `etc` | `<mounts>/etc`, read-only host share | Host-controlled configuration |

`var` and `tmp` are deliberately **not host-directory mounts**. `/app/var/.defuss-*` is reserved for restoration staging. Host shares are separate from the daemon's private state. There is no staged/sync-back mount in the persistence path. Full disk durability is explicitly requested; database `fsync` still depends on the engine, host filesystem and hardware honoring flushes.

The `host` name resolves to `100.96.0.1`, the explicitly configured virtio-net gateway. Pinned smolvm maps gateway TCP destinations to host loopback. Runtime guests have outbound networking and may reach host services. Builds use `--block-private` to prevent host/private-network access. Published application ports use the engine's host listener; use firewall rules for network exposure. The management API itself binds only `127.0.0.1`.

## First application

Commit `examples/hello` to an HTTPS Git repository, either at its root or in a subdirectory. Its endpoints cover health (`/health`), SQLite persistence (`/counter`), host reachability (`/host`), the output share (`/output`), the input/etc shares, `tmp` reset and environment (`/shares`), and the deployed version (`/version`, read from an optional `version.txt`).

Create `app.json` with your actual repository and absolute share directory:

```json
{
  "name": "hello",
  "repository": "https://github.com/your-org/your-bun-app.git",
  "git_ref": "main",
  "subdirectory": "",
  "mounts": "/absolute/path/to/hello-shares",
  "cpus": 2,
  "memory_mib": 1024,
  "storage_gib": 20,
  "ports": [{"host": 8080, "guest": 3000}],
  "env": {"PORT": "3000"},
  "build": false,
  "health": {"port": 3000, "path": "/health"},
  "stop_timeout_seconds": 30
}
```

On Windows use a JSON-escaped drive path, e.g. `C:\\Users\\you\\defuss-shares\\hello`.

```sh
defuss-container create app.json
defuss-container build hello
# Build returns a job id. Pass that actual id:
defuss-container wait JOB_ID
# The completed job's result contains the immutable release id:
defuss-container deploy hello RELEASE_ID
defuss-container wait DEPLOY_JOB_ID
defuss-container show hello
defuss-container logs hello
# Any API route through the same client the desktop bridge uses (JSON output):
defuss-container api GET /v1/apps/hello/releases
```

Every action returns immediately with a durable job record; use `wait`, `job`, `job-log`, or the GUI for completion. `wait` returns nonzero for failed/interrupted jobs. Job submission is not success. CPU/memory/mount/port specifications are immutable after registration in this first release; deploy new commits from the configured Git ref. For changes, back up, delete/re-register with the same app name, deploy, restore the backup. Delete requires `--confirm APP_NAME` and removes the VM disk. Host shares, release artifacts and backups remain.

```sh
defuss-container backup hello
defuss-container backups hello
defuss-container restore hello BACKUP_ID
defuss-container rollback hello
defuss-container stop hello
defuss-container start hello
```

Rollback switches code only. It does **not** undo database migrations. Failed readiness selects the previous code release and restarts it if it was previously running. A base-image digest change is refused for an existing machine; migrate through a backup and fresh machine. A failed restore leaves the workload stopped and retains its safety backup. If the host dies during restore, use the recorded pre-restore backup; restoring a large directory is not an atomic filesystem transaction.

## Desktop

```sh
make gui         # native app for this OS: desktop/dist-tauri-build/release (binary and bundle)
make start-gui   # open it against the `make start` daemon, starting that daemon if it is down
make desktop     # build the UI and generate the host without compiling it
```

The UI does not start the daemon, own its lifetime, or invoke smolvm; `make start-gui` starts it first. While the daemon is unreachable, the UI retries after 1, 2, 4 ... up to 30 s, and the header shows a Reconnect button that retries at once. Its Rust extension makes authenticated HTTP requests; daemon credentials never enter JavaScript. A browser-only Vite preview displays the UI but cannot control the daemon because the native bridge is absent.

## Test / benchmark

The `Makefile` is the interface for humans, agents and CI (`make` lists the verbs). Development daemon and clients, all on the state directory `var/home`:

```sh
make start                        # build, install into tmp/e2e/install and run the daemon detached (logs: make log)
make cli                          # build the CLI (and daemon) only
make start-cli ARGS="show hello"  # run the CLI against that daemon (default ARGS=list)
make gui && make start-gui        # build and open the desktop app against it
make stop
```

Gates:

```sh
make setup      # rustup + pinned toolchain, locked Rust/bun dependencies, Playwright browsers, cargo-llvm-cov
make lint       # rustfmt, Clippy and oxlint (warnings fail), desktop type check
make test       # Rust, guest helper and builder tests, daemon/CLI smoke, UI interaction tests (no VM)
make coverage   # Rust line coverage including the real daemon/CLI smoke run; gate floor 60%
make e2e        # real VM: installed CLI/daemon/engine, full guest lifecycle, GUI in Chromium and WebKit
make verify     # lint test coverage e2e, what CI runs
make metrics    # peak RSS and disk use from the last e2e run
```

`make e2e` installs the release build and the checksummed engine into `tmp/e2e/install` and builds the UI. Then `tests/e2e/run.py` serves a fixture repository over local HTTPS and drives the installed CLI through build, deploy, guest wiring (shares, `host` alias, `tmp`), SQLite persistence, backup and restore, redeploy, rollback, a crashing release, daemon restart and delete. Every CLI command runs at least once. Finally `desktop/tests/e2e.test.mjs` clicks through the built UI in Playwright against the same live daemon. Results, logs, screenshots, peak RSS and disk use go to `output/e2e/`. It needs a hypervisor and network access to Docker Hub and the npm registry.

Explicit network gate (pulls and verifies the real Bun image):

```sh
cargo test --test registry -- --ignored --nocapture
```

Guest filesystem benchmark against a deployed app of the `make start` daemon:

```sh
make bench APP=hello
```

The benchmark measures guest-local versus shared-output I/O, including full-sync SQLite commits. It labels reads as cached and does not claim physical-disk bandwidth.

See [architecture](ARCH.md), [API](docs/API.md), [security](docs/SECURITY.md) and [verification](docs/VERIFICATION.md).
