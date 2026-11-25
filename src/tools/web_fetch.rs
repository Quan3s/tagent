use super::{schema_object, truncate_output, Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use regex::Regex;
use reqwest::Client;
use serde_json::{json, Value};

pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch an HTTP(S) page and return readable text. No proxy or API key is used."
    }

    fn input_schema(&self) -> Value {
        schema_object(
            json!({"url": {"type": "string", "description": "http(s) URL"}}),
            &["url"],
        )
    }

    async fn call(&self, args: &Value, _ctx: &ToolContext) -> Result<ToolOutput> {
        let Some(url) = args.get("url").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("web_fetch: missing 'url'"));
        };
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok(ToolOutput::err("web_fetch: only http(s) URLs are allowed"));
        }
        let client = Client::builder()
            .user_agent("ta/0.1 (+local coding agent)")
            .build()?;
        let response = match client.get(url).send().await {
            Ok(r) => r,
            Err(e) => return Ok(ToolOutput::err(format!("web_fetch: request failed: {e}"))),
        };
        if !response.status().is_success() {
            return Ok(ToolOutput::err(format!(
                "web_fetch: HTTP {}",
                response.status().as_u16()
            )));
        }
        let bytes = match response.bytes().await {
            Ok(b) => b,
            Err(e) => return Ok(ToolOutput::err(format!("web_fetch: read failed: {e}"))),
        };
        if bytes.len() > 2 * 1024 * 1024 {
            return Ok(ToolOutput::err(
                "web_fetch: response too large (2 MiB limit)",
            ));
        }
        let raw = String::from_utf8_lossy(&bytes);
        let text = if raw.contains('<') {
            html_to_text(&raw)
        } else {
            raw.into_owned()
        };
        Ok(ToolOutput::ok(truncate_output(&text, 256 * 1024)))
    }
}

fn html_to_text(html: &str) -> String {
    // TODO(verify): richer HTML extraction (readability) if models need it.
    let mut without_scripts = html.to_string();
    for tag in ["script", "style", "svg"] {
        let pattern = format!(r"(?is)<{tag}[^>]*>.*?</{tag}>");
        if let Ok(re) = Regex::new(&pattern) {
            without_scripts = re.replace_all(&without_scripts, " ").into_owned();
        }
    }
    let without_scripts = without_scripts.as_str();
    let tags = Regex::new(r"(?s)<[^>]+>").unwrap();
    let plain = tags.replace_all(&without_scripts, " ");
    let space = Regex::new(r"[ \t]{2,}").unwrap();
    let lines = Regex::new(r"\n{3,}").unwrap();
    let out = space.replace_all(&plain, " ");
    lines.replace_all(&out, "\n\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_html_noise() {
        let text =
            html_to_text("<html><script>bad()</script><body>Hello <b>world</b></body></html>");
        assert!(text.contains("Hello world"));
        assert!(!text.contains("bad()"));
    }
}
