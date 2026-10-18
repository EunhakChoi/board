# Agent guide

board is a code-first design board: real React UI code rendered with mock APIs, reviewed and changed
by a team (comments, proposals, versions). Pre-alpha.

## Layout

- `apps/board-server` (Rust) · `apps/board-render` (Rust + Chromium) · `apps/board-workspace` · `apps/board-agent` · `apps/board-cli` · `apps/board-web` (React + shadcn)
- `crates/*` layered Rust crates (`board-model` L0 → `board-engine` L1 → `board-store` · `board-db` · `board-collab` L2 → `board-api` L3); `cargo xtask layers` enforces it
- `apps/board-workspace` (Node): installs, verifies and builds project commits; the only place project code runs on the server side, so its child processes get a minimal environment
- `templates/recommended`: the recommended project setup (Vite + React + TS + Tailwind v4 + shadcn + MSW)
- `packages/sdk` project contract, vite plugin and frame runtime · `packages/api-client` generated from `contracts/openapi.yaml` · `packages/i18n` en/ko strings

## Rules

- `make check` (lint + typecheck + test) must pass before you finish.
- Clients call the server only through `@board/api-client`. Change `contracts/openapi.yaml` first, then run `make gen-api` and commit the generated `schema.d.ts`.
- Every user-facing string goes through `@board/i18n`. English is the default; keep `ko.json` keys a subset of `en.json`.
- `board-render`, `board-workspace` and `board-agent` take orders from `board-server` only.
- Every action is a command in `board-engine`; the web tool, CLI, MCP and agents call the same commands.
- Work on a branch, open a PR to `main`.
