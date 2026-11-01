# Episodes

<!-- Newest last. The gate appends FAIL|DONE|FINDING; past 100 entries it trims only its own oldest DONE|FAIL|learned
FINDING (git keeps them). Leads (LESSON, FINDING learn=none) stay until wrap settles them; doctor fails above 30.
Agents append `<UTC ISO> s=<session> LESSON <VAE-DIALECT>` only for a falsified HYPOTHESIS, a dead end, or a root cause.
Session start injects the 3 newest open entries: LESSON, FINDING learn=none, FAIL without a later DONE.
Wrap: EVERY lead becomes a test | .vae/VERIFY.py rule | one MEMORY line with BC, then delete; OR delete BC evidence. -->

2026-10-31T15:31:04Z s=b6ad2ae1 FAIL docs.pages,prose,toolchain,env.example,package
2026-10-31T15:35:13Z s=b6ad2ae1 FAIL toolchain,package
2026-10-31T15:36:35Z s=b6ad2ae1 FAIL toolchain,package,lint,tests.unit,tests.e2e.1,coverage
2026-10-31T15:39:37Z s=b6ad2ae1 FAIL toolchain,package,tests.unit,tests.e2e.1,coverage
2026-10-31T15:44:36Z s=b6ad2ae1 FAIL toolchain,package
2026-10-31T15:57:56Z s=b6ad2ae1 DONE fp=5c7141df36af cov=69.2% paths=.github/workflows/verify.yml,AGENTS.md,ARCH.md,Cargo.lock(+56)
2026-10-31T15:57:56Z s=b6ad2ae1 FINDING guest/supervisor.mjs, guest/build.mjs (package scripts via `bun run --shell bun`) learn=test: tests/build.test.mjs packageScript test + e2e; .vae/VERIFY.py rule guest.no-bun-run-shell
2026-10-31T15:57:56Z s=b6ad2ae1 FINDING src/service.rs:wait_ready_release learn=test: lifecycle test workload_that_stops_the_machine_fails_fast_and_restores_previous fails at 63 s without the fix
2026-10-31T15:57:56Z s=b6ad2ae1 FINDING src/bin/defuss-containerd.rs:main (EnvFilter) learn=test: scripts/control-smoke.py asserts an ISO-8601 INFO 'daemon listening' line with RUST_LOG unset; fails on the old code
2026-10-31T15:57:56Z s=b6ad2ae1 FINDING desktop/src/main.tsx name input pattern learn=test: desktop/tests/ui.test.mjs pattern test fails on the old pattern
2026-10-31T15:57:56Z s=b6ad2ae1 FINDING src/provider.rs:Engine::start_no_workload (maintenance, builder) learn=test: lifecycle test backup_waits_until_a_provision_only_start_accepts_exec fails without the wait; engine cause UNKNOWN (MEMORY.md)
2026-10-31T15:57:56Z s=b6ad2ae1 FINDING tests/contracts.rs:generated_machine_uses_durable_storage_and_no_shell learn=none: the fixed test itself runs under the default macOS TMPDIR in make test
2026-10-31T15:57:56Z s=b6ad2ae1 FINDING Makefile (GNU Make 3.81 PATH) learn=memory: .vae/MEMORY.md line; the stop-hook gate itself re-checks it
2026-10-31T15:57:56Z s=b6ad2ae1 FINDING src/bin/defuss-containerd.rs, src/process.rs (ANSI in log files) learn=none: no mechanical check added: escape codes from third-party child output depend on their NO_COLOR support
2026-10-31T15:57:56Z s=b6ad2ae1 FINDING scripts/install.py:default_home learn=none: one-line fallback; .env.example documents the keys as optional
2026-10-31T15:57:56Z s=b6ad2ae1 FINDING scripts/control-smoke.py, scripts/install.py (target dir) learn=none: make coverage runs control-smoke with a relocated CARGO_TARGET_DIR on every gate
2026-10-31T15:57:56Z s=b6ad2ae1 FINDING Makefile:coverage learn=none: recipe change; observed TOTAL 69.19% afterwards
2026-10-31T15:57:56Z s=b6ad2ae1 FINDING docs/VERIFICATION.md resource guidance (B04/P06, B03) learn=none: page fix; numbers come from output/e2e/resources.json and the engine seed directory
2026-10-31T15:57:56Z s=b6ad2ae1 FINDING ARCH.md security and privacy (L09/P07) learn=none: page fix
2026-10-31T15:57:56Z s=b6ad2ae1 FINDING ARCH.md, desktop/ARCH.md Mermaid diagrams (T07) learn=none: render with `bunx @mermaid-js/mermaid-cli` when allowed
2026-10-31T16:55:56Z s=b6ad2ae1 FAIL tests.e2e.1
2026-10-31T16:56:13Z s=b6ad2ae1 FAIL tests.unit
2026-10-31T17:04:47Z s=b6ad2ae1 DONE fp=720b907dd6d2 cov=69.3% paths=.github/workflows/verify.yml,AGENTS.md,ARCH.md,Cargo.lock(+57)
2026-10-31T17:04:47Z s=b6ad2ae1 FINDING desktop/native/src/lib.rs defuss_tauri_extend + desktop/defuss-tauri.json capabilities learn=test: desktop/tests/ui.test.mjs checks every invoked command is a plugin command granted to the loopback origin; .vae/MEMORY.md notes that Playwright bridges bypass t
2026-10-31T17:04:47Z s=b6ad2ae1 FINDING desktop/package.json @tauri-apps/api learn=verifier: .vae/checks/tauri-versions.py + rule desktop.tauri-versions; fails on the old 2.9.0 pin (mutation run)
2026-10-31T17:04:47Z s=b6ad2ae1 FINDING desktop/src/main.tsx refresh/setInterval learn=test: ui.test.mjs: backoff timing on Playwright's clock, auto-reconnect without click, Reconnect reset
2026-10-31T17:04:47Z s=b6ad2ae1 FINDING src/bin/defuss-containerd.rs Args.port learn=test: scripts/control-smoke.py: a taken port exits non-zero naming DEFUSS_CONTAINER_PORT
2026-10-31T17:04:47Z s=b6ad2ae1 FINDING scripts/install.py main (binary copy) learn=none: the crash of an in-place overwrite was not reproduced here (macOS); no test pins it
2026-10-31T17:04:47Z s=b6ad2ae1 FINDING agent workflow: background gate + stop hook learn=memory: .vae/MEMORY.md line: wait for the gate in the foreground
2026-11-01T01:31:11Z s=b6ad2ae1 DONE fp=dbb2533d7ef6 cov=69.3% paths=AGENTS.md
2026-11-01T01:31:11Z s=b6ad2ae1 FINDING AGENTS.md:37 L09/P07 learn=none: A documented shell check's coverage of the commands it names is not expressible as a VERIFY.py rule kind and gate overlap is runtime state; the AGENTS.md rule i
2026-11-01T01:55:06Z s=b6ad2ae1 DONE fp=4161045a2e4e cov=69.3% paths=AGENTS.md,README.md,docs/VERIFICATION.md,docs/assets/site.css(+2)
2026-11-01T01:55:06Z s=b6ad2ae1 FINDING README.md:12 P07 learn=none: A wording precision defect in a page; no rule kind can check what a sentence claims, and the edit run records it.
2026-11-01T01:55:06Z s=b6ad2ae1 FINDING README.md:58 B02 learn=none: A wording precision defect in a page; no rule kind can check what a sentence claims, and the edit run records it.
2026-11-01T01:55:06Z s=b6ad2ae1 FINDING README.md:8 T04 learn=none: A wording precision defect in a page; no rule kind can check what a sentence claims, and the edit run records it.
2026-11-01T01:55:06Z s=b6ad2ae1 FINDING README.md:177 B02 learn=none: A wording precision defect in a page; no rule kind can check what a sentence claims, and the edit run records it.
2026-11-01T01:55:06Z s=b6ad2ae1 FINDING docs/index.html:113 B02 learn=none: A wording precision defect in a page; no rule kind can check what a sentence claims, and the edit run records it.
2026-11-01T01:55:06Z s=b6ad2ae1 FINDING docs/index.html:195 B02 learn=none: A wording precision defect in a page; no rule kind can check what a sentence claims, and the edit run records it.
2026-11-01T01:55:06Z s=b6ad2ae1 FINDING docs/index.html:307 P07 learn=none: A wording precision defect in a page; no rule kind can check what a sentence claims, and the edit run records it.
2026-11-01T01:55:06Z s=b6ad2ae1 FINDING docs/assets/site.css:.dc-table th learn=none: Visual layout defect found by rendering; the site has no automated visual test and one would pin CSS values.
2026-11-01T01:55:06Z s=b6ad2ae1 FINDING docs/assets/site.css:.dc-flip learn=none: Visual layout defect found by rendering; the site has no automated visual test and one would pin CSS values.
2026-11-01T01:55:06Z s=b6ad2ae1 FINDING docs/VERIFICATION.md:53 B09 learn=none: A wording precision defect in a page; no rule kind can check what a sentence claims, and the edit run records it.
2026-11-01T01:55:06Z s=b6ad2ae1 FINDING docs/assets/site.js:codeLines learn=none: An unverified code comment; no rule kind can check a comment's truth.
2026-11-01T02:13:32Z s=b6ad2ae1 DONE fp=0fe0fa6e95ea cov=69.3% paths=AGENTS.md,Makefile,README.md,docs/VERIFICATION.md(+3)
2026-11-01T02:13:32Z s=b6ad2ae1 FINDING Makefile:start/detach/site learn=none: Found and fixed by rendering or reading; no rule kind checks layout or a label's precision.
2026-11-01T02:13:32Z s=b6ad2ae1 FINDING docs/index.html:.mk-stat CLI learn=none: Found and fixed by rendering or reading; no rule kind checks layout or a label's precision.
2026-11-01T02:13:32Z s=b6ad2ae1 FINDING docs/index.html:#measured charts learn=none: Found and fixed by rendering or reading; no rule kind checks layout or a label's precision.
2026-11-01T02:34:17Z s=b6ad2ae1 FAIL tests.e2e.1
2026-11-01T02:38:15Z s=b6ad2ae1 FAIL lint
2026-11-01T02:41:06Z s=b6ad2ae1 DONE fp=eb8987b13f36 cov=69.9% paths=AGENTS.md,Makefile,README.md,docs/API.md(+8)
2026-11-01T02:41:06Z s=b6ad2ae1 FINDING src/provider.rs:Engine::start_no_workload learn=test: lifecycle tests backup_boots_again_when_a_provision_only_boot_never_accepts_exec (fails with one boot: mutation run) and backup_fails_after_two_provision_only_b
2026-11-01T02:41:06Z s=b6ad2ae1 FINDING .env.example:DEFUSS_CONTAINER_ENGINE learn=none: A clap env behavior; a rule cannot tell a harmful empty key from a documented one.
2026-11-01T02:41:06Z s=b6ad2ae1 FINDING docs/assets/site.css:tab panels learn=none: Visual and interaction check done in Playwright; no gate rule renders the site.
2026-11-01T02:41:06Z s=b6ad2ae1 FINDING docs/index.html:#measured charts learn=none: Layout defect found by rendering.
2026-11-01T02:41:06Z s=b6ad2ae1 FINDING tests/e2e/measure.py:job_durations learn=none: Exercised by every make e2e run; the summary table is its output.
2026-11-01T03:11:35Z s=b6ad2ae1 DONE fp=8165e882f861 cov=69.9% paths=AGENTS.md,Makefile,README.md,docs/API.md(+8)
2026-11-01T03:11:35Z s=b6ad2ae1 FINDING docs/index.html:hero .dc-accent learn=none: A wording decision that belongs to the human.
2026-11-01T13:25:21Z s=b6ad2ae1 DONE fp=67618a64de94 cov=69.9% paths=.github/workflows/verify.yml,README.md,desktop/tests/ui.test.mjs
2026-11-01T13:25:21Z s=b6ad2ae1 FINDING .github/workflows/verify.yml:14 jobs.linux.runs-on learn=none: The comment beside the pin states the constraint and when to move it; a rule against ubuntu-latest would pin a config value, not a code invariant.
2026-11-01T13:25:21Z s=b6ad2ae1 FINDING desktop/tests/ui.test.mjs:19 page() clock setup learn=verifier: .vae/VERIFY.py desktop.tests.clock-paused (not_regex \.clock\.install\( on desktop/tests/*.mjs) matches the old setup and not the fixed file.
2026-11-01T13:25:21Z s=b6ad2ae1 FINDING README.md:21 B (evidence) status sentence learn=none: A status sentence that tracks CI results cannot be checked mechanically without querying CI.
