# CLI gist

<!-- Commands future agents need that the Makefile verbs do not cover (setup, env, data, deploy):
- VERIFIED[purpose] `cmd`: only after an observed success
- UNKNOWN[purpose] BC gap
Budget 2 KiB (`vae.py doctor --repo .`); entries are injected at session start. -->
- VERIFIED[artifact] `python3 scripts/install.py --home tmp/e2e/install`: release build + checksummed engine, what `make e2e` consumes
- VERIFIED[e2e without GUI] `python3 tests/e2e/run.py --install tmp/e2e/install --browsers ""`
- VERIFIED[registry gate] `cargo test --test registry -- --ignored --nocapture` (blobs redirect to production.cloudfront.docker.com)
- VERIFIED[leftover VMs] `tmp/e2e/install/engine/smolvm machine ls`, then `machine delete --name NAME --force`
- VERIFIED[dev clients] `make start-cli ARGS="doctor"`, `make gui` then `make start-gui`: CLI/app on var/home against the `make start` daemon (127.0.0.1:63705)
