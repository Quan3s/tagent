use super::{schema_object, truncate_output, Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use regex::Regex;
use reqwest::Client;
use serde_json::{json, Value};

pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the public web. Uses DuckDuckGo's HTML endpoint; no backend proxy or BYOK key."
    }

    fn input_schema(&self) -> Value {
        schema_object(
            json!({
                "query": {"type": "string"},
                "max_results": {"type": "integer", "default": 5}
            }),
            &["query"],
        )
    }

    async fn call(&self, args: &Value, _ctx: &ToolContext) -> Result<ToolOutput> {
        let Some(query) = args.get("query").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("web_search: missing 'query'"));
        };
        let max = args
            .get("max_results")
            .and_then(Value::as_u64)
            .unwrap_or(5)
            .min(10) as usize;
        // TODO(verify): DDG HTML markup can change; fall back to plain result
        // blocks without breaking the tool contract.
        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            percent_encode(query)
        );
        let client = Client::builder()
            .user_agent("Mozilla/5.0 (compatible; ta/0.1)")
            .build()?;
        let body = match client.get(&url).send().await {
            Ok(r) if r.status().is_success() => r.text().await?,
            Ok(r) => {
                return Ok(ToolOutput::err(format!(
                    "web_search: HTTP {}",
                    r.status().as_u16()
                )))
            }
            Err(e) => return Ok(ToolOutput::err(format!("web_search: request failed: {e}"))),
        };
        let title_re =
            Regex::new(r#"(?s)class="result__a"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#).unwrap();
        let snippet_re =
            Regex::new(r#"(?s)class="result__snippet"[^>]*>(.*?)</(?:a|div)>"#).unwrap();
        let strip = Regex::new(r"<[^>]+>").unwrap();
        let mut out = String::new();
        for (index, caps) in title_re.captures_iter(&body).enumerate() {
            if index >= max {
                break;
            }
            let href = html_unescape(&caps[1]);
            let title = strip.replace_all(&caps[2], "").trim().to_string();
            let snippet = snippet_re
                .captures_iter(&body)
                .nth(index)
                .map(|c| strip.replace_all(&c[1], "").trim().to_string())
                .unwrap_or_default();
            out.push_str(&format!(
                "{}. {}\n   {}\n   {}\n",
                index + 1,
                title,
                href,
                snippet
            ));
        }
        if out.is_empty() {
            return Ok(ToolOutput::ok("(no results)"));
        }
        Ok(ToolOutput::ok(truncate_output(&out, 64 * 1024)))
    }
}

fn percent_encode(input: &str) -> String {
    let mut out = String::new();
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn html_unescape(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_query() {
        assert_eq!(percent_encode("a b&c"), "a%20b%26c");
    }
}
