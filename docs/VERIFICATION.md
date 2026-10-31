# Verification

Automated gates cover the control plane, archive/build helpers, OCI packaging and the UI. They do not boot a VM. Only `scripts/vm-smoke.py` on real hardware proves builds, readiness, the `host` alias, shares, persistence, backup/restore, redeploy and rollback. Run it on every OS/architecture before distributing binaries.

## Gates

| Gate | Command | Proves | Needs |
|---|---|---|---|
| Format, lint | `make check` | rustfmt clean; Clippy with warnings denied | Rust 1.99.1 |
| Rust tests | `cargo test --locked` | API auth/Origin/Host, spec validation, SQLite job recovery, OCI layout, layer validation, process-group timeout; lifecycle (deploy, failed-deploy rollback, corrupt release, base change, restore failure) against a fake engine | — |
| Guest archive | `node --test tests/archive.test.mjs` | Traversal, escaping/absolute links, duplicates, checksum, truncation, size limit | Node 24 |
| Bun builder | `node --test tests/build.test.mjs` | Real `guest/build.mjs` run: lockfile install, scripts suppressed, build output kept, secrets/data excluded, dependency loadable | Bun 1.4.x |
| Control plane | `python3 scripts/control-smoke.py` (after `cargo build`) | Real daemon + CLI: auth, Origin rejection, failed job, single-instance lock, private files, restart, token rotation; no VM | Loopback listener |
| Registry | `cargo test --test registry -- --ignored --nocapture` | Pinned `oven/bun:1.4.4-distroless` resolves for this architecture; manifest and blob digests verify; image contains `/usr/local/bin/bun` | Docker Hub access |
| Engine bundle | `python3 scripts/install.py --engine-only --home DIR` | Bundle SHA-256 matches the embedded pin; unpacks; `DIR/engine/smolvm --version` prints `smolvm 1.32.4` | GitHub access |
| Desktop UI | `cd desktop && npm ci --ignore-scripts && npm run prepare:host` | TypeScript check, Vite build, defuss-tauri host generation | Node 24 |
| UI interaction | `cd desktop && npx playwright install chromium && npm run test:ui` | 7 browser tests against a mock native bridge | Chromium, loopback listener |
| Native bridge | `cargo check --manifest-path desktop/native/Cargo.toml --locked` | Tauri extension compiles for this OS | Tauri OS prerequisites |
| Hardware | `python3 scripts/vm-smoke.py --repository URL --subdirectory examples/hello` | Full lifecycle in a real VM (see above) | Running daemon, hypervisor, published fixture repo |
| Benchmark | `python3 scripts/benchmark.py APP --engine PATH --output FILE` | Guest-local `var` vs. shared `output` I/O, SQLite FULL commits; cached reads labelled as such | Deployed app |

`scripts/control-smoke.py` and `scripts/install.py` locate binaries through `cargo metadata`, so `CARGO_TARGET_DIR` and `build.target-dir` are honored.

## Platform status

| Platform | Date | Passed | Not yet run |
|---|---|---|---|
| Linux x86_64 | 2026-10-31 | Format, Clippy, 24 Rust tests, 12 archive tests, 2 Bun builder tests (Bun 1.4.4), control-plane smoke, registry (amd64), engine bundle, desktop UI build, 7 UI interaction tests, native `cargo check` with GTK/WebKit | Hardware gate (no `/dev/kvm`), benchmark, systemd service, native desktop link/launch |
| macOS 26.7.1, Apple Silicon (arm64) | 2026-10-31 | Format, Clippy, 24 Rust tests, 12 archive tests, 2 Bun builder tests (Bun 1.4.3), debug build, registry (arm64), engine bundle, engine CLI contract (below), desktop UI build, native `cargo check` | Control-plane smoke and UI interaction tests (the check environment blocked loopback listeners), Hypervisor.framework prerequisite (`sysctl` blocked), hardware gate, benchmark, launchd service, native desktop link/launch |
| Windows x86_64 | — | — | Everything |

The GitHub Actions matrix (`.github/workflows/verify.yml`) runs the automated gates on all three OSes. Hosted runners have no usable hypervisor, so CI success does not replace the hardware gate.

## macOS notes

**Toolchain.** Install the Xcode Command Line Tools and Rust through rustup; `rust-toolchain.toml` selects 1.99.1. If linking fails with `unknown architecture arm64e.x1-macos` in a `.tbd` file, the installed `ld` is older than the newest SDK (observed: CLT 26.6 `ld-1267` with `MacOSX27.0.sdk`). Update the Command Line Tools, or point the build at an older installed SDK:

```sh
export SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk
```

**Engine.** The pinned `smolvm-1.32.4-darwin-arm64.tar.gz` contains a bash wrapper `smolvm` and `smolvm-bin`. `smolvm-bin` is ad-hoc signed (no Developer ID) with `com.apple.security.hypervisor`, `com.apple.security.cs.allow-jit` and `com.apple.security.cs.disable-library-validation`. `scripts/install.py` downloads it without a quarantine attribute, so Gatekeeper does not block it. Do not install a copy downloaded through a browser: it carries `com.apple.quarantine`. Always run the `smolvm` wrapper, never `smolvm-bin` directly, because the wrapper sets the library path.

**Engine CLI contract.** Every flag the provider adapter passes was checked against the macOS 1.32.4 binary's `--help`: `machine create` (`--name --image --net --net-backend virtio-net --block-private --guest-subnet --cpus --mem --storage --disk-durability --user --stop-timeout --stop-on-exit --label --volume --port`), `start`, `stop --timeout`, `delete --force`, `exec --name -- …`, `status --json`, `ls --quiet`, `logs --tail`, `cp`, `data-dir`. `machine start --no-workload` is not listed in `--help` but is accepted; an unknown flag is rejected. `--guest-subnet 100.96.0.0/30` puts the gateway, and therefore `host`, at `100.96.0.1`. The engine caps `--stop-timeout` at 300 s, and the app spec maximum (240 + 5) stays below that.

**Share directories.** On macOS, `/tmp`, `/var` and `/etc` are symlinks into `/private`, and `mounts` must not contain a symlink component. Put shares under your home directory, e.g. `~/defuss-shares/hello`, or pass the `/private/...` path.

**Paths and service.** State lives in `~/Library/Application Support/defuss-container`. `install.py --service` writes `~/Library/LaunchAgents/org.defuss.container.plist` (KeepAlive, umask 077). The daemon logs to `daemon.stdout.log` and `daemon.stderr.log` in the state directory. Check it with `launchctl print gui/$(id -u)/org.defuss.container`. The launchd `PATH` (`/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin`) covers the `git` and `sysctl` the daemon runs.

**Proxies.** Before booting, smolvm passes `HTTP(S)_PROXY` from the daemon environment to the guest image pull. It refuses a proxy on host loopback if it cannot find a host address the guest can reach. Start the daemon without such proxy variables, or give smolvm a LAN-reachable proxy.

## Required before a production binary release

- Run `scripts/vm-smoke.py` on Linux KVM (x86_64 and arm64), macOS Apple Silicon HVF and Windows x86_64 WHP. Use a dedicated fixture repository and test ports. The script creates and deletes its own app; keep its logs when a step fails.
- Run `scripts/benchmark.py` on each target host and filesystem. Publish only the measured samples.
- Test native GUI linking and operation, installer login services, restart with active jobs, OS shutdown/reboot, low disk space, network loss and forced termination during restore. Check guest flush behavior and recovery on real storage.
- Sign and notarize, then run platform distribution checks.

## Upstream references

- [smolvm 1.32.4 source and release](https://github.com/smol-machines/smolvm/releases/tag/v1.32.4)
- [smolmachines Rust SDK](https://docs.rs/smolmachines/)
- [Bun distroless image tag](https://hub.docker.com/r/oven/bun/tags?name=1.4.4-distroless)
- [defuss-tauri](https://tauri.defuss.org)
- [Tauri platform prerequisites](https://v2.tauri.app/start/prerequisites/)

Engine archives, crates and npm dependencies are third-party software under their own licenses.
