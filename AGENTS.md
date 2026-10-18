# Agent guide

board is a code-first design board: real React UI code rendered with mock APIs, reviewed and changed
by a team (comments, proposals, versions). Pre-alpha.

## Layout

- `apps/web` web tool (React + shadcn) · `apps/cli` CLI + skill
- `crates/server` board-server (Rust, axum) · `crates/core` pure rules, no I/O · `crates/renderer` Chromium capture
- `services/workspace` runs project code (install, build, vite, checks) · `services/agent-runner` web agent containers
- `packages/sdk` project contract · `packages/api-client` generated from `contracts/openapi.yaml` · `packages/i18n` en/ko strings

## Rules

- `make check` (lint + typecheck + test) must pass before you finish.
- Clients call the server only through `@board/api-client`; change `contracts/openapi.yaml` first.
- Every user-facing string goes through `@board/i18n`. English is the default; keep `ko.json` keys a subset of `en.json`.
- Provider services (renderer, workspace, agent-runner) take orders from the server only.
- Work on a branch, open a PR to `main`.
