# Agent guide

charet is a code-first design board: real React UI code rendered with mock APIs, reviewed and changed
by a team (comments, proposals, versions). Pre-alpha.

## Layout

- `apps/charet-server` (Rust) · `apps/charet-render` (Rust + Chromium) · `apps/charet-workspace` · `apps/charet-agent` · `apps/charet-cli` · `apps/charet-web` (React + shadcn)
- `crates/*` layered Rust crates (`charet-model` L0 → `charet-engine` L1 → `charet-store` · `charet-db` · `charet-collab` L2 → `charet-api` L3); `cargo xtask layers` enforces it
- `apps/charet-workspace` (Node): installs, verifies and builds project commits; the only place project code runs on the server side, so its child processes get a minimal environment
- `apps/charet-render` (Rust + Chromium): renders preview pages into images, element maps (component chain + `data-src`) and problems
- `templates/recommended`: the recommended project setup (Vite + React + TS + Tailwind v4 + shadcn + MSW)
- `packages/sdk` project contract, vite plugin and frame runtime · `packages/api-client` generated from `contracts/openapi.yaml` · `packages/i18n` en/ko strings

## Rules

- `make check` (lint + typecheck + test) must pass before you finish.
- Clients call the server only through `@charet/api-client`. Change `contracts/openapi.yaml` first, then run `make gen-api` and commit the generated `schema.d.ts`.
- Every user-facing string goes through `@charet/i18n`. English is the default; keep `ko.json` keys a subset of `en.json`.
- `charet-render`, `charet-workspace` and `charet-agent` take orders from `charet-server` only.
- Every action is a command in `charet-engine`; the web tool, CLI, MCP and agents call the same commands.
- Work on a branch, open a PR to `main`.
