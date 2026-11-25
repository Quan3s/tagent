# tagent (`ta`)

`ta` is a small BYOK coding agent for Termux. A single Rust binary serves the
Web UI, the headless/TUI clients, the agent loop, provider adapters, tools, and
extension loaders. The browser PWA is the primary client; no Node.js runtime is
needed to run the binary.

## Quick start

```sh
# One-shot, headless run
export ANTHROPIC_API_KEY='...' # never logged, persisted, or echoed
ta -p "read Cargo.toml" -m anthropic/<your-model> --permission-mode yolo

# Local Web UI (binds 127.0.0.1 by default)
ta serve
# open the URL printed on stderr; it includes a one-time token for /api/login
```

`ta` without flags opens the thin TUI. `ta -c` continues the latest session and
`ta -r <id>` resumes a specific one.

## What is included

- **Agent loop**: streamed messages, thinking deltas, native tool calls,
  permission replies, interrupts, token/cost usage, context compaction,
  checkpoints, and recursive `task` subagents.
- **Providers**: Anthropic plus OpenAI-compatible, OpenRouter, Gemini
  (`https://generativelanguage.googleapis.com/v1beta/openai`), and any local
  OpenAI-compatible endpoint. Local loopback endpoints accept a placeholder key.
- **Tools**: `read`, `write`, `edit`, `multi_edit`, `glob`, `grep`, `bash`, `ls`,
  `web_fetch`, `web_search`, `todo`, `task`, `ask_user`, and optional
  Termux:API tools. Writes are confined to the workspace roots; dangerous shell
  commands are always denied.
- **Extensions**: Claude Code plugins/marketplaces, commands, skills, agents,
  hooks, `.mcp.json` servers, `CLAUDE.md`/`AGENTS.md` instructions, and read-only
  adapters for other agent config formats. Plugin scripts run as subprocesses
  with explicit user confirmation.
- **Web UI**: Svelte + Vite + Tailwind PWA embedded with `rust-embed`; mobile
  drawer, streaming chat, permission cards, tool/diff/todo cards, file tree and
  viewer, command palette, settings, plugin/skill/MCP pages, and lazy-loaded
  CodeMirror/xterm panels. Inline Lucide SVG icons only, with accessible labels.
- **Security**: random startup token, HttpOnly `SameSite=Strict` cookie,
  Origin/CSRF/Host/DNS-rebinding checks, token redaction, path-safe uploads, and
  `0600` encrypted key storage. No key is returned in full by any API.

## CLI

```text
ta                         # TUI
ta -p "prompt"             # headless
ta serve [--host --port --token]
ta -c | ta -r <id>         # continue / resume
ta auth add|ls|rm          # BYOK credentials (env first, then encrypted file)
ta model ls|set            # main/fast/plan/subagent models
ta plugin ls|add|rm|import|enable|disable
ta skill ls|add|info
ta agent ls|run
ta mcp ls|add|rm
ta doctor
ta config get|set|edit
```

Claude-compatible flags are supported, including `-m`, `-p`,
`--output-format text|json|stream-json`, `--permission-mode ask|auto-edit|plan|yolo`,
`--allowedTools`, `--disallowedTools`, `--add-dir`, `--plugin-dir`, `--agent`,
`--mcp-config`, `--system-prompt`, `--append-system-prompt`, `--max-turns`,
`--budget`, `--resume`, `--continue`, `--compact`, `--verbose`, `--debug`, and
`--no-color`.

## Config

Config lives at `$XDG_CONFIG_HOME/tagent/config.toml` or
`$HOME/.config/tagent/config.toml`. Sessions are appended as JSONL under
`$XDG_DATA_HOME/tagent/sessions` or `$HOME/.local/share/tagent/sessions`.

```toml
[providers.anthropic]
type = "anthropic"
key_env = "ANTHROPIC_API_KEY"

[providers.local]
type = "openai-compat"
base_url = "http://127.0.0.1:11434/v1"

[roles]
main = "anthropic/<your-model>"
fast = "anthropic/<your-model>"
plan = "anthropic/<your-model>"
subagent = "anthropic/<your-model>"

[settings.pricing]
"anthropic/<your-model>" = 3.0
```

Only the environment variable **name** is stored in config. `ta auth add <provider>`
writes the value to an encrypted file with mode `0600` under the data directory;
environment variables take precedence. Secrets are stripped from events, tool
output, sessions, and `Debug` output.

## Daemon API

The local daemon exposes `GET/POST /api/sessions`,
`WS /api/sessions/{id}/stream`, `GET /api/sessions/{id}/events`, uploads and
file reads, tree/diff/undo, config and providers, auth, plugins, skills, agents,
MCP, command rendering, and `GET /api/openapi.json`. State-changing requests
with cookies require a matching `Origin`; bearer-token requests are intended for
native clients.

## Build

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release
./scripts/build-android.sh
```

Builds use rustls (no system OpenSSL) and produce:

- `target/release/ta` — host binary
- `artifacts/android/ta-arm64` — ELF64 AArch64, `/system/bin/linker64`
- `artifacts/android/ta-arm32` — ELF32 ARM, `/system/bin/linker`

Android builds target API 24 using NDK r25c. Frontend assets are built with
`cd web && npm install && npm run build`; the resulting `web/dist` is embedded
into the Rust binary, so end users do not need Node.js.

## Verification

```sh
cd web && npm run check && npm run build && cd ..
cargo test --all-targets
```

Unit tests cover CLI/config parsing, permission precedence and globs, secret
redaction, session replay, provider streaming/tool-call conversion, extension
loaders, hooks, pricing, and path confinement. `tests/agent_loop.rs` runs the
full loop against a local mock Anthropic SSE server: tool call → tool result →
final text, with request and session assertions. The daemon was smoke-tested for
health/login cookies, REST routes, WebSocket replay/prompt, uploads, extension
routes, and CSRF/Origin rejection.

Known on-device verification items are marked `TODO(verify)` in source: exact
Termux keystore CLI syntax, streamable HTTP MCP sessions, richer HTML extraction,
and per-day cost persistence. They are non-blocking fallbacks, not hidden
dependencies.
