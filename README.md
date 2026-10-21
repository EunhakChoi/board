# charet

*The live-code design charrette for React and AI agents.*

**English** | [한국어](README.ko.md)

A code-first design board for teams. Your real UI code (React + TypeScript), rendered with mock APIs,
laid out as components, pages and screens. Teammates comment, propose changes with an AI agent or by
hand, and accept them as versions — so the design never drifts from the code.

> Status: pre-alpha, private. Not usable yet.

## Run it

```bash
docker compose up -d     # then open the admin link from `docker compose logs server`
```

Settings, the CLI, running without Docker and security notes: [docs/local.md](docs/local.md).
How a change goes from a draft to a version: [docs/workflow.md](docs/workflow.md).
Asking a coding agent for changes from the web tool: [docs/agent.md](docs/agent.md).
Changing text, variants, spacing and order by hand: [docs/hand-edits.md](docs/hand-edits.md).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
