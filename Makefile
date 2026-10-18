CARGO ?= $(shell command -v cargo 2>/dev/null || echo $(HOME)/.cargo/bin/cargo)
PNPM ?= pnpm

.PHONY: setup lint format typecheck test test-rust test-ts check

setup:
	$(PNPM) install --frozen-lockfile
	$(CARGO) fetch

lint:
	$(CARGO) xtask layers
	$(PNPM) exec biome check --error-on-warnings .
	$(CARGO) fmt --all --check
	$(CARGO) clippy --workspace --all-targets -- -D warnings

format:
	$(PNPM) exec biome check --write .
	$(CARGO) fmt --all

typecheck:
	$(PNPM) -r --if-present run typecheck

test: test-rust test-ts

test-rust:
	$(CARGO) test --workspace

test-ts:
	$(PNPM) -r --if-present run test

check: lint typecheck test
