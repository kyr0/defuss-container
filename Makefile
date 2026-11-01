# Project interface (defuss-vae layout): the same verbs for humans, agents and CI; `make` lists them with usage.
# Service: the daemon on a development state directory; stdout/stderr → var/log/$(NAME).*, pid → tmp/$(NAME).pid.
NAME        ?= defuss-containerd
N           ?= 50
# scripts/install.py target used by e2e and the dev daemon: bin/ (release build) and engine/ (pinned, checksummed).
E2E_INSTALL ?= tmp/e2e/install
DEV_HOME    ?= var/home
RUN         ?= $(CURDIR)/$(E2E_INSTALL)/bin/defuss-containerd --home $(CURDIR)/$(DEV_HOME) --engine $(CURDIR)/$(E2E_INSTALL)/engine/smolvm
export RUN
-include .env
ENV_KEYS := $(shell sed -n 's/^\([A-Za-z_][A-Za-z0-9_]*\)=.*/\1/p' .env 2>/dev/null)
ifneq ($(ENV_KEYS),)
export $(ENV_KEYS)
endif
export PATH := $(PATH):$(HOME)/.local/bin:$(HOME)/.bun/bin:$(HOME)/.cargo/bin
# GNU Make 3.81 (macOS) execs simple commands with its startup PATH, ignoring the export above; a shell gets it.
SHELL := env PATH='$(PATH)' /bin/sh
LOG   = var/log/$(NAME)
PID   = tmp/$(NAME).pid
ALIVE = [ -f $(PID) ] && kill -0 "$$(cat $(PID))" 2>/dev/null
# Clients of the `make start` daemon: both read connection.json from $(DEV_HOME), as any CLI or app run would.
CLI_BIN = $(CURDIR)/$(E2E_INSTALL)/bin/defuss-container
GUI_BIN = $(CURDIR)/desktop/dist-tauri-build/release/defuss-tauri-host
ARGS   ?= list
SITE_PORT ?= 63706
# Formatters: Prettier (desktop/package.json pin) for JS/TS/CSS/HTML/JSON, Ruff (pinned here, run through uvx) for
# Python, rustfmt for Rust. `make format` writes, `make lint` checks the same files.
PRETTIER = desktop/node_modules/.bin/prettier
PRETTIER_FILES = 'desktop/{src,tests}/**/*.{ts,tsx,mjs,css}' 'desktop/*.{html,json}' 'guest/*.mjs' 'tests/*.mjs' 'examples/*/*.{ts,json}'
RUFF = uvx ruff@0.17.3
PYTHON_DIRS = scripts tests

.PHONY: help setup install start detach site stop restart status log metrics bench check format lint test coverage e2e cli start-cli gui start-gui desktop native verify build
.DEFAULT_GOAL := help

help: ## list the verbs with their usage (default goal)
	@awk -F':.*## ' '/^[a-zA-Z0-9_ -]+:.*## /{printf "  %-12s %s\n", $$1, $$2}' $(firstword $(MAKEFILE_LIST))

setup: ## rustup if missing, the toolchain pinned in rust-toolchain.toml, locked Rust/bun deps, browsers, cargo-llvm-cov, uv
	command -v cargo >/dev/null 2>&1 || curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal --default-toolchain none
	cargo fetch --locked
	cd desktop && bun install --frozen-lockfile --ignore-scripts && bunx playwright install chromium webkit
	command -v cargo-llvm-cov >/dev/null 2>&1 || cargo install cargo-llvm-cov --locked
	command -v uvx >/dev/null 2>&1 || curl -LsSf https://astral.sh/uv/install.sh | sh

install: ## release build + checksummed engine into $(E2E_INSTALL) (the artifact e2e consumes)
	python3 scripts/install.py --home $(E2E_INSTALL)

start: install ## run the installed daemon detached on $(DEV_HOME): pid in tmp/, logs in var/log/
	@$(MAKE) --no-print-directory detach
	@echo "CLI: make start-cli, or $(E2E_INSTALL)/bin/defuss-container --home $(DEV_HOME)"

# Runs $$RUN detached as service $(NAME): its own session (or process group), pid in tmp/, stdout/stderr in var/log/.
detach:
	@mkdir -p var/log tmp
	@if $(ALIVE); then echo "running pid=$$(cat $(PID))"; exit 0; fi; \
	if command -v setsid >/dev/null 2>&1; then nohup setsid sh -c "$$RUN" >$(LOG).stdout 2>$(LOG).stderr </dev/null & \
	else set -m; nohup sh -c "$$RUN" >$(LOG).stdout 2>$(LOG).stderr </dev/null & set +m; fi; \
	echo $$! >$(PID); sleep 1; \
	if $(ALIVE); then echo "running pid=$$(cat $(PID))"; else echo "dead; see: make log NAME=$(NAME)"; exit 1; fi

# The static project site (docs/) as its own service; status, log and stop take NAME=site.
site: ## serve the project site docs/ on http://127.0.0.1:$(SITE_PORT) in the background; stop: make stop NAME=site
	@$(MAKE) --no-print-directory detach NAME=site RUN='python3 -u -m http.server $(SITE_PORT) --bind 127.0.0.1 --directory docs'
	@echo "site: http://127.0.0.1:$(SITE_PORT)/"

stop: ## SIGTERM the daemon group (running jobs finish first), SIGKILL after 60 s
	@if $(ALIVE); then p=$$(cat $(PID)); kill -s TERM -- -$$p 2>/dev/null || kill -s TERM $$p; \
	i=0; while kill -0 $$p 2>/dev/null && [ $$i -lt 600 ]; do sleep 0.1; i=$$((i+1)); done; \
	kill -s KILL -- -$$p 2>/dev/null || true; fi; rm -f $(PID); echo stopped

restart: stop start ## stop, then start

status: ## running | dead (stale pid) | stopped, as LSB exit codes
	@if $(ALIVE); then echo "running pid=$$(cat $(PID))"; \
	elif [ -f $(PID) ]; then echo "dead pid=$$(cat $(PID)); see: make log"; exit 1; \
	else echo stopped; exit 3; fi

log: ## tail daemon stdout and stderr logs (N=50 lines)
	@for f in $(LOG).stdout $(LOG).stderr; do [ -f $$f ] && { echo "== $$f"; tail -n $(N) $$f; }; done; \
	ls $(LOG).* >/dev/null 2>&1 || echo "∅ no logs for $(NAME)"

metrics: ## peak RSS and disk use measured by the last e2e run
	@cat output/e2e/summary.md 2>/dev/null || { echo "UNKNOWN[metrics] BC no e2e run yet: make e2e"; exit 2; }

bench: ## guest filesystem benchmark of a deployed app: make bench APP=<name> (daemon from make start)
	@[ -n "$(APP)" ] || { echo "UNKNOWN[bench] BC APP unset: make bench APP=<deployed app>"; exit 2; }
	python3 scripts/benchmark.py $(APP) --cli $(E2E_INSTALL)/bin/defuss-container --home $(DEV_HOME) --engine $(E2E_INSTALL)/engine/smolvm --output output/bench-$(APP).json

check: lint ## alias of lint (earlier name)

format: ## rewrite Rust, JS/TS/CSS/HTML/JSON and Python files in the project style
	cargo fmt --all
	cargo fmt --manifest-path desktop/native/Cargo.toml
	$(PRETTIER) --write --log-level warn $(PRETTIER_FILES)
	$(RUFF) format $(PYTHON_DIRS)

lint: ## format check (rustfmt, Prettier, Ruff), Clippy, oxlint and Ruff with warnings denied, desktop type check
	cargo fmt --all -- --check
	cargo clippy --all-targets --locked -- -D warnings
	cargo fmt --manifest-path desktop/native/Cargo.toml -- --check
	desktop/node_modules/.bin/oxlint --deny-warnings desktop/src desktop/tests guest tests examples/hello
	$(PRETTIER) --check --log-level warn $(PRETTIER_FILES)
	$(RUFF) format --check $(PYTHON_DIRS)
	$(RUFF) check $(PYTHON_DIRS)
	cd desktop && bunx tsc --noEmit

test: ## Rust + guest helper + builder tests, daemon/CLI smoke, UI interaction tests (no VM)
	cargo test --locked
	node --test tests/archive.test.mjs
	node --test tests/build.test.mjs
	cargo build --locked
	python3 scripts/control-smoke.py
	cd desktop && bun run test:ui

# Instrumented build in its own target dir; control-smoke.py finds it through `cargo metadata`, so the CLI and daemon
# count as they run as real processes, not only through in-process tests.
coverage: ## Rust line coverage of tests + the real daemon/CLI smoke run; prints TOTAL <n>%
	@set -e; mkdir -p output; export CARGO_TARGET_DIR="$${CARGO_TARGET_DIR:-$(CURDIR)/target}/coverage"; \
	eval "$$(cargo llvm-cov show-env --sh)"; cargo llvm-cov clean --workspace; : >output/coverage-run.log; \
	run() { "$$@" >>output/coverage-run.log 2>&1 || { tail -n 40 output/coverage-run.log; exit 1; }; }; \
	run cargo test --locked; run cargo build --locked; run python3 scripts/control-smoke.py; \
	cargo llvm-cov report --summary-only | tee output/coverage.txt | awk '/^TOTAL/{print "TOTAL " $$10}'

e2e: install ## installed artifact + built UI against a real VM: CLI, guest lifecycle, GUI (Playwright) → output/e2e/
	cd desktop && bun run build
	python3 tests/e2e/run.py --install $(E2E_INSTALL) --dist desktop/dist --output output/e2e

cli: install ## build the CLI (and daemon) into $(E2E_INSTALL)

start-cli: ## run the built CLI against the make start daemon (started if down): make start-cli ARGS="show hello"
	@$(ALIVE) || $(MAKE) --no-print-directory start
	@"$(CLI_BIN)" --home "$(CURDIR)/$(DEV_HOME)" $(ARGS)

gui: ## build the native desktop app for this OS (binary + bundle under desktop/dist-tauri-build/release)
	cd desktop && bun install --frozen-lockfile --ignore-scripts && bun run desktop

start-gui: ## open the built desktop app against the make start daemon (started if down); exits with the window
	@[ -x "$(GUI_BIN)" ] || { echo "UNKNOWN[start-gui] BC no desktop build: make gui"; exit 2; }
	@$(ALIVE) || $(MAKE) --no-print-directory start
	DEFUSS_CONTAINER_HOME="$(CURDIR)/$(DEV_HOME)" "$(GUI_BIN)"

desktop: ## build the UI and generate the defuss-tauri host without compiling it
	cd desktop && bun install --frozen-lockfile --ignore-scripts && bun run prepare:host

native: ## type-check the native desktop extension
	cargo check --manifest-path desktop/native/Cargo.toml --locked

build: ## optimized binaries
	cargo build --release --locked

# What CI runs; the gate runs the same verbs one by one. Fail-fast order: cheapest first.
verify: lint test coverage e2e ## what CI runs: every gate verb, cheapest first
