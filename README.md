# tagent (`ta`)

A small BYOK coding agent for Termux. M0 is a single Rust binary with a
headless agent loop, Anthropic streaming, and three built-in tools (`read`,
`write`, `bash`). The Web UI, TUI, plugins, MCP, and credential commands are
later milestones and are reported as such by the CLI.

## Build

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release
```

The release binary is `target/release/ta`. It uses rustls (no system OpenSSL)
and has no Node.js runtime dependency.

### Android builds

Both Android ABIs are built and verified with the Android NDK (API 24):

```sh
./scripts/build-android.sh
```

Outputs:

- `artifacts/android/ta-arm64` — ELF64 AArch64, `/system/bin/linker64`
- `artifacts/android/ta-arm32` — ELF32 ARM, `/system/bin/linker`

Both link only against Android `libc.so` and `libdl.so`; no OpenSSL or Node.js
is needed. `ring` (the rustls crypto backend) is compiled by the NDK clang.

## Run

```sh
export ANTHROPIC_API_KEY='...'        # value is never logged, persisted, or echoed
ta -p "read Cargo.toml" -m anthropic/<your-model> --permission-mode yolo
```

`-m provider/model` (or `[roles].main` in the config) is required in M0; no
model ID is hardcoded. Useful output modes:

```sh
ta -p "..." --output-format text
ta -p "..." --output-format json
ta -p "..." --output-format stream-json
```

Permission rules accept Claude-style `Tool(pattern)` values, for example
`--allowedTools "Bash(git:*)"` and `--disallowedTools "Bash(git push:*)"`.
Dangerous shell commands are denied in every mode. In headless mode a rule
that would ask is returned as a denied tool call instead of prompting.

## Config

Config lives at `$XDG_CONFIG_HOME/tagent/config.toml` or
`$HOME/.config/tagent/config.toml`. Sessions are appended as JSONL under
`$XDG_DATA_HOME/tagent/sessions` or `$HOME/.local/share/tagent/sessions`.

```toml
[providers.anthropic]
type = "anthropic"
key_env = "ANTHROPIC_API_KEY"
# base_url = "https://api.anthropic.com"

[roles]
main = "anthropic/<your-model>"
```

Only the environment variable **name** is stored. The resolved key stays in
the provider adapter and is stripped from tool output, events, and sessions.

## Verification

```sh
cargo test
```

Unit tests cover CLI parsing, config resolution, permission precedence and
globs, secret redaction, session replay, SSE parsing, and path confinement.
`tests/agent_loop.rs` runs the full loop against a local mock Anthropic SSE
server: tool call → tool result → final text, with request and session checks.

Measured on this machine: `ta --help` starts in ~3 ms; release binary is ~3.2 MB
(2.9 MB arm64, 1.8 MB arm32).
