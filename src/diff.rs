//! Small dependency-free unified diff for edit cards. It is intentionally
//! bounded: large files produce a truncated diff rather than consuming memory.

use std::collections::HashMap;

pub fn unified(path: &str, before: &str, after: &str) -> String {
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();
    if old == new {
        return String::new();
    }
    let (ops, _) = lcs_ops(&old, &new, 600);
    let mut out = format!("--- a/{path}\n+++ b/{path}\n");
    let mut old_no = 1usize;
    let mut new_no = 1usize;
    for op in ops {
        match op {
            Op::Equal(line) => {
                out.push_str(&format!(" {line}\n"));
                old_no += 1;
                new_no += 1;
            }
            Op::Delete(line) => {
                out.push_str(&format!("-{line}\n"));
                old_no += 1;
            }
            Op::Insert(line) => {
                out.push_str(&format!("+{line}\n"));
                new_no += 1;
            }
        }
    }
    let _ = (old_no, new_no);
    if out.len() > 64 * 1024 {
        out.truncate(64 * 1024);
        out.push_str("\n… [diff truncated]\n");
    }
    out
}

#[derive(Clone, Copy)]
enum Op<'a> {
    Equal(&'a str),
    Delete(&'a str),
    Insert(&'a str),
}

/// Classic LCS with a cap on the DP table. Over the cap, fall back to a
/// whole-file replacement so the UI still shows something useful.
fn lcs_ops<'a>(old: &'a [&'a str], new: &'a [&'a str], cap: usize) -> (Vec<Op<'a>>, usize) {
    if old.len().saturating_mul(new.len()) > cap * cap {
        let mut ops = Vec::with_capacity(old.len() + new.len());
        ops.extend(old.iter().map(|line| Op::Delete(line)));
        ops.extend(new.iter().map(|line| Op::Insert(line)));
        return (ops, 0);
    }
    let n = old.len();
    let m = new.len();
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if old[i] == new[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut ops = Vec::new();
    let mut i = 0;
    let mut j = 0;
    while i < n && j < m {
        if old[i] == new[j] {
            ops.push(Op::Equal(old[i]));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            ops.push(Op::Delete(old[i]));
            i += 1;
        } else {
            ops.push(Op::Insert(new[j]));
            j += 1;
        }
    }
    while i < n {
        ops.push(Op::Delete(old[i]));
        i += 1;
    }
    while j < m {
        ops.push(Op::Insert(new[j]));
        j += 1;
    }
    (ops, dp[0][0] as usize)
}

/// Parse a unified diff header into added/removed line counts for diff cards.
pub fn counts(diff: &str) -> HashMap<String, (usize, usize)> {
    let mut out = HashMap::new();
    let mut current: Option<String> = None;
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            current = Some(path.to_string());
            out.entry(path.to_string()).or_insert((0, 0));
            continue;
        }
        let Some(path) = current.as_ref() else {
            continue;
        };
        let entry = out.entry(path.clone()).or_insert((0, 0));
        if line.starts_with('+') && !line.starts_with("+++") {
            entry.0 += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            entry.1 += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_add_and_remove_lines() {
        let diff = unified("a.txt", "one\ntwo\n", "one\nTWO\nthree\n");
        assert!(diff.contains("-two"));
        assert!(diff.contains("+TWO"));
        assert!(diff.contains("+three"));
        let counts = counts(&diff);
        assert_eq!(counts.get("a.txt"), Some(&(2, 1)));
    }
}
