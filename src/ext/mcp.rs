//! MCP compatibility: `.mcp.json` parsing plus a stdio JSON-RPC client.
//! HTTP/SSE is best-effort (see `TODO(verify)` on `HttpMcpClient`).

use anyhow::{anyhow, bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE};
use serde::{
    de::Error as DeError, ser::Error as SerError, Deserialize, Deserializer, Serialize, Serializer,
};
use serde_json::{Map as JsonMap, Value};
use std::{collections::BTreeMap, fmt, path::Path, process::Stdio, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command as TokioCommand};

use crate::ext::frontmatter::split_tokens;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, PartialEq)]
pub enum McpTransport {
    Stdio {
        command: String,
        args: Vec<String>,
        env: BTreeMap<String, String>,
    },
    Http {
        url: String,
        headers: BTreeMap<String, String>,
    },
}

impl fmt::Debug for McpTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never print env values or auth headers: they may contain API keys.
        match self {
            Self::Stdio { command, args, env } => f
                .debug_struct("Stdio")
                .field("command", command)
                .field("args", args)
                .field("env", &env.keys().collect::<Vec<_>>())
                .finish(),
            Self::Http { url, headers } => f
                .debug_struct("Http")
                .field("url", url)
                .field("headers", &headers.keys().collect::<Vec<_>>())
                .finish(),
        }
    }
}

impl McpTransport {
    /// Parse the per-server object from `.mcp.json`. Unknown fields are ignored.
    pub fn from_value(value: &Value) -> Result<Self> {
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if kind == "http" || kind == "sse" {
            let url = value
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("HTTP MCP server requires 'url'"))?;
            return Ok(Self::Http {
                url: url.to_string(),
                headers: string_map(value.get("headers"), "headers")?,
            });
        }
        if let Some(command) = value.get("command").and_then(Value::as_str) {
            return Ok(Self::Stdio {
                command: command.to_string(),
                args: string_list(value.get("args")),
                env: string_map(value.get("env"), "env")?,
            });
        }
        if let Some(url) = value.get("url").and_then(Value::as_str) {
            return Ok(Self::Http {
                url: url.to_string(),
                headers: string_map(value.get("headers"), "headers")?,
            });
        }
        bail!("MCP server needs 'command' or 'url'")
    }
}

impl Serialize for McpTransport {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = JsonMap::new();
        match self {
            Self::Stdio { command, args, env } => {
                map.insert("type".into(), Value::String("stdio".into()));
                map.insert("command".into(), Value::String(command.clone()));
                map.insert(
                    "args".into(),
                    serde_json::to_value(args).map_err(SerError::custom)?,
                );
                map.insert(
                    "env".into(),
                    serde_json::to_value(env).map_err(SerError::custom)?,
                );
            }
            Self::Http { url, headers } => {
                map.insert("type".into(), Value::String("http".into()));
                map.insert("url".into(), Value::String(url.clone()));
                map.insert(
                    "headers".into(),
                    serde_json::to_value(headers).map_err(SerError::custom)?,
                );
            }
        }
        map.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for McpTransport {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Self::from_value(&value).map_err(DeError::custom)
    }
}

#[derive(Clone)]
pub struct McpServer {
    pub name: String,
    pub transport: McpTransport,
    pub enabled: bool,
}

impl fmt::Debug for McpServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpServer")
            .field("name", &self.name)
            .field("transport", &self.transport)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl McpServer {
    pub fn parse(name: &str, value: &Value) -> Result<Self> {
        let enabled = value
            .get("enabled")
            .and_then(Value::as_bool)
            .or_else(|| {
                value
                    .get("disabled")
                    .or_else(|| value.get("disable"))
                    .and_then(Value::as_bool)
                    .map(|d| !d)
            })
            .unwrap_or(true);
        let transport = match value.get("transport") {
            Some(inner) => McpTransport::from_value(inner)?,
            None => McpTransport::from_value(value)?,
        };
        Ok(Self {
            name: name.to_string(),
            transport,
            enabled,
        })
    }
}

impl Serialize for McpServer {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serde_json::to_value(&self.transport)
            .map_err(SerError::custom)?
            .as_object()
            .cloned()
            .unwrap_or_default();
        map.insert("enabled".into(), Value::Bool(self.enabled));
        map.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for McpServer {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let name = value
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| DeError::custom("MCP server has no name"))?;
        McpServer::parse(name, &value).map_err(DeError::custom)
    }
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .collect(),
        Some(Value::String(s)) => split_tokens(s),
        _ => Vec::new(),
    }
}

fn string_map(value: Option<&Value>, label: &str) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    let Some(value) = value else {
        return Ok(out);
    };
    let obj = value
        .as_object()
        .ok_or_else(|| anyhow!("{} must be an object", label))?;
    for (key, value) in obj {
        let text = match value {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Null => continue,
            _ => value.to_string(),
        };
        out.insert(key.clone(), text);
    }
    Ok(out)
}

/// Parse `{ "mcpServers": { name: {...} } }` (also accepts `servers`).
pub fn parse_servers(value: &Value) -> Result<Vec<McpServer>> {
    let servers = value
        .get("mcpServers")
        .or_else(|| value.get("servers"))
        .ok_or_else(|| anyhow!("MCP config has no 'mcpServers' object"))?;
    parse_servers_map(servers)
}

/// Parse a bare `{ name: {...} }` map, as used by plugin manifests.
pub fn parse_servers_map(value: &Value) -> Result<Vec<McpServer>> {
    let servers = value
        .as_object()
        .ok_or_else(|| anyhow!("MCP servers must be an object"))?;
    let mut out = Vec::new();
    for (name, config) in servers {
        // A bare `false` is a common way to disable a server.
        if matches!(config, Value::Bool(false)) {
            continue;
        }
        // One malformed server should not prevent the rest from loading.
        if let Ok(server) = McpServer::parse(name, config) {
            out.push(server);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub fn load_mcp_file(path: &Path) -> Result<Vec<McpServer>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read MCP config {}", path.display()))?;
    let value: Value = serde_json::from_str(&text)
        .with_context(|| format!("invalid MCP config {}", path.display()))?;
    parse_servers(&value)
}

/// Stdio JSON-RPC MCP client. Lines on stdout are newline-delimited JSON.
pub struct StdioMcpClient {
    name: String,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl fmt::Debug for StdioMcpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StdioMcpClient")
            .field("name", &self.name)
            .field("next_id", &self.next_id)
            .finish_non_exhaustive()
    }
}

impl Drop for StdioMcpClient {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

impl StdioMcpClient {
    pub async fn connect(server: &McpServer) -> Result<Self> {
        let (command, args, env) = match &server.transport {
            McpTransport::Stdio { command, args, env } => (command, args, env),
            McpTransport::Http { .. } => bail!("MCP server '{}' is not stdio", server.name),
        };
        let mut cmd = TokioCommand::new(command);
        cmd.args(args).envs(env.clone());
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Drained to /dev/null so a chatty server cannot deadlock a request.
            .stderr(Stdio::null());
        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn MCP server '{}'", server.name))?;
        let stdin = child.stdin.take().context("MCP stdin was not piped")?;
        let stdout = child.stdout.take().context("MCP stdout was not piped")?;
        let mut client = Self {
            name: server.name.clone(),
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        };
        client.initialize().await?;
        Ok(client)
    }

    async fn initialize(&mut self) -> Result<()> {
        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "ta", "version": "0.1.0" }
        });
        let _ = self.request("initialize", params).await?;
        self.notify("notifications/initialized", Value::Object(JsonMap::new()))
            .await?;
        Ok(())
    }

    async fn write_message(&mut self, message: &Value) -> Result<()> {
        let mut line = serde_json::to_string(message)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.write_message(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
        .await
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.write_message(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .await?;
        let response = tokio::time::timeout(REQUEST_TIMEOUT, self.read_response(id))
            .await
            .with_context(|| format!("MCP server '{}' timed out on {}", self.name, method))??;
        Ok(response)
    }

    async fn read_response(&mut self, id: u64) -> Result<Value> {
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).await?;
            if read == 0 {
                bail!("MCP server '{}' closed stdout", self.name);
            }
            let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
                continue; // Ignore non-JSON noise on stdout.
            };
            // Skip server notifications and responses to other requests.
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                bail!("MCP server '{}' returned error: {}", self.name, error);
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    pub async fn list_tools(&mut self) -> Result<Vec<Value>> {
        let result = self
            .request("tools/list", Value::Object(JsonMap::new()))
            .await?;
        Ok(result
            .get("tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    pub async fn call_tool(&mut self, name: &str, args: Value) -> Result<String> {
        let result = self
            .request(
                "tools/call",
                serde_json::json!({ "name": name, "arguments": args }),
            )
            .await?;
        tool_text(&result)
    }
}

/// Best-effort HTTP/SSE MCP client: one JSON-RPC request per POST, parsing
/// either a JSON body or the first `data:` frame of an SSE response.
pub struct HttpMcpClient {
    name: String,
    url: String,
    headers: HeaderMap,
    client: reqwest::Client,
    next_id: u64,
}

impl fmt::Debug for HttpMcpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Header values may carry auth tokens; show names only.
        f.debug_struct("HttpMcpClient")
            .field("name", &self.name)
            .field("url", &self.url)
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl HttpMcpClient {
    pub async fn connect(server: &McpServer) -> Result<Self> {
        let (url, headers) = match &server.transport {
            McpTransport::Http { url, headers } => (url, headers),
            McpTransport::Stdio { .. } => bail!("MCP server '{}' is not HTTP", server.name),
        };
        let mut header_map = HeaderMap::new();
        for (key, value) in headers {
            let name = HeaderName::from_bytes(key.as_bytes())
                .with_context(|| format!("invalid MCP header name '{}'", key))?;
            let value = HeaderValue::from_str(value)
                .with_context(|| format!("invalid MCP header value for '{}'", key))?;
            header_map.insert(name, value);
        }
        Ok(Self {
            name: server.name.clone(),
            url: url.clone(),
            headers: header_map,
            client: reqwest::Client::new(),
            next_id: 1,
        })
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        // TODO(verify): streamable HTTP sessions, server-initiated SSE streams
        // and OAuth are not implemented here. This path sends a single
        // request and reads one response frame; use stdio for full MCP support.
        let id = self.next_id;
        self.next_id += 1;
        let message = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let mut builder = self
            .client
            .post(&self.url)
            .header("accept", "application/json, text/event-stream");
        for (name, value) in self.headers.iter() {
            builder = builder.header(name.clone(), value.clone());
        }
        let response = tokio::time::timeout(REQUEST_TIMEOUT, builder.json(&message).send())
            .await
            .with_context(|| format!("MCP HTTP server '{}' timed out", self.name))??;
        if !response.status().is_success() {
            bail!(
                "MCP HTTP server '{}' returned {}",
                self.name,
                response.status()
            );
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let body = response.text().await?;
        let parsed = if content_type.contains("event-stream") {
            parse_sse_frame(&body)?
        } else {
            serde_json::from_str::<Value>(&body)
                .with_context(|| format!("invalid JSON from MCP server '{}'", self.name))?
        };
        if let Some(error) = parsed.get("error") {
            bail!("MCP server '{}' returned error: {}", self.name, error);
        }
        Ok(parsed.get("result").cloned().unwrap_or(Value::Null))
    }

    pub async fn list_tools(&mut self) -> Result<Vec<Value>> {
        let result = self
            .request("tools/list", Value::Object(JsonMap::new()))
            .await?;
        Ok(result
            .get("tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    pub async fn call_tool(&mut self, name: &str, args: Value) -> Result<String> {
        let result = self
            .request(
                "tools/call",
                serde_json::json!({ "name": name, "arguments": args }),
            )
            .await?;
        tool_text(&result)
    }
}

/// Both stdio and HTTP clients expose the same operations.
pub enum McpClient {
    Stdio(StdioMcpClient),
    Http(HttpMcpClient),
}

impl fmt::Debug for McpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stdio(inner) => f.debug_tuple("Stdio").field(inner).finish(),
            Self::Http(inner) => f.debug_tuple("Http").field(inner).finish(),
        }
    }
}

impl McpClient {
    pub async fn connect(server: &McpServer) -> Result<Self> {
        if !server.enabled {
            bail!("MCP server '{}' is disabled", server.name);
        }
        match &server.transport {
            McpTransport::Stdio { command, .. } if command.is_empty() => {
                bail!("MCP server '{}' has an empty command", server.name)
            }
            McpTransport::Stdio { .. } => Ok(Self::Stdio(StdioMcpClient::connect(server).await?)),
            McpTransport::Http { url, .. } if url.is_empty() => {
                bail!("MCP server '{}' has an empty URL", server.name)
            }
            McpTransport::Http { .. } => Ok(Self::Http(HttpMcpClient::connect(server).await?)),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Stdio(inner) => &inner.name,
            Self::Http(inner) => &inner.name,
        }
    }

    pub async fn list_tools(&mut self) -> Result<Vec<Value>> {
        match self {
            Self::Stdio(inner) => inner.list_tools().await,
            Self::Http(inner) => inner.list_tools().await,
        }
    }

    pub async fn call_tool(&mut self, name: &str, args: Value) -> Result<String> {
        match self {
            Self::Stdio(inner) => inner.call_tool(name, args).await,
            Self::Http(inner) => inner.call_tool(name, args).await,
        }
    }
}

fn parse_sse_frame(body: &str) -> Result<Value> {
    for line in body.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        if let Ok(value) = serde_json::from_str::<Value>(data.trim()) {
            if value.is_object() {
                return Ok(value);
            }
        }
    }
    bail!("MCP SSE response contained no JSON data frame")
}

fn tool_text(result: &Value) -> Result<String> {
    let mut texts = Vec::new();
    if let Some(content) = result.get("content").and_then(Value::as_array) {
        for item in content {
            if let Some(text) = item
                .get("text")
                .or_else(|| item.get("data"))
                .and_then(Value::as_str)
            {
                texts.push(text.to_string());
            }
        }
    }
    let text = if texts.is_empty() {
        result.to_string()
    } else {
        texts.join("\n")
    };
    if result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        bail!("MCP tool error: {}", text);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mcp_json_config() {
        let root = std::env::temp_dir().join(format!("ta-mcp-test-{}", std::process::id()));
        fs_create(&root);
        let path = root.join(".mcp.json");
        std::fs::write(
            &path,
            r#"{"mcpServers":{
                "local":{"command":"cat","args":["a","b"],"env":{"TOKEN":7}},
                "remote":{"type":"http","url":"https://example.com/mcp","headers":{"Authorization":"SECRETVALUE123"},"enabled":false},
                "off":false
            }}"#,
        )
        .unwrap();
        let servers = load_mcp_file(&path).unwrap();
        assert_eq!(servers.len(), 2);
        let local = servers.iter().find(|s| s.name == "local").unwrap();
        match &local.transport {
            McpTransport::Stdio { command, args, env } => {
                assert_eq!(command, "cat");
                assert_eq!(args, &vec!["a".to_string(), "b".to_string()]);
                assert_eq!(env.get("TOKEN").map(String::as_str), Some("7"));
            }
            _ => panic!("expected stdio transport"),
        }
        assert!(local.enabled);
        let remote = servers.iter().find(|s| s.name == "remote").unwrap();
        match &remote.transport {
            McpTransport::Http { url, headers } => {
                assert_eq!(url, "https://example.com/mcp");
                assert!(headers.contains_key("Authorization"));
            }
            _ => panic!("expected http transport"),
        }
        assert!(!remote.enabled);
        // Debug output must never include env or header values.
        let debug = format!("{:?}", remote);
        assert!(!debug.contains("SECRETVALUE123"));
        std::fs::remove_dir_all(root).unwrap();
    }

    fn fs_create(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
    }
}
