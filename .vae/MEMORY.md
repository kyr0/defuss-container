# Agent memory

<!-- One tagged line per durable fact NOT derivable from code, git or docs:
- VERIFIED[scope] fact BC evidence
- HYPOTHESIS[scope] claim BC observation; falsifier=`cmd`
- UNKNOWN[scope] gap BC missing evidence
[scope] = the narrowest path|module|command|condition the evidence covers, not a topic: the entry decides nothing
outside it, and recurrence inside one subsystem never widens it.
Every line carries its BC and stays within 240 chars. Replace stale lines instead of appending.
Mechanizable lessons belong in tests or .vae/VERIFY.py.
Budget 4 KiB (`vae.py doctor --repo .`); entries are injected at session start. -->
- VERIFIED[guest runtime layer: supervisor/archive/policy.mjs] baked into the machine image at create; deploys never refresh it, existing apps keep the old copy until re-created BC oci::assemble + Engine::create_args
- VERIFIED[macOS build, CLT 26.6 ld-1267] cannot link against MacOSX27.0.sdk (`unknown architecture arm64e.x1`); set SDKROOT to MacOSX26.5.sdk BC link failure 2026-10-31
- VERIFIED[.github guest job] hosted arm64 macOS runners lack nested virtualization BC docs.github.com runner limits; guest e2e there needs vars.MACOS_HV_RUNNER
- VERIFIED[agent Bash + VAE commit hook] the hook matches any git-commit text in a Bash command, fixture repos and heredocs included; fixture commits run from tests/e2e (Python) BC hook refusal 2026-10-31
- VERIFIED[smolvm 1.32.4 `machine start --no-workload`] an exec right after it failed once in ~30 (`/usr/local/bin/bun` not found); Engine::start_no_workload waits for exec BC e2e job log 2026-10-31; cause UNKNOWN
- VERIFIED[GNU Make 3.81, macOS] simple recipe commands ignore `export PATH`; Makefile sets SHELL with the PATH BC `make: cargo: No such file or directory` from the gate
- VERIFIED[smolvm 1.32.4 image import] each new image starts an `image-seed-*` helper VM (8 GiB configured, ~700 MiB peak host RSS on macOS) and leaves a seed in the engine cache BC e2e sampler 2026-10-31
- VERIFIED[desktop Playwright suites] the scripted/CLI bridges bypass Tauri, so its ACL is untested there; IPC changes need a `make gui` + `make start-gui` run BC ACL rejection found only in the real app 2026-10-31
- VERIFIED[agent turn + stop hook] ending a turn while a background `vae.py gate` runs starts a second gate; overlapping runs failed a UI test and an e2e backup that both pass alone BC gate logs 2026-10-31; wait for the gate in the foreground
