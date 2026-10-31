.PHONY: check test desktop native verify build
check:
	cargo fmt --all -- --check
	cargo clippy --all-targets --locked -- -D warnings

test:
	cargo test --locked
	node --test tests/archive.test.mjs
	node --test tests/build.test.mjs
	cargo build --locked
	python3 scripts/control-smoke.py

desktop:
	cd desktop && npm ci --ignore-scripts && npm run prepare:host

native:
	cargo check --manifest-path desktop/native/Cargo.toml --locked

verify: check test desktop native

build:
	cargo build --release --locked
