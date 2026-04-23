# TuxTalks Oxide - Development Automation

.PHONY: check test lint audit bench ci

# Default: Run everything that CI runs
ci: check lint test audit

check:
	cargo check --all-targets --all-features

test:
	cargo nextest run --all-features

lint:
	cargo fmt --all -- --check
	cargo clippy --all-targets --all-features -- -D warnings -D clippy::pedantic

audit:
	cargo audit

bench:
	cargo bench

docs:
	cargo doc --no-deps --open

clean:
	cargo clean
