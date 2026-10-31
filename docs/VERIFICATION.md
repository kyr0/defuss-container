# Verification

`make verify` is the gate: lint, tests, coverage and the real-VM e2e, the same verbs CI runs. Only the e2e boots VMs. It proves builds, readiness, the `host` alias, shares, persistence, backup and restore, redeploy, rollback, failure handling and the desktop UI against a live daemon. Run it on every OS and architecture before distributing binaries.

## Gates

| Gate | Command | Proves | Needs |
|---|---|---|---|
| Lint | `make lint` | rustfmt clean; Clippy and oxlint with warnings denied; desktop TypeScript check; native extension formatting | Rust 1.99.1, Bun 1.4 |
| Tests | `make test` | Rust contract tests (API auth/Origin/Host, spec validation, SQLite job recovery, OCI layout, layer validation, process-group timeout) and lifecycle tests against a scripted engine (rollback, crashing release, corrupt release, base change, restore failure). Guest archive and Bun builder tests. Real daemon + CLI smoke (auth, Origin rejection, failed job, single-instance lock, private files, restart, token rotation). UI interaction tests against a scripted bridge | Bun 1.4.x, Chromium, loopback listener |
| Coverage | `make coverage` | Rust line coverage of the tests plus the real daemon/CLI smoke run, floor 60% | `cargo-llvm-cov`, `llvm-tools-preview` |
| E2E | `make e2e` | Installed artifact (`scripts/install.py`) against a real VM, then the built UI in Playwright Chromium and WebKit against the same daemon (details below) | Hypervisor, Docker Hub, npm registry, Git, openssl |
| Registry | `cargo test --test registry -- --ignored --nocapture` | Pinned `oven/bun:1.4.4-distroless` resolves for this architecture; manifest and blob digests verify; image contains `/usr/local/bin/bun` | Docker Hub access |
| Native bridge | `make native` | Tauri extension compiles for this OS | Tauri OS prerequisites |
| Benchmark | `make bench APP=NAME` | Guest-local `var` vs. shared `output` I/O, SQLite FULL commits; cached reads labelled as such | App deployed on the `make start` daemon |

`scripts/control-smoke.py` and `scripts/install.py` locate binaries through `cargo metadata`, so `CARGO_TARGET_DIR` and `build.target-dir` are honored.

### What the e2e covers

`tests/e2e/run.py` consumes the installed artifact: `bin/defuss-container`, `bin/defuss-containerd` and the checksummed engine. It serves a fixture repository (the example app under `apps/hello`) over local HTTPS with a throwaway certificate. Then it walks through these steps:

1. Start the daemon and check `doctor`.
2. Register the app; duplicate and invalid specs are rejected.
3. Build in a builder VM, including `wait --timeout 0`, `jobs`, `job` and `job-log`.
4. Deploy with health gating.
5. Check the guest wiring: input and etc shares with input read-only, output share, `tmp` emptied on start, environment, and the `host` alias reaching a host loopback server.
6. Check SQLite persistence across stop and start, then backup and restore with its safety backup.
7. Build and deploy v2, then roll back with the data kept.
8. Deploy a release whose start script crashes: it is refused fast and the previous release serves again.
9. Restart the daemon: the token rotates, the VM keeps serving and the desired state is reconciled.
10. Exercise the `api` command.
11. Delete the app; the VM is removed and backups stay.

The run fails if any `defuss-container --help` subcommand never ran. `desktop/tests/e2e.test.mjs` then drives the built `desktop/dist` in a real browser. A Playwright binding replaces Tauri IPC and calls `defuss-container api`, the same Rust client the native bridge uses, so every click reaches the live daemon. It covers the dashboard, runtime diagnostics, live status, logs, stop/start, backup and restore dialogs, the release deploy dialog, activity details, and create and delete through the form, with no console errors, page errors or failed requests. Results go to `output/e2e/`: `report.json` (steps and timings), `summary.md` and `resources.json` (peak RSS, disk), daemon and job logs, GUI logs and screenshots.

## Platform status

| Platform | Date | Passed | Not yet run |
|---|---|---|---|
| macOS 26.7.1, Apple Silicon (M5 Max) | 2026-10-31 | `make lint`, `make test` (26 Rust, 12 archive, 3 builder, smoke, 11 UI), coverage 69%, registry (arm64), engine bundle and CLI contract, **full e2e on HVF incl. GUI in Chromium and WebKit**, native `cargo check` | launchd service, signed distribution |
| Linux x86_64 | 2026-10-31 | Format, Clippy, Rust tests, archive and builder tests, smoke, registry (amd64), engine bundle, UI build and interaction tests, native `cargo check` (original source check, before the e2e existed) | E2E (no `/dev/kvm` then); CI `linux` job runs it on KVM |
| Windows x86_64 | not yet | nothing | Everything; CI `platforms` and `guest` jobs |

CI (`.github/workflows/verify.yml`):

- `linux` runs `make setup` and `make verify` on `ubuntu-latest`, guest e2e included, after giving the runner user `/dev/kvm`.
- `platforms` runs every gate that needs no VM on hosted macOS and Windows.
- `guest` runs the e2e on macOS and Windows. GitHub documents that hosted arm64 macOS runners do not support nested virtualization. Whether hosted Windows runners expose Windows Hypervisor Platform is UNKNOWN. Set the repository variables `MACOS_HV_RUNNER` / `WINDOWS_HV_RUNNER` to a runner with a usable hypervisor (JSON `runs-on`, e.g. `["self-hosted","macOS","ARM64"]`); the job then blocks merges. Without them it runs on the hosted runner, records the hypervisor gap in its `output/e2e` artifact and does not fail the workflow.

## Resource footprint

VERIFIED on macOS 26.7.1, Apple Silicon (M5 Max), HVF, by `make e2e` on 2026-10-31 with the example app (1024 MiB, 2 vCPUs, 20 GiB storage). Peak RSS comes from the kernel for our processes (`wait4`), and from sampling `ps` every second for the engine's VM processes. The desktop app was sampled with `ps` every 0.5 s, by hand, from the `bun run desktop` bundle; `make e2e` drives the UI in Playwright instead. `make metrics` prints the numbers from the latest run.

| Process | Peak RSS |
|---|---|
| `defuss-containerd` (whole run) | 25 MiB |
| `defuss-container` (largest of 51 calls) | 15 MiB |
| Desktop app `Defuss Container.app`, dashboard open for 20 s (app process / with its 3 WebKit helper processes) | 95 MiB / 189 MiB |
| App VM host process (guest used 86 MiB of 986 MiB) | 334 MiB |
| Builder VM host process, during a build | 286 MiB |
| smolvm `image-seed-*` helper VM, while importing a new image (8 GiB configured, guest used 179 MiB) | 656 to 707 MiB |

Estimate from this run, not a measured limit: the app VM's host process held 334 MiB while its guest used 86 MiB, so budget each app's configured memory plus about 0.25 GiB. Importing a release image briefly needs about 0.7 GiB more for the helper VM.

| Disk | Allocated |
|---|---|
| Installed binaries (`bin/`) | 11 MiB |
| Desktop app bundle (`Defuss Container.app`) | 6.1 MiB |
| Engine bundle (`engine/`; sparse disk templates, 30 GiB apparent) | 133 MiB |
| Verified base-image blobs (daemon `cache/`) | 58 MiB |
| Each release: `app.tar` 4.5 MiB + `image.oci.tar` 62 MiB | 66 MiB |
| App VM disks after the run (sparse; can grow to `storage_gib` + 10 GiB overlay) | 169 MiB |
| Engine image seed per newly created machine image (smolvm cache) | about 150 MiB |
| Backups of the example's `var` | under 0.1 MiB each |

One app with three releases came to about 0.7 GiB (bin, engine, base blobs, releases, VM disk, the seeds of this run). A machine that never built before also needs the Bun base image seed, 136 MiB here, so plan about 0.85 GiB. UNKNOWN: how smolvm evicts image seeds. `machine prune` works per machine, and seeds from deleted apps stay in the engine cache (macOS: `~/Library/Caches/smolvm/image-seeds`). No retention policy for releases, backups or seeds exists yet.

## macOS notes

**Toolchain.** Install the Xcode Command Line Tools and Rust through rustup; `rust-toolchain.toml` selects 1.99.1. If linking fails with `unknown architecture arm64e.x1-macos` in a `.tbd` file, the installed `ld` is older than the newest SDK (observed: CLT 26.6 `ld-1267` with `MacOSX27.0.sdk`). Update the Command Line Tools, or point the build at an older installed SDK:

```sh
export SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk
```

**Engine.** The pinned `smolvm-1.32.4-darwin-arm64.tar.gz` contains a bash wrapper `smolvm` and `smolvm-bin`. `smolvm-bin` is ad-hoc signed (no Developer ID) with `com.apple.security.hypervisor`, `com.apple.security.cs.allow-jit` and `com.apple.security.cs.disable-library-validation`. `scripts/install.py` downloads it without a quarantine attribute, so Gatekeeper does not block it. Do not install a copy downloaded through a browser: it carries `com.apple.quarantine`. Always run the `smolvm` wrapper, never `smolvm-bin` directly, because the wrapper sets the library path.

**Engine CLI contract.** Every flag the provider adapter passes was checked against the macOS 1.32.4 binary's `--help`: `machine create` (`--name --image --net --net-backend virtio-net --block-private --guest-subnet --cpus --mem --storage --disk-durability --user --stop-timeout --stop-on-exit --label --volume --port`), `start`, `stop --timeout`, `delete --force`, `exec --name -- …`, `status --json`, `ls --quiet`, `logs --tail`, `cp`, `data-dir`. `machine start --no-workload` is not listed in `--help` but is accepted; an unknown flag is rejected. `--guest-subnet 100.96.0.0/30` puts the gateway, and therefore `host`, at `100.96.0.1`. The engine caps `--stop-timeout` at 300 s, and the app spec maximum (240 + 5) stays below that.

**Desktop app.** VERIFIED on 2026-10-31: `make gui` builds the app and `.dmg`, and `make start-gui` connects it to the `make start` daemon on port 63705. tauri-cli refuses to build when `@tauri-apps/api` and the `tauri` crate differ in minor version, so both are pinned to 2.12.1.

**Share directories.** On macOS, `/tmp`, `/var` and `/etc` are symlinks into `/private`, and `mounts` must not contain a symlink component. Put shares under your home directory, e.g. `~/defuss-shares/hello`, or pass the `/private/...` path.

**Paths and service.** State lives in `~/Library/Application Support/defuss-container`. `install.py --service` writes `~/Library/LaunchAgents/org.defuss.container.plist` (KeepAlive, umask 077). The daemon logs to `daemon.stdout.log` and `daemon.stderr.log` in the state directory. Check it with `launchctl print gui/$(id -u)/org.defuss.container`. The launchd `PATH` (`/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin`) covers the `git` and `sysctl` the daemon runs.

**Proxies.** Before booting, smolvm passes `HTTP(S)_PROXY` from the daemon environment to the guest image pull. It refuses a proxy on host loopback if it cannot find a host address the guest can reach. Start the daemon without such proxy variables, or give smolvm a LAN-reachable proxy.

## Required before a production binary release

- Run `make e2e` on Linux KVM (x86_64 and arm64), macOS Apple Silicon HVF and Windows x86_64 WHP, and keep `output/e2e/` for each.
- Run `make bench` on each target host and filesystem; publish only the measured samples.
- Test native GUI linking and operation, installer login services, OS shutdown/reboot, low disk space, network loss and forced termination during restore. Check guest flush behavior and recovery on real storage.
- Sign and notarize, then run platform distribution checks.

## Upstream references

- [smolvm 1.32.4 source and release](https://github.com/smol-machines/smolvm/releases/tag/v1.32.4)
- [smolmachines Rust SDK](https://docs.rs/smolmachines/)
- [Bun distroless image tag](https://hub.docker.com/r/oven/bun/tags?name=1.4.4-distroless)
- [defuss-tauri](https://tauri.defuss.org)
- [Tauri platform prerequisites](https://v2.tauri.app/start/prerequisites/)

Engine archives, crates and npm dependencies are third-party software under their own licenses.
