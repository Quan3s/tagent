//! Read-only adapters for instruction files used by other coding agents.
//! Nothing here is executed; files are returned as plain text for context.

use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Instruction and config files recognized from other agents, in stable order.
pub fn discover_instruction_files(workspace: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for file in [
        "AGENTS.md",
        "CLAUDE.md",
        "GEMINI.md",
        ".clinerules",
        ".github/copilot-instructions.md",
        "opencode.json",
        "opencode.jsonc",
        ".opencode.json",
        ".opencode/opencode.json",
        ".config/opencode/opencode.json",
        ".opencode/AGENTS.md",
        ".config/opencode/AGENTS.md",
        "gemini-extension.json",
    ] {
        push_if_present(&mut paths, workspace.join(file));
    }
    for file in collect_with_extension(&workspace.join(".cursor/rules"), "mdc") {
        push_if_present(&mut paths, file);
    }
    for file in collect_with_extension(&workspace.join(".github/prompts"), "md") {
        if file
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.ends_with(".prompt.md"))
            .unwrap_or(false)
        {
            push_if_present(&mut paths, file);
        }
    }
    if workspace.join(".clinerules").is_dir() {
        for file in collect_with_extension(&workspace.join(".clinerules"), "md") {
            push_if_present(&mut paths, file);
        }
    }
    paths
}

fn push_if_present(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_file() && !paths.contains(&path) {
        paths.push(path);
    }
}

fn collect_with_extension(dir: &Path, extension: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| e == extension)
                .unwrap_or(false)
        })
        .collect();
    out.sort();
    out
}

/// Read every recognized instruction file. JSON configs contribute their
/// instruction-like string fields (or the raw file if none are found).
pub fn project_instructions(workspace: &Path) -> Vec<String> {
    discover_instruction_files(workspace)
        .into_iter()
        .filter_map(|path| {
            let text = fs::read_to_string(&path).ok()?;
            if text.trim().is_empty() {
                return None;
            }
            let is_config = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.ends_with(".json") || n.ends_with(".jsonc"))
                .unwrap_or(false);
            if is_config {
                let extracted = extract_instructions(&text);
                if !extracted.is_empty() {
                    return Some(extracted);
                }
            }
            Some(text)
        })
        .collect()
}

fn extract_instructions(text: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return String::new();
    };
    let mut out = Vec::new();
    collect_instruction_strings(&value, &mut out);
    out.join("\n\n")
}

fn collect_instruction_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(obj) => {
            for (key, value) in obj {
                let key = key.to_ascii_lowercase();
                if matches!(
                    key.as_str(),
                    "instructions"
                        | "instruction"
                        | "systemprompt"
                        | "system_prompt"
                        | "prompt"
                        | "context"
                ) {
                    if let Some(text) = value.as_str() {
                        if !text.trim().is_empty() {
                            out.push(text.trim().to_string());
                        }
                        continue;
                    }
                }
                collect_instruction_strings(value, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_instruction_strings(item, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_only_known_files() {
        let root = std::env::temp_dir().join(format!("ta-adapter-test-{}", std::process::id()));
        fs::create_dir_all(root.join(".cursor/rules")).unwrap();
        fs::create_dir_all(root.join(".github/prompts")).unwrap();
        fs::write(root.join("AGENTS.md"), "agents").unwrap();
        fs::write(root.join(".cursor/rules/main.mdc"), "cursor").unwrap();
        fs::write(root.join(".github/prompts/review.prompt.md"), "review").unwrap();
        fs::write(root.join(".github/prompts/ignored.md"), "no").unwrap();
        fs::write(
            root.join("opencode.json"),
            r#"{"systemPrompt":"be concise"}"#,
        )
        .unwrap();
        let files = discover_instruction_files(&root);
        let names: Vec<&str> = files
            .iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()))
            .collect();
        assert!(names.contains(&"AGENTS.md"));
        assert!(names.contains(&"main.mdc"));
        assert!(names.contains(&"review.prompt.md"));
        assert!(!names.contains(&"ignored.md"));
        let instructions = project_instructions(&root).join("\n");
        assert!(instructions.contains("agents"));
        assert!(instructions.contains("be concise"));
        assert!(!instructions.contains("no\n"));
        fs::remove_dir_all(root).unwrap();
    }
}
