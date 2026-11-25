# tagent — Coding Agent cho Termux (Web UI + CLI)

Lệnh: `ta`. Daemon cục bộ + Web UI (PWA) là client chính, TUI/CLI là client mỏng.

---

## 1. Nguyên tắc

- **Nhẹ**: một binary, khởi động < 200ms, RAM idle < 80MB.
- **Mobile-first**: dùng tốt trên màn hình < 400px, chạm là chính.
- **Tương thích trước**: đọc thẳng plugin/skill/agent của hệ sinh thái có sẵn.
- **BYOK thuần**: không backend trung gian, key lưu cục bộ.
- **Bảo mật mặc định chặt**: daemon có quyền shell nên bind localhost + token.
- **Icon là SVG inline** (bộ Lucide), tuyệt đối không emoji, không icon font.

## 2. Stack

| Lớp | Lựa chọn |
|---|---|
| Core/daemon | Rust, `tokio`, `axum` (HTTP + WebSocket/SSE) |
| LLM HTTP | `reqwest` + `eventsource-stream` |
| Config/parse | `serde`, TOML/JSON, `serde_yaml` (frontmatter) |
| Search | `ignore` + `grep-searcher` |
| Repo map | `tree-sitter` (tùy chọn) |
| Frontend | Svelte (hoặc React) + Vite + Tailwind, build tĩnh, nhúng bằng `rust-embed` |
| Editor/terminal | CodeMirror 6, xterm.js (lazy-load) |
| TUI | `ratatui` + `crossterm` (client mỏng) |
| Script plugin | subprocess shell/Node/Python, không nhúng runtime |

Phương án phụ: Go + Bubble Tea, hoặc TypeScript (Hono + Ink) nếu ưu tiên tương thích plugin hơn hiệu năng.

## 3. Kiến trúc

```
Browser (PWA) ──HTTP/WS──► ta daemon
TUI / ta -p  ─────────►   ├─ core/      agent loop, context, compaction
                            ├─ providers/ adapter LLM (BYOK)
                            ├─ tools/     registry + executor
                            ├─ ext/       plugin/skill/agent/hook/MCP loader
                            ├─ perm/      permission engine
                            ├─ session/   JSONL append-only, checkpoint, undo
                            └─ web/       API + static assets + uploads
```

Agent loop: input → dựng context (system + memory + skill metadata) → gọi model (stream) → parse tool call → permission → thực thi → trả kết quả → lặp. Hook chèn trước/sau mỗi bước.

Giao thức sự kiện (JSON, mỗi sự kiện có `id` tăng dần để resume theo `last_event_id`):
`message.delta`, `thinking.delta`, `tool.start`, `tool.result`, `permission.request`, `diff`, `todo.update`, `usage`, `error`, `done`.

## 4. CLI

```
ta                      # mở TUI
ta serve [--host --port --token]   # Web UI, mặc định 127.0.0.1:7777
ta "prompt"             # TUI với prompt đầu
ta -p "prompt"          # headless
ta -c | ta -r <id>      # continue / resume
ta auth add|ls|rm
ta model ls|set
ta plugin add|ls|rm|update|enable|disable|import
ta skill ls|add|info
ta agent ls|run
ta mcp add|ls|rm
ta doctor
ta config get|set|edit
```

Flag (trùng Claude Code):

```
-m, --model <provider/model>     -p, --print
--output-format text|json|stream-json
--permission-mode ask|auto-edit|plan|yolo
--allowedTools / --disallowedTools "Bash(git:*)"
--add-dir <path>    --system-prompt / --append-system-prompt
--max-turns <n>     --budget <usd>
--plugin-dir <path> --agent <name>   --mcp-config <file>
--resume / --continue
--no-color --compact --verbose --debug
```

## 5. Prefix

| Prefix | Ý nghĩa |
|---|---|
| `/` | slash command (built-in, plugin, skill) |
| `!` | chạy shell, kết quả vào context |
| `@` | tham chiếu file/thư mục/agent, có autocomplete |
| `#` | ghi nhanh vào memory (AGENTS.md / CLAUDE.md) |
| `&` | chạy nền |
| `?` | bảng phím tắt |

Built-in: `/help /model /clear /compact /cost /context /resume /undo /redo /diff /plan /init /permissions /plugin /skills /agents /mcp /hooks /config /theme /doctor /export /quit`. Command plugin: `/plugin:command`, cho gọi rút gọn khi không trùng.

## 6. BYOK / Provider

```toml
# ~/.config/tagent/config.toml
[providers.anthropic]
type = "anthropic"
key_env = "ANTHROPIC_API_KEY"

[providers.local]
type = "openai-compat"
base_url = "http://192.168.1.10:11434/v1"

[roles]
main = "anthropic/claude-sonnet"
fast = "anthropic/claude-haiku"
```

- Adapter: Anthropic, OpenAI (Chat + Responses), Gemini, OpenRouter, mọi endpoint OpenAI-compatible.
- Lưu key: env → `termux-keystore` → file mã hóa quyền 600. Không ghi key vào log/session, API không trả key đầy đủ.
- Schema message/tool-call chung nội bộ; streaming, tool use, vision, prompt caching; fallback XML/JSON khi model không có native tool calling.
- Vai trò: `main`, `fast`, `plan`, `subagent`. Theo dõi token/chi phí, bảng giá ghi đè được.

## 7. Tool

Built-in: `read`, `write`, `edit`, `multi_edit`, `glob`, `grep`, `bash` (timeout, nền, đọc output từng phần), `ls`, `web_fetch`, `web_search`, `todo`, `task`, `ask_user`.
Termux (qua Termux:API, tùy chọn): `termux_notify`, `termux_clipboard`, `termux_open`, `termux_battery`.

- Chạy song song khi độc lập; kết quả dài được cắt/tóm tắt; mỗi tool có JSON Schema.
- **Permission**: rule `Tool(pattern)` (`Bash(git status)`, `Edit(src/**)`), allow/ask/deny, ưu tiên session > project > user. Chặn mặc định lệnh nguy hiểm và ghi ngoài workspace.
- **Checkpoint**: snapshot trước mỗi lần sửa để `/undo`.
- **Hooks**: `PreToolUse`, `PostToolUse`, `UserPromptSubmit`, `SessionStart`, `Stop`, `SubagentStop`; JSON qua stdin, quyết định qua exit code/JSON.

## 8. Tương thích plugin / skill / agent

**Claude Code (ưu tiên 1):** `.claude-plugin/plugin.json` + `marketplace.json`; `commands/*.md` (`description`, `allowed-tools`, `argument-hint`, `model`; `$ARGUMENTS`, `$1`, `` !`cmd` ``, `@file`); `agents/*.md`; `skills/*/SKILL.md`; `hooks/hooks.json`; `.mcp.json`; `${CLAUDE_PLUGIN_ROOT}`; đọc `CLAUDE.md`, `.claude/`, `~/.claude/`. `ta plugin add <repo>@<marketplace>` clone vào `~/.local/share/tagent/plugins`.

**Agent Skills (`SKILL.md`):** chỉ nạp `name` + `description` vào system prompt, đọc đầy đủ khi kích hoạt, file phụ đọc theo yêu cầu.

**Adapter chỉ đọc cho agent khác:** `AGENTS.md`, `GEMINI.md`, `gemini-extension.json` + command TOML, `.cursor/rules/*.mdc`, `.clinerules`, `.github/copilot-instructions.md`, `.github/prompts/*.prompt.md`, OpenCode config/agents. **MCP** là mẫu số chung: stdio, HTTP/SSE, OAuth.

`ta plugin import <path>` báo phần map được/không map được. Plugin khai báo `requires` để cảnh báo thiếu binary trên Termux.

**Skill/plugin dưới dạng toolcall:**
- Tool `Skill(name)`: model thấy danh sách tên + mô tả, gọi khi cần, trả `SKILL.md` kèm đường dẫn thư mục.
- Tool plugin: qua MCP đi kèm hoặc script có `tool.json` (schema vào/ra), subprocess JSON stdin/stdout.
- Subagent: `task(agent, prompt)` context riêng, tool giới hạn theo `tools:`, giới hạn độ sâu đệ quy, chạy song song được.
- Namespacing: `plugin__tool`, `mcp__server__tool`; permission rule áp dụng lên các tên này.
- Tin cậy: plugin mới phải xác nhận một lần (liệt kê hook/command/tool), pin commit/checksum. Cache chỉ mục metadata lúc cài.

## 9. Web UI

### 9.1 Layout

- **Mobile (< 760px):** một cột; sidebar là drawer vuốt từ trái; permission và command palette là bottom sheet; composer dính đáy.
- **Tablet/desktop:** sidebar 264px + vùng chat; panel phải (file viewer, terminal, preview) mở bằng nút trên header.

Vùng chính: header (tên task, repo·branch, trạng thái, nút Diff/Terminal) → luồng hội thoại (cột giữa, max 760px) → composer.

### 9.2 Thành phần

- **Sidebar**: logo, nút Task mới, danh sách task (chấm trạng thái: chạy/chờ/xong), mục Plugin/Skill/MCP kèm số lượng, chân hiển thị model và chi phí hôm nay.
- **Tin nhắn người dùng**: bong bóng tối căn phải, chip file đính kèm.
- **Phản hồi agent**: markdown, code highlight + nút copy, thinking gập collapsible.
- **Tool card**: mỗi dòng `tên tool · tham số · thời gian`, chấm trạng thái, gập/mở để xem input/output.
- **Diff card**: header file + `+n −n`, hunk có nền xanh/đỏ nhạt, duyệt từng hunk, nút Apply/Reject/Undo.
- **Permission card**: viền cảnh báo, preview lệnh/diff, nút `Cho phép / Luôn cho phép / Từ chối / Sửa lệnh`.
- **Composer**: textarea nhiều dòng; `/` mở command palette, `@` file picker mờ, `!` shell, `#` memory; nút đính kèm, chip permission mode, chip model, nút gửi.
- **Terminal panel** (xterm.js), **file viewer** (CodeMirror), **todo list**, **cây subagent**.
- **Settings**: provider/key, model theo vai trò, permission, plugin/skill/MCP, theme, ngôn ngữ (vi/en).

### 9.3 Design tokens (theme tối mặc định)

```
--bg        #0e0f11    --panel   #121316    --panel-2 #1c1e23
--border    #24262b    --border-2 #2b2e35
--text      #e8e9ec    --muted   #9aa0aa
--accent    #6ee7b7    --accent-ink #08130f
--warn      #f5b942    --danger  #f85149
--add-bg rgba(52,168,83,.16) --add-fg #9be3ad
--del-bg rgba(248,81,73,.15) --del-fg #ffb0aa
radius: 8 / 12 / 16 / pill      spacing: bội số 4
font UI: Inter 14/1.55          font code: JetBrains Mono 12.5
```

Theme sáng và AMOLED (nền `#000`) dẫn xuất từ cùng token. Đạt tương phản chữ ≥ 4.5:1.

### 9.4 Icon (bắt buộc)

- Chỉ **SVG inline**, bộ **Lucide**, `stroke="currentColor"`, `stroke-width` 1.6–1.8, cỡ 16/20px, `fill="none"`.
- **Cấm emoji** và cấm icon font. Icon trang trí: `aria-hidden="true"`; nút chỉ có icon: `aria-label`.
- Tạo module `icons.ts` (hoặc sprite) xuất từng icon thành component; không tải icon từ CDN.
- Cần tối thiểu: plus, paperclip, send/arrow-up, folder, file, file-diff, terminal, git-branch, plug, sparkles/skill, server/mcp, settings, check, x, chevron, copy, undo, search, menu, battery, cloud/upload, download, trash.

### 9.5 Tương tác và độ hoàn thiện

- Touch target ≥ 44px; hỗ trợ `safe-area-inset`; tránh nảy khi bàn phím ảo mở.
- Virtualized list cho hội thoại dài; lazy-load CodeMirror/xterm; tách chunk JS.
- Skeleton khi tải, trạng thái rỗng, toast lỗi; nút Stop luôn thấy được khi agent chạy.
- PWA: manifest, service worker, cài được, **share target** (Chia sẻ từ Android → tagent).
- Phím tắt cho bàn phím rời: `Ctrl+K` palette, `Ctrl+Enter` gửi, `Esc` dừng.

## 10. Upload / download file

- Cách thêm: nút đính kèm, kéo thả, dán ảnh, chụp camera, share target.
- `POST /api/uploads` (multipart, chunked cho file lớn) → `<workspace>/.ta/uploads/<session>/` → trả `file_id` + path → gắn vào message như tham chiếu `@`.
- Theo loại: ảnh (resize, vision), PDF/docx/xlsx (trích text), zip/tar (hỏi giải nén), code/text (vào workspace), file lạ (chỉ metadata).
- Download: file agent tạo hiện thành card có preview/tải; tải thư mục dạng zip.
- An toàn: giới hạn dung lượng, kiểm tra MIME, chống path traversal, không tự thực thi, tách uploads khỏi rule `Edit` mặc định.

## 11. Bảo mật

- Bind `127.0.0.1` mặc định; LAN/Tailscale phải bật rõ ràng.
- Token ngẫu nhiên lúc khởi động (in kèm URL/QR), cookie `HttpOnly` + `SameSite=Strict`.
- Kiểm tra `Origin`, chống CSRF và DNS rebinding.
- Key BYOK chỉ nhập qua Settings, lưu phía daemon, không bao giờ trả lại đầy đủ.
- Hook/script plugin chạy với quyền người dùng: bắt buộc xác nhận, hiển thị hành động trước khi chạy.
- HTTPS tùy chọn qua Tailscale hoặc Cloudflare Tunnel.

## 12. API

```
GET/POST /api/sessions
WS       /api/sessions/:id/stream   (prompt, permission.reply, interrupt)
POST     /api/uploads      GET /api/files/*path
GET      /api/tree         GET /api/diff
GET/PUT  /api/config       GET /api/providers   POST /api/auth
GET      /api/plugins|skills|agents|mcp  (+ install/enable/disable)
```

Có OpenAPI để làm app native hoặc extension sau này.

## 13. Đặc thù Termux

- Cài: `cargo install`, binary `aarch64`, sau này `pkg install ta`.
- `ta doctor` kiểm tra `git`, `ripgrep`, `termux-api`, quyền storage, Node/Python.
- Dùng `$PREFIX`, `$HOME`, không giả định `/usr`, `/tmp`.
- Tự gọi `termux-wake-lock` khi task dài; lưu session sau mỗi lượt vì Android có thể kill tiến trình.
- Session chạy ở daemon: đóng tab không mất task, mở lại resume theo `last_event_id`.

## 14. Lộ trình

| GĐ | Nội dung | Ước tính |
|---|---|---|
| M0 | Skeleton Rust, CLI, agent loop, 1 provider, tool read/write/bash, `-p` | 2 tuần |
| M1 | `ta serve`, WebSocket, chat streaming, tool card, permission sheet, session list | 3 tuần |
| M2 | Upload, file tree/viewer, diff viewer, palette, `@ ! #` | 3 tuần |
| M3 | Đa provider BYOK, Settings, chi phí, compaction, checkpoint/undo | 2 tuần |
| M4 | Loader Claude Code (commands, agents, skills, hooks, marketplace), trang plugin | 3 tuần |
| M5 | MCP, subagent UI, terminal panel, PWA/offline, adapter agent khác | 3 tuần |
| M6 | TUI mỏng, `doctor`, tool Termux:API, đóng gói, tài liệu, beta | 2 tuần |

Ưu tiên khi cắt phạm vi: M0 → M1 → M4 → M2 → M3 → còn lại.

## 15. Rủi ro

- Định dạng plugin thay đổi: adapter tách riêng, test bằng fixture plugin thật.
- Bảo mật plugin/hook và bề mặt tấn công web: opt-in cho mọi mở rộng.
- Khác biệt tool calling giữa model: lớp fallback + bộ test conformance theo provider.
- Android kill nền / trình duyệt: append-only JSONL, resume theo event id.
- Máy yếu: lazy-load, virtualized list, nén context sớm.
- Màn hình nhỏ: test trên máy thật, không chỉ emulator.

## 16. Tiêu chí hoàn thành (MVP)

1. `ta serve` mở được UI trên Chrome Android trong Termux, chat streaming ổn định.
2. Sửa file qua `edit` có diff, permission, undo.
3. Upload ảnh/PDF/zip và agent dùng được.
4. Cài và chạy được ít nhất một plugin Claude Code thật (command + skill + agent).
5. Resume session sau khi tab bị đóng hoặc daemon bị kill.
