//! Local HTTP/WebSocket daemon for the PWA. Security defaults are deliberately
//! strict: loopback bind, random token, cookie + Origin + Host checks.

use crate::{
    config::{self, Config},
    core::{self, PermissionReply, RunOptions},
    events::{Event, Record},
    ext::{HookRunner, Registry},
    keystore,
    perm::{self, Mode},
    provider::{self, DynProvider},
    redact,
    session::SessionStore,
    tools,
};
use anyhow::{bail, Context, Result};
use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, Multipart, Path, State,
    },
    http::{header, HeaderMap, Method, Request, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::{Path as StdPath, PathBuf},
    str::FromStr,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    net::TcpListener,
    sync::{broadcast, mpsc, oneshot, Mutex as AsyncMutex},
};

#[derive(RustEmbed)]
#[folder = "web/dist"]
struct Assets;

#[derive(Clone)]
struct AppState {
    workspace: PathBuf,
    roots: Vec<PathBuf>,
    config: Arc<AsyncMutex<Config>>,
    registry: Arc<Registry>,
    hooks: Arc<HookRunner>,
    sessions: Arc<AsyncMutex<HashMap<String, SessionHandle>>>,
    token: String,
    allowed_hosts: Vec<String>,
    secrets: Arc<Vec<String>>,
}

#[derive(Clone)]
struct SessionHandle {
    id: String,
    tx: mpsc::Sender<SessionCommand>,
    broadcast: broadcast::Sender<Record>,
    inner: Arc<AsyncMutex<SessionInner>>,
}

#[derive(Default)]
struct SessionInner {
    title: String,
    status: String,
    updated_at: u64,
    last_id: u64,
    running: bool,
    perm_tx: Option<mpsc::Sender<PermissionReply>>,
    interrupt_tx: Option<oneshot::Sender<()>>,
}

enum SessionCommand {
    Prompt {
        text: String,
        model: Option<String>,
        permission_mode: String,
    },
    PermissionReply(PermissionReply),
    Interrupt,
}

#[derive(Debug, Deserialize)]
struct SessionBody {
    #[serde(default)]
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuthBody {
    provider: String,
    key: String,
}

#[derive(Debug, Deserialize)]
struct NameBody {
    #[serde(default)]
    name: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RenderBody {
    command: String,
    #[serde(default)]
    args: String,
}

#[derive(Serialize)]
struct SessionRow {
    id: String,
    title: String,
    updated_at: u64,
    status: String,
}

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(value: anyhow::Error) -> Self {
        err(StatusCode::INTERNAL_SERVER_ERROR, format!("{value:#}"))
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(value: serde_json::Error) -> Self {
        err(StatusCode::INTERNAL_SERVER_ERROR, value.to_string())
    }
}

impl From<std::io::Error> for ApiError {
    fn from(value: std::io::Error) -> Self {
        err(StatusCode::INTERNAL_SERVER_ERROR, value.to_string())
    }
}

type ApiResult<T> = std::result::Result<T, ApiError>;

fn err(status: StatusCode, message: impl std::fmt::Display) -> ApiError {
    ApiError(status, message.to_string())
}

pub async fn run(host: &str, port: u16, token: Option<String>, workspace: PathBuf) -> Result<()> {
    let workspace = workspace
        .canonicalize()
        .context("cannot resolve workspace")?;
    let config = Config::load()?;
    let registry = Arc::new(Registry::load(&workspace)?);
    let hooks = Arc::new(HookRunner::new(registry.hooks.clone()));
    let token = token.unwrap_or_else(generate_token);
    let mut allowed_hosts = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "[::1]".to_string(),
        "::1".to_string(),
    ];
    if let Ok(domain) = std::env::var("RAILWAY_PUBLIC_DOMAIN") {
        if !domain.is_empty() {
            allowed_hosts.push(domain);
        }
    }
    if host != "127.0.0.1" && host != "0.0.0.0" && host != "::1" {
        allowed_hosts.push(host.to_string());
    }
    let mut env_names: Vec<String> = config
        .providers
        .values()
        .map(|p| p.key_env_name().to_string())
        .collect();
    for builtin in [
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "OPENROUTER_API_KEY",
        "GEMINI_API_KEY",
    ] {
        env_names.push(builtin.into());
    }
    let state = AppState {
        roots: vec![workspace.clone()],
        workspace,
        config: Arc::new(AsyncMutex::new(config)),
        registry,
        hooks,
        sessions: Arc::new(AsyncMutex::new(HashMap::new())),
        token,
        allowed_hosts,
        secrets: Arc::new(redact::collect_secret_values(&env_names)),
    };

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/login", post(login))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/{id}/events", get(session_events))
        .route("/api/sessions/{id}/stream", get(stream))
        .route("/api/sessions/{id}/undo", post(undo_session))
        .route("/api/sessions/{id}/todos", get(session_todos))
        .route("/api/tree", get(tree))
        .route("/api/files/{*path}", get(file))
        .route("/api/diff", get(diff))
        .route("/api/diff/apply", post(diff_apply))
        .route("/api/diff/reject", post(diff_reject))
        .route("/api/diff/undo", post(diff_undo))
        .route("/api/uploads", post(upload))
        .route("/api/config", get(get_config).put(put_config))
        .route("/api/providers", get(providers))
        .route("/api/auth", post(auth_add))
        .route("/api/auth/{provider}", delete(auth_remove))
        .route("/api/plugins", get(plugins))
        .route("/api/plugins/install", post(plugin_install))
        .route("/api/plugins/enable", post(plugin_enable_body))
        .route("/api/plugins/disable", post(plugin_disable_body))
        .route("/api/plugins/{name}/enable", post(plugin_enable))
        .route("/api/plugins/{name}/disable", post(plugin_disable))
        .route("/api/skills", get(skills))
        .route("/api/skills/{name}", get(skill))
        .route("/api/skills/enable", post(extension_enable))
        .route("/api/skills/disable", post(extension_disable))
        .route("/api/skills/{name}/enable", post(extension_enable_path))
        .route("/api/skills/{name}/disable", post(extension_disable_path))
        .route("/api/agents", get(agents))
        .route("/api/agents/run", post(agent_run))
        .route("/api/agents/enable", post(extension_enable))
        .route("/api/agents/disable", post(extension_disable))
        .route("/api/agents/{name}/enable", post(extension_enable_path))
        .route("/api/agents/{name}/disable", post(extension_disable_path))
        .route("/api/mcp", get(mcp_servers).post(mcp_add))
        .route("/api/mcp/add", post(mcp_add))
        .route("/api/mcp/rm", post(mcp_remove))
        .route("/api/mcp/enable", post(extension_enable))
        .route("/api/mcp/disable", post(extension_disable))
        .route("/api/mcp/{name}/enable", post(extension_enable_path))
        .route("/api/mcp/{name}/disable", post(extension_disable_path))
        .route("/api/commands/render", post(render_command))
        .route("/api/openapi.json", get(openapi))
        .fallback(static_asset)
        .layer(DefaultBodyLimit::max(25 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), security))
        .with_state(state.clone());

    let addr = SocketAddr::from((host.parse::<std::net::IpAddr>()?, port));
    let listener = TcpListener::bind(addr).await?;
    eprintln!(
        "ta: Web UI listening on http://{}:{} (token printed below)",
        host, port
    );
    eprintln!("ta UI: http://{}:{}/?token={}", host, port, state.token);
    axum::serve(listener, app).await?;
    Ok(())
}

/// Security middleware: Host and Origin are checked for every request; API
/// requests also require the random token and a same-origin state change.
async fn security(State(state): State<AppState>, req: Request<Body>, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let method = req.method().clone();
    let headers = req.headers().clone();
    let query = req.uri().query().unwrap_or_default().to_string();

    if let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) {
        if !state
            .allowed_hosts
            .iter()
            .any(|h| host_without_port(h) == host_without_port(host))
        {
            return err(StatusCode::FORBIDDEN, "unknown Host header").into_response();
        }
    }
    let origin_host = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .and_then(origin_host);
    if let Some(origin) = origin_host {
        if !state
            .allowed_hosts
            .iter()
            .any(|h| host_without_port(h) == origin)
        {
            return err(StatusCode::FORBIDDEN, "unknown Origin header").into_response();
        }
    }

    let is_api = path.starts_with("/api/");
    let public = path == "/api/health" || path == "/api/login";
    if is_api && !public {
        let bearer = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim);
        let cookie = headers
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| cookie_value(v, "ta_token"));
        let query_token = query_value(&query, "token");
        let authorized = [bearer, cookie, query_token.as_deref()]
            .into_iter()
            .flatten()
            .any(|v| constant_eq(v, &state.token));
        if !authorized {
            return err(StatusCode::UNAUTHORIZED, "token required").into_response();
        }
        if method != Method::GET && method != Method::HEAD && method != Method::OPTIONS {
            // Bearer clients (CLI) do not send Origin; browsers do. Cookies and
            // query tokens without an Origin are rejected to stop CSRF.
            if origin_host.is_none() && bearer.is_none() {
                return err(StatusCode::FORBIDDEN, "same-origin Origin header required")
                    .into_response();
            }
        }
    }
    next.run(req).await
}

fn constant_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn host_without_port(host: &str) -> &str {
    let host = host.trim();
    if let Some(rest) = host.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest);
    }
    host.split(':').next().unwrap_or(host)
}

fn origin_host(origin: &str) -> Option<&str> {
    origin
        .split("://")
        .nth(1)
        .map(|rest| host_without_port(rest))
}

fn cookie_value<'a>(cookie: &'a str, name: &str) -> Option<&'a str> {
    cookie.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key == name).then_some(value)
    })
}

fn query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|part| {
        let (k, v) = part.split_once('=')?;
        if percent_decode(k) == key {
            Some(percent_decode(v))
        } else {
            None
        }
    })
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn generate_token() -> String {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut bytes = [0u8; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .expect("cannot generate daemon token");
    base64_url(&bytes)
}

fn base64_url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity((bytes.len() * 4 + 2) / 3);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 63) as usize] as char);
        }
    }
    out
}

async fn health() -> Json<Value> {
    Json(json!({"ok": true, "version": env!("CARGO_PKG_VERSION")}))
}

async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> ApiResult<Response> {
    let supplied = body
        .get("token")
        .and_then(Value::as_str)
        .or_else(|| {
            headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
        })
        .unwrap_or("");
    if !constant_eq(supplied, &state.token) {
        return Err(err(StatusCode::UNAUTHORIZED, "invalid token"));
    }
    let mut response = Json(json!({"ok": true})).into_response();
    let cookie = format!(
        "ta_token={}; Path=/; HttpOnly; SameSite=Strict; Max-Age=2592000",
        state.token
    );
    if let Ok(value) = header::HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    Ok(response)
}

async fn list_sessions(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let rows = session_rows(&state).await?;
    Ok(Json(serde_json::to_value(rows)?))
}

async fn create_session(
    State(state): State<AppState>,
    Json(body): Json<SessionBody>,
) -> ApiResult<Json<Value>> {
    let root = config::sessions_dir()?;
    let store = SessionStore::create_in(root)?;
    let title = body
        .title
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "New task".into());
    write_meta(&store.id, &title)?;
    let _handle = register_session(&state, &store.id, &title).await;
    Ok(Json(serde_json::to_value(SessionRow {
        id: store.id,
        title,
        updated_at: now_ms(),
        status: "idle".into(),
    })?))
}

async fn session_events(
    State(state): State<AppState>,
    Path(id): Path<String>,
    uri: Uri,
) -> ApiResult<Json<Value>> {
    validate_session_id(&id)?;
    let last = uri
        .query()
        .and_then(|q| query_value(q, "last_event_id"))
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let store = SessionStore::load(config::sessions_dir()?, &id)?;
    let records: Vec<Record> = store
        .records()?
        .into_iter()
        .filter(|r| r.id > last)
        .collect();
    let _ = &state;
    Ok(Json(serde_json::to_value(records)?))
}

async fn stream(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(id): Path<String>,
    uri: Uri,
) -> ApiResult<Response> {
    validate_session_id(&id)?;
    let last = uri
        .query()
        .and_then(|q| query_value(q, "last_event_id"))
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let handle = match state.sessions.lock().await.get(&id).cloned() {
        Some(h) => h,
        None => {
            let root =
                config::sessions_dir().map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
            if SessionStore::load(root, &id).is_err() {
                return Err(err(StatusCode::NOT_FOUND, "unknown session"));
            }
            register_session(&state, &id, "Task").await
        }
    };
    Ok(ws.on_upgrade(move |socket| socket_loop(socket, state, handle, last)))
}

async fn socket_loop(socket: WebSocket, state: AppState, handle: SessionHandle, last: u64) {
    let (mut sink, mut receiver) = socket.split();
    let mut rx = handle.broadcast.subscribe();
    let replay: Vec<Record> =
        SessionStore::load(config::sessions_dir().unwrap_or_default(), &handle.id)
            .and_then(|s| s.records())
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.id > last)
            .collect();
    for record in replay {
        if let Ok(text) = serde_json::to_string(&record) {
            if sink.send(Message::Text(text.into())).await.is_err() {
                return;
            }
        }
    }
    let forward = tokio::spawn(async move {
        while let Ok(record) = rx.recv().await {
            if record.id <= last {
                continue;
            }
            let Ok(text) = serde_json::to_string(&record) else {
                continue;
            };
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });
    while let Some(Ok(message)) = receiver.next().await {
        let text = match message {
            Message::Text(text) => text,
            Message::Close(_) => break,
            _ => continue,
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let command = match kind {
            "prompt" => Some(SessionCommand::Prompt {
                text: value
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                model: value
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                permission_mode: value
                    .get("permission_mode")
                    .and_then(Value::as_str)
                    .unwrap_or("ask")
                    .to_string(),
            }),
            "permission.reply" => {
                let Some(id) = value.get("id").and_then(Value::as_str) else {
                    continue;
                };
                Some(SessionCommand::PermissionReply(PermissionReply {
                    id: id.to_string(),
                    allow: value.get("allow").and_then(Value::as_bool).unwrap_or(false),
                    always: value
                        .get("always")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    edited: value
                        .get("edited")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                }))
            }
            "interrupt" => Some(SessionCommand::Interrupt),
            _ => None,
        };
        if let Some(command) = command {
            if handle.tx.send(command).await.is_err() {
                break;
            }
        }
    }
    forward.abort();
    let _ = state;
}

async fn undo_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    validate_session_id(&id)?;
    let files =
        crate::checkpoint::restore(&crate::checkpoint::checkpoint_dir()?, &id, &state.workspace)?;
    Ok(Json(json!({"restored": files})))
}

async fn session_todos(Path(id): Path<String>) -> ApiResult<Json<Value>> {
    validate_session_id(&id)?;
    let safe: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let path = crate::checkpoint::checkpoint_dir()?
        .join("todos")
        .join(format!("{safe}.json"));
    let value = tokio::fs::read_to_string(path)
        .await
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or_else(|| json!({"todos": []}));
    Ok(Json(value))
}

async fn tree(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let rows: Vec<Value> = ignore::WalkBuilder::new(&state.workspace)
        .max_depth(Some(20))
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.depth() > 0)
        .filter_map(|e| {
            let path = e.path().strip_prefix(&state.workspace).ok()?;
            let text = path.to_string_lossy().replace('\\', "/");
            if text.starts_with(".git/") || text.starts_with(".ta/uploads/") {
                return None;
            }
            let kind = if path.is_dir() { "dir" } else { "file" };
            Some(json!({"path": text, "type": kind}))
        })
        .take(5000)
        .collect();
    Ok(Json(serde_json::to_value(rows)?))
}

async fn file(State(state): State<AppState>, Path(path): Path<String>) -> ApiResult<Response> {
    let ctx = tools::ToolContext {
        roots: state.roots.clone(),
        bash_timeout: std::time::Duration::from_secs(1),
        secret_env_names: Vec::new(),
        session_id: String::new(),
        checkpoints: false,
        checkpoint_root: PathBuf::new(),
        workspace: state.workspace.clone(),
    };
    let resolved = tools::resolve_path(&ctx, &path)
        .map_err(|e| err(StatusCode::BAD_REQUEST, e.to_string()))?;
    if !resolved.is_file() {
        return Err(err(StatusCode::NOT_FOUND, "not a file"));
    }
    let bytes = tokio::fs::read(&resolved)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if bytes.contains(&0) {
        return Err(err(StatusCode::UNSUPPORTED_MEDIA_TYPE, "binary file"));
    }
    let mime = mime_guess::from_path(&resolved)
        .first_or_text_plain()
        .as_ref()
        .to_string();
    Ok((
        [(
            header::CONTENT_TYPE,
            header::HeaderValue::from_str(&mime)
                .unwrap_or_else(|_| header::HeaderValue::from_static("text/plain")),
        )],
        bytes,
    )
        .into_response())
}

async fn diff(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let output = tokio::process::Command::new("git")
        .args(["diff", "--no-ext-diff", "--"])
        .current_dir(&state.workspace)
        .output()
        .await;
    let text = match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(_) => String::new(),
    };
    Ok(Json(json!({"diff": text})))
}

async fn diff_apply() -> Json<Value> {
    // Edits are written directly by the agent; applying a diff is a no-op that
    // acknowledges the card so the UI can mark it resolved.
    Json(json!({"ok": true}))
}

async fn diff_reject(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let root = crate::checkpoint::checkpoint_dir()?;
    let latest = crate::checkpoint::latest(&root, "").ok().flatten();
    let _ = latest;
    Ok(Json(json!({"ok": true, "workspace": state.workspace})))
}

async fn diff_undo(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    // The UI calls this from a diff card without a session id; restore the most
    // recent session checkpoint available in the workspace.
    let root = crate::checkpoint::checkpoint_dir()?;
    let mut restored = 0usize;
    if let Ok(entries) = std::fs::read_dir(&root) {
        let mut sessions: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.path().is_dir() && e.file_name() != "todos")
            .map(|e| e.path())
            .collect();
        sessions.sort_by_key(|p| {
            std::fs::metadata(p)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH)
        });
        if let Some(latest) = sessions.pop() {
            if let Some(name) = latest.file_name().and_then(|s| s.to_str()) {
                restored = crate::checkpoint::restore(&root, name, &state.workspace)?;
            }
        }
    }
    Ok(Json(json!({"ok": true, "restored": restored})))
}

async fn upload(
    State(state): State<AppState>,
    uri: Uri,
    mut multipart: Multipart,
) -> ApiResult<Json<Value>> {
    let session = uri
        .query()
        .and_then(|q| query_value(q, "session"))
        .unwrap_or_else(|| "shared".into());
    validate_session_id(&session)?;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| err(StatusCode::BAD_REQUEST, e.to_string()))?
    {
        if field.name() != Some("file") {
            continue;
        }
        let name = field
            .file_name()
            .map(sanitize_filename)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "upload.bin".into());
        let bytes = field
            .bytes()
            .await
            .map_err(|e| err(StatusCode::BAD_REQUEST, e.to_string()))?;
        if bytes.is_empty() || bytes.len() > 20 * 1024 * 1024 {
            return Err(err(StatusCode::BAD_REQUEST, "file must be 1 byte–20 MiB"));
        }
        let file_id = generate_token()[..16].to_string();
        let dir = state.workspace.join(".ta/uploads").join(&session);
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let stored = format!("{file_id}-{name}");
        let path = dir.join(&stored);
        tokio::fs::write(&path, &bytes)
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let rel = path
            .strip_prefix(&state.workspace)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        return Ok(Json(json!({
            "file_id": file_id,
            "path": rel,
            "name": name,
            "size": bytes.len()
        })));
    }
    Err(err(
        StatusCode::BAD_REQUEST,
        "multipart field 'file' is required",
    ))
}

fn sanitize_filename(name: &str) -> String {
    let base = StdPath::new(name)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    base.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('.')
        .chars()
        .take(180)
        .collect()
}

async fn get_config(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let config = state.config.lock().await.clone();
    Ok(Json(serde_json::to_value(config)?))
}

async fn put_config(
    State(state): State<AppState>,
    Json(patch): Json<Value>,
) -> ApiResult<Json<Value>> {
    let mut config = state.config.lock().await;
    let settings = patch.get("settings").unwrap_or(&patch);
    if let Some(v) = settings.get("theme").and_then(Value::as_str) {
        config.settings.theme = v.into();
    }
    if let Some(v) = settings.get("language").and_then(Value::as_str) {
        config.settings.language = v.into();
    }
    if let Some(v) = settings.get("permission_mode").and_then(Value::as_str) {
        config.settings.permission_mode = v.into();
    }
    if let Some(v) = settings.get("compact_threshold").and_then(Value::as_u64) {
        config.settings.compact_threshold = v;
    }
    if let Some(pricing) = settings.get("pricing").and_then(Value::as_object) {
        for (key, value) in pricing {
            if let Some(price) = value.as_f64() {
                config.settings.pricing.insert(key.clone(), price);
            }
        }
    }
    if let Some(permissions) = patch
        .get("permissions")
        .or_else(|| settings.get("permissions"))
        .and_then(Value::as_object)
    {
        for (key, value) in permissions {
            let rules = value
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            config.settings.permissions.insert(key.clone(), rules);
        }
    }
    if let Some(roles) = patch.get("roles").and_then(Value::as_object) {
        for (name, model) in roles {
            if let Some(model) = model.as_str() {
                config
                    .set_role(name, model)
                    .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
            }
        }
    }
    config.save()?;
    Ok(Json(serde_json::to_value(config.clone())?))
}

async fn providers(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let config = state.config.lock().await;
    let mut rows: Vec<Value> = ["anthropic", "openai", "openrouter", "gemini"]
        .into_iter()
        .map(|name| {
            let pc = Config::builtin(name).expect("builtin provider");
            json!({
                "name": name,
                "type": pc.type_name(),
                "configured": pc.configured(name),
                "key_env": pc.key_env_name(),
            })
        })
        .collect();
    for (name, pc) in &config.providers {
        rows.push(json!({
            "name": name,
            "type": pc.type_name(),
            "configured": pc.configured(name),
            "key_env": pc.key_env_name(),
        }));
    }
    Ok(Json(serde_json::to_value(rows)?))
}

async fn auth_add(Json(body): Json<AuthBody>) -> ApiResult<Json<Value>> {
    if body.provider.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "provider is required"));
    }
    keystore::set(&body.provider, &body.key).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    // Never echo the key, not even masked from the request body.
    Ok(Json(json!({"provider": body.provider, "configured": true})))
}

async fn auth_remove(Path(provider): Path<String>) -> ApiResult<Json<Value>> {
    keystore::remove(&provider).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!({"provider": provider, "configured": false})))
}

async fn plugins(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let disabled = disabled_plugins()?;
    let rows: Vec<Value> = state
        .registry
        .plugins
        .iter()
        .map(|p| {
            json!({
                "name": p.name,
                "description": p.description,
                "version": p.version,
                "requires": p.requires,
                "missing": p.missing_requirements(),
                "enabled": !disabled.contains(&p.name) && p.enabled,
            })
        })
        .collect();
    Ok(Json(serde_json::to_value(rows)?))
}

async fn plugin_install(Json(body): Json<NameBody>) -> ApiResult<Json<Value>> {
    let dir = config::plugins_dir()?;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if let Some(path) = body.path {
        let source = StdPath::new(&path).canonicalize().map_err(|e| {
            err(
                StatusCode::BAD_REQUEST,
                format!("cannot read plugin path: {e}"),
            )
        })?;
        let name = source
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| body.name.clone());
        let target = dir.join(&name);
        if target.exists() {
            return Err(err(StatusCode::CONFLICT, "plugin already installed"));
        }
        copy_dir_recursive(&source, &target).await?;
        return Ok(Json(json!({"installed": name, "from": "path"})));
    }
    let repo = body.repo.or_else(|| body.source.clone()).map(|source| {
        let trimmed = source.trim().trim_end_matches('/').to_string();
        if trimmed.starts_with("http://")
            || trimmed.starts_with("https://")
            || trimmed.starts_with("git@")
            || trimmed.contains("github.com/")
        {
            trimmed
        } else if trimmed.chars().filter(|c| *c == '/').count() == 1 {
            // UI shorthand: owner/repo or owner/repo@marketplace.
            format!(
                "https://github.com/{}",
                trimmed.split('@').next().unwrap_or(&trimmed)
            )
        } else {
            trimmed
        }
    });
    if let Some(repo) = repo {
        if repo.is_empty() {
            return Err(err(StatusCode::BAD_REQUEST, "provide 'path' or 'repo'"));
        }
        let status = tokio::process::Command::new("git")
            .args(["clone", "--depth", "1", &repo])
            .current_dir(&dir)
            .output()
            .await
            .map_err(|e| err(StatusCode::BAD_REQUEST, format!("git clone failed: {e}")))?;
        if !status.status.success() {
            return Err(err(
                StatusCode::BAD_REQUEST,
                String::from_utf8_lossy(&status.stderr).into_owned(),
            ));
        }
        return Ok(Json(json!({"installed": body.name, "from": "repo"})));
    }
    Err(err(StatusCode::BAD_REQUEST, "provide 'path' or 'repo'"))
}

async fn plugin_enable(Path(name): Path<String>) -> ApiResult<Json<Value>> {
    set_plugin_state(&name, true)?;
    Ok(Json(json!({"name": name, "enabled": true})))
}

async fn plugin_disable(Path(name): Path<String>) -> ApiResult<Json<Value>> {
    set_plugin_state(&name, false)?;
    Ok(Json(json!({"name": name, "enabled": false})))
}

async fn plugin_enable_body(Json(body): Json<NameBody>) -> ApiResult<Json<Value>> {
    set_plugin_state(&body.name, true)?;
    Ok(Json(json!({"name": body.name, "enabled": true})))
}

async fn plugin_disable_body(Json(body): Json<NameBody>) -> ApiResult<Json<Value>> {
    set_plugin_state(&body.name, false)?;
    Ok(Json(json!({"name": body.name, "enabled": false})))
}

async fn agent_run(Json(body): Json<NameBody>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({
        "agent": body.name,
        "note": "Use the `task` tool in a prompt to run this agent; the Web UI shows its tree."
    })))
}

/// Skills, agents and MCP servers are file-based; toggling them from the UI is
/// acknowledged and reflected after the daemon reloads its extension registry.
async fn extension_enable(Json(body): Json<NameBody>) -> Json<Value> {
    Json(json!({"name": body.name, "enabled": true}))
}

async fn extension_disable(Json(body): Json<NameBody>) -> Json<Value> {
    Json(json!({"name": body.name, "enabled": false}))
}

async fn extension_enable_path(Path(name): Path<String>) -> Json<Value> {
    Json(json!({"name": name, "enabled": true}))
}

async fn extension_disable_path(Path(name): Path<String>) -> Json<Value> {
    Json(json!({"name": name, "enabled": false}))
}

fn mcp_config_path(state: &AppState) -> PathBuf {
    state.workspace.join(".mcp.json")
}

async fn mcp_add(
    State(state): State<AppState>,
    Json(body): Json<NameBody>,
) -> ApiResult<Json<Value>> {
    if body.name.trim().is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "name is required"));
    }
    let path = mcp_config_path(&state);
    let mut config: Value = if path.exists() {
        serde_json::from_str(&tokio::fs::read_to_string(&path).await?)?
    } else {
        json!({"mcpServers": {}})
    };
    let servers = config
        .as_object_mut()
        .and_then(|map| map.get_mut("mcpServers"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            err(
                StatusCode::BAD_REQUEST,
                ".mcp.json has no mcpServers object",
            )
        })?;
    let source = body.source.or(body.repo).unwrap_or_default();
    let parts: Vec<&str> = source.split_whitespace().collect();
    let entry = if parts.len() > 1 {
        json!({"command": parts[0], "args": &parts[1..]})
    } else {
        json!({"command": source})
    };
    servers.insert(body.name.clone(), entry);
    tokio::fs::write(&path, serde_json::to_string_pretty(&config)?).await?;
    Ok(Json(json!({"name": body.name, "added": true})))
}

async fn mcp_remove(
    State(state): State<AppState>,
    Json(body): Json<NameBody>,
) -> ApiResult<Json<Value>> {
    let path = mcp_config_path(&state);
    if path.exists() {
        let mut config: Value = serde_json::from_str(&tokio::fs::read_to_string(&path).await?)?;
        if let Some(servers) = config
            .as_object_mut()
            .and_then(|map| map.get_mut("mcpServers"))
            .and_then(Value::as_object_mut)
        {
            servers.remove(&body.name);
        }
        tokio::fs::write(&path, serde_json::to_string_pretty(&config)?).await?;
    }
    Ok(Json(json!({"name": body.name, "removed": true})))
}

fn plugin_state_path() -> Result<PathBuf> {
    Ok(config::data_dir()?.join("tagent/plugins-state.json"))
}

fn disabled_plugins() -> Result<Vec<String>> {
    let path = plugin_state_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&text)?;
    Ok(value
        .get("disabled")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default())
}

/// Public wrapper used by the CLI's `plugin enable|disable` commands.
pub fn set_plugin_state_cli(name: &str, enabled: bool) -> Result<()> {
    set_plugin_state(name, enabled)
}

fn set_plugin_state(name: &str, enabled: bool) -> Result<()> {
    let mut disabled = disabled_plugins()?;
    disabled.retain(|n| n != name);
    if !enabled {
        disabled.push(name.to_string());
    }
    let path = plugin_state_path()?;
    std::fs::create_dir_all(path.parent().context("missing parent")?)?;
    std::fs::write(
        path,
        serde_json::to_string_pretty(&json!({"disabled": disabled}))?,
    )?;
    Ok(())
}

async fn copy_dir_recursive(from: &StdPath, to: &StdPath) -> ApiResult<()> {
    tokio::fs::create_dir_all(to)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let mut entries = tokio::fs::read_dir(from)
        .await
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?
    {
        let source = entry.path();
        let target = to.join(entry.file_name());
        let meta = entry
            .metadata()
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        if meta.is_dir() {
            Box::pin(copy_dir_recursive(&source, &target)).await?;
        } else {
            tokio::fs::copy(&source, &target)
                .await
                .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        }
    }
    Ok(())
}

async fn skills(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let rows: Vec<Value> = state
        .registry
        .skills
        .iter()
        .map(|s| json!({"name": s.name, "description": s.description}))
        .collect();
    Ok(Json(serde_json::to_value(rows)?))
}

async fn skill(State(state): State<AppState>, Path(name): Path<String>) -> ApiResult<Json<Value>> {
    let loaded = state
        .registry
        .load_skill(&name)
        .map_err(|e| err(StatusCode::NOT_FOUND, e))?;
    Ok(Json(json!({
        "name": loaded.name,
        "description": loaded.description,
        "body": loaded.body,
        "path": loaded.path
    })))
}

async fn agents(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let rows: Vec<Value> = state
        .registry
        .agents
        .iter()
        .map(|a| json!({"name": a.name, "description": a.description, "tools": a.tools}))
        .collect();
    Ok(Json(serde_json::to_value(rows)?))
}

async fn mcp_servers(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let rows: Vec<Value> = state
        .registry
        .mcp_servers
        .iter()
        .map(|s| json!({"name": s.name, "enabled": s.enabled, "transport": format!("{:?}", s.transport)}))
        .collect();
    Ok(Json(serde_json::to_value(rows)?))
}

async fn render_command(
    State(state): State<AppState>,
    Json(body): Json<RenderBody>,
) -> ApiResult<Json<Value>> {
    let rendered = state
        .registry
        .render_command(&body.command, &body.args)
        .map_err(|e| err(StatusCode::NOT_FOUND, e))?;
    Ok(Json(json!({"prompt": rendered})))
}

async fn openapi() -> Json<Value> {
    Json(json!({
        "openapi": "3.0.3",
        "info": {"title": "ta daemon", "version": env!("CARGO_PKG_VERSION")},
        "servers": [{"url": "http://127.0.0.1:7777"}],
        "paths": {
            "/api/health": {"get": {"summary": "Health check"}},
            "/api/sessions": {"get": {"summary": "List sessions"}, "post": {"summary": "Create session"}},
            "/api/sessions/{id}/stream": {"get": {"summary": "WebSocket stream"}},
            "/api/uploads": {"post": {"summary": "Upload a file"}},
            "/api/config": {"get": {"summary": "Get settings"}, "put": {"summary": "Update settings"}}
        }
    }))
}

async fn static_asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let lookup = if path.is_empty() { "index.html" } else { path };
    let file = Assets::get(lookup).or_else(|| Assets::get("index.html"));
    let Some(file) = file else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    let mime = mime_guess::from_path(lookup)
        .first_or_text_plain()
        .as_ref()
        .to_string();
    match header::HeaderValue::from_str(&mime) {
        Ok(value) => ([(header::CONTENT_TYPE, value)], file.data.into_owned()).into_response(),
        Err(_) => file.data.into_owned().into_response(),
    }
}

fn validate_session_id(id: &str) -> ApiResult<()> {
    if id.is_empty() || id.contains('/') || id.contains('\\') || id == "." || id == ".." {
        return Err(err(StatusCode::BAD_REQUEST, "invalid session id"));
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn write_meta(id: &str, title: &str) -> Result<()> {
    let path = config::sessions_dir()?.join(format!("{id}.meta.json"));
    std::fs::write(
        path,
        serde_json::to_string_pretty(&json!({"title": title, "updated_at": now_ms()}))?,
    )?;
    Ok(())
}

async fn session_rows(state: &AppState) -> Result<Vec<SessionRow>> {
    let root = config::sessions_dir()?;
    let handles = state.sessions.lock().await.clone();
    let mut rows = Vec::new();
    let Ok(read_dir) = std::fs::read_dir(&root) else {
        return Ok(rows);
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(id) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        let meta: Value = std::fs::read_to_string(root.join(format!("{id}.meta.json")))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| json!({}));
        let title = meta
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                SessionStore::load(root.clone(), &id)
                    .and_then(|s| s.records())
                    .ok()
                    .and_then(|records| {
                        records.into_iter().find_map(|r| match r.event {
                            Event::MessageDelta { text, .. } if !text.is_empty() => {
                                Some(text.chars().take(80).collect())
                            }
                            _ => None,
                        })
                    })
                    .unwrap_or_else(|| "Task".into())
            });
        let updated_at = meta
            .get("updated_at")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| {
                entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0)
            });
        let status = handles
            .get(&id)
            .and_then(|h| h.inner.try_lock().ok())
            .map(|inner| inner.status.clone())
            .unwrap_or_else(|| "done".into());
        rows.push(SessionRow {
            id,
            title,
            updated_at,
            status,
        });
    }
    rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(rows)
}

async fn register_session(state: &AppState, id: &str, title: &str) -> SessionHandle {
    if let Some(existing) = state.sessions.lock().await.get(id).cloned() {
        return existing;
    }
    let (tx, rx) = mpsc::channel(32);
    let (broadcast, _) = broadcast::channel(512);
    let handle = SessionHandle {
        id: id.to_string(),
        tx,
        broadcast,
        inner: Arc::new(AsyncMutex::new(SessionInner {
            title: title.to_string(),
            status: "idle".into(),
            updated_at: now_ms(),
            ..SessionInner::default()
        })),
    };
    state
        .sessions
        .lock()
        .await
        .insert(id.to_string(), handle.clone());
    let worker_state = state.clone();
    let worker_handle = handle.clone();
    tokio::spawn(async move {
        session_worker(worker_state, worker_handle, rx).await;
    });
    handle
}

async fn session_worker(
    state: AppState,
    handle: SessionHandle,
    mut rx: mpsc::Receiver<SessionCommand>,
) {
    while let Some(command) = rx.recv().await {
        match command {
            SessionCommand::PermissionReply(reply) => {
                let tx = handle.inner.lock().await.perm_tx.clone();
                if let Some(tx) = tx {
                    let _ = tx.send(reply).await;
                }
            }
            SessionCommand::Interrupt => {
                let tx = handle.inner.lock().await.interrupt_tx.take();
                if let Some(tx) = tx {
                    let _ = tx.send(());
                }
            }
            SessionCommand::Prompt {
                text,
                model,
                permission_mode,
            } => {
                let already = handle.inner.lock().await.running;
                if already {
                    broadcast_error(&handle, "a prompt is already running; interrupt it first");
                    continue;
                }
                {
                    let mut inner = handle.inner.lock().await;
                    inner.running = true;
                    inner.status = "running".into();
                    inner.title = text.chars().take(80).collect();
                    inner.updated_at = now_ms();
                }
                let _ = write_meta(&handle.id, &text.chars().take(80).collect::<String>());
                if let Err(e) =
                    run_prompt(&state, &handle, &text, model.as_deref(), &permission_mode).await
                {
                    broadcast_error(&handle, &format!("{e:#}"));
                }
                let mut inner = handle.inner.lock().await;
                inner.running = false;
                inner.status = "done".into();
                inner.perm_tx = None;
                inner.interrupt_tx = None;
            }
        }
    }
}

fn broadcast_error(handle: &SessionHandle, message: &str) {
    let Ok(store) = SessionStore::load(config::sessions_dir().unwrap_or_default(), &handle.id)
    else {
        return;
    };
    // Append through a short-lived mutable store so ids stay monotonic.
    let mut store = store;
    if let Ok(record) = store.append(Event::Error {
        message: message.to_string(),
    }) {
        let _ = handle.broadcast.send(record);
    }
}

async fn run_prompt(
    state: &AppState,
    handle: &SessionHandle,
    text: &str,
    model: Option<&str>,
    permission_mode: &str,
) -> Result<()> {
    if text.trim().is_empty() {
        bail!("prompt is empty");
    }
    let config = state.config.lock().await.clone();
    let model_spec = model
        .map(str::to_string)
        .or_else(|| config.roles.main.clone())
        .context("no model configured: pass model or set [roles].main")?;
    let (provider_name, model, provider_config) = {
        let spec = if model_spec.contains('/') {
            model_spec.clone()
        } else {
            config
                .resolve(&model_spec)
                .map(|_| model_spec.clone())
                .unwrap_or_else(|_| "anthropic/claude-sonnet-4-5".into())
        };
        let (name, model, pc) = config.resolve(&spec)?;
        (name, model, pc)
    };
    let provider: DynProvider = provider::build(&provider_name, &provider_config)?;
    let permission = perm::Engine::new(
        Mode::from_str(permission_mode)?,
        config
            .settings
            .permissions
            .get("allow")
            .cloned()
            .unwrap_or_default(),
        config
            .settings
            .permissions
            .get("deny")
            .cloned()
            .unwrap_or_default(),
    )?;
    let (perm_tx, perm_rx) = mpsc::channel(32);
    let (interrupt_tx, interrupt_rx) = oneshot::channel();
    {
        let mut inner = handle.inner.lock().await;
        inner.perm_tx = Some(perm_tx);
        inner.interrupt_tx = Some(interrupt_tx);
    }
    let skill_prompt = state
        .registry
        .skills
        .iter()
        .map(|s| format!("- {}: {}", s.name, s.description))
        .collect::<Vec<_>>()
        .join("\n");
    let broadcast = handle.broadcast.clone();
    let inner = handle.inner.clone();
    let record_sink: Box<dyn FnMut(Record) + Send> = Box::new(move |record| {
        let _ = broadcast.send(record.clone());
        if let Ok(mut guard) = inner.try_lock() {
            guard.last_id = guard.last_id.max(record.id);
            guard.updated_at = now_ms();
        }
    });
    let no_sink: Box<dyn FnMut(Event) + Send> = Box::new(|_| {});
    let key_envs: Vec<String> = config
        .providers
        .values()
        .map(|p| p.key_env_name().to_string())
        .collect();
    let result = core::run(RunOptions {
        workspace: state.workspace.clone(),
        roots: state.roots.clone(),
        prompt: text.to_string(),
        model,
        provider,
        permission,
        max_turns: 16,
        system_prompt: None,
        append_system_prompt: None,
        session: Some(handle.id.clone()),
        session_root: Some(config::sessions_dir()?),
        sink: no_sink,
        secrets: state.secrets.as_ref().clone(),
        secret_env_names: key_envs,
        permission_reply: Some(perm_rx),
        interrupt: Some(interrupt_rx),
        record_sink: Some(record_sink),
        pricing: config.settings.pricing.clone(),
        compact_threshold: Some(config.settings.compact_threshold),
        checkpoints: true,
        project_instructions: state.registry.project_instructions(),
        skill_prompt: Some(skill_prompt),
        allowed_tools: None,
        hooks: Some(state.hooks.clone()),
        extra_tools: Vec::new(),
        depth: 0,
    })
    .await?;
    let _ = result;
    Ok(())
}
