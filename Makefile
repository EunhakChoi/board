CARGO ?= $(shell command -v cargo 2>/dev/null || echo $(HOME)/.cargo/bin/cargo)
PNPM ?= pnpm

.PHONY: setup gen-api lint format typecheck test test-rust test-ts check e2e docker-smoke

setup:
	$(PNPM) install --frozen-lockfile
	$(CARGO) fetch

gen-api:
	$(PNPM) --filter @charet/api-client run gen

lint:
	$(CARGO) xtask layers
	$(PNPM) exec biome check --error-on-warnings .
	$(PNPM) -r --if-present run lint
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
	$(CARGO) build --quiet -p charet-server
	$(PNPM) -r --if-present run test

check: lint typecheck test

# The web tool against the real stack (needs a Playwright Chromium: `pnpm --filter charet-web exec playwright install chromium chromium-headless-shell`).
e2e:
	$(CARGO) build --quiet -p charet-server -p charet-render
	$(PNPM) --filter charet-web run build
	$(PNPM) --filter charet-web run e2e

# `docker compose` install on spare ports (5380 / 5381), checked end to end, then removed.
docker-smoke:
	scripts/docker-smoke.sh
