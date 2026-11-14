//! Guardrails: run agent/user-written checks against an edit before it lands.
//! Policy lives in data (check specs on knowledge nodes), not in this code.

use crate::db::Db;
use crate::knowledge::short;
use anyhow::Result;
use rusqlite::params;
use serde_json::Value;

#[derive(Debug, Default)]
pub struct GuardResult {
    pub block: Option<String>,
    pub warnings: Vec<String>,
}

pub struct Rule {
    pub id: String,
    pub content: String,
    pub trust: String,
    pub strict: bool,
    pub spec: Value,
}

pub fn rules(db: &Db) -> Result<Vec<Rule>> {
    let mut st = db.conn.prepare(
        "SELECT id, content, trust, strict, check_spec FROM nodes
         WHERE check_spec IS NOT NULL AND superseded_by IS NULL AND trust IN ('user','verified','asserted')
         ORDER BY id",
    )?;
    let rows = st
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?, r.get::<_, String>(4)?))
        })?
        .filter_map(|x| x.ok())
        .filter_map(|(id, content, trust, strict, spec)| {
            serde_json::from_str(&spec).ok().map(|spec| Rule { id, content, trust, strict: strict == 1, spec })
        })
        .collect();
    Ok(rows)
}

/// `added` = text the edit introduces; `full` = the file content after the edit.
pub fn check_edit(db: &Db, rel_path: &str, added: &str, full: &str) -> Result<GuardResult> {
    let mut res = GuardResult::default();
    for r in rules(db)? {
        if let Some(g) = r.spec.get("paths").and_then(|v| v.as_str()) {
            let Ok(glob) = globset::Glob::new(g) else { continue };
            if !glob.compile_matcher().is_match(rel_path) {
                continue;
            }
        }
        let msg_extra = r.spec.get("message").and_then(|v| v.as_str()).unwrap_or("");
        let mut violated = None;
        if let Some(re) = r.spec.get("forbid").and_then(|v| v.as_str())
            && let Ok(re) = regex::Regex::new(re)
                && let Some(m) = re.find(added) {
                    violated = Some(format!("adds `{}`", short(m.as_str(), 60)));
                }
        if violated.is_none()
            && let Some(re) = r.spec.get("require").and_then(|v| v.as_str())
                && let Ok(re) = regex::Regex::new(re)
                    && !full.is_empty() && !re.is_match(full) {
                        violated = Some(format!("result no longer contains /{}/", short(re.as_str(), 60)));
                    }
        if let Some(v) = violated {
            let text = format!(
                "{} ({}) — this edit {} in {}. {}",
                r.id,
                short(&r.content, 200),
                v,
                rel_path,
                msg_extra
            );
            if r.strict && r.trust == "user" {
                res.block = Some(format!(
                    "Blocked by Anchor rule set by the user: {text} Change the edit to respect it, or ask the user to change the rule."
                ));
            } else {
                res.warnings.push(text);
            }
        }
    }
    Ok(res)
}

/// Build the post-edit text for Claude Code Edit / MultiEdit / Write tool inputs.
/// Returns (added_text, full_text_after).
pub fn simulate_edit(tool: &str, input: &Value, abs_path: &std::path::Path) -> (String, String) {
    let old = std::fs::read_to_string(abs_path).unwrap_or_default();
    match tool {
        "Write" => {
            let content = input.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let old_lines: std::collections::HashSet<&str> = old.lines().collect();
            let added: String = content
                .lines()
                .filter(|l| !old_lines.contains(l))
                .collect::<Vec<_>>()
                .join("\n");
            (added, content)
        }
        "Edit" => {
            let o = input.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
            let n = input.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
            let all = input.get("replace_all").and_then(|v| v.as_bool()).unwrap_or(false);
            let full = if o.is_empty() { old.clone() } else if all { old.replace(o, n) } else { old.replacen(o, n, 1) };
            (added_lines(o, n), full)
        }
        "MultiEdit" => {
            let mut full = old.clone();
            let mut added = String::new();
            if let Some(edits) = input.get("edits").and_then(|v| v.as_array()) {
                for e in edits {
                    let o = e.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
                    let n = e.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
                    let all = e.get("replace_all").and_then(|v| v.as_bool()).unwrap_or(false);
                    if !o.is_empty() {
                        full = if all { full.replace(o, n) } else { full.replacen(o, n, 1) };
                    }
                    added.push_str(&added_lines(o, n));
                    added.push('\n');
                }
            }
            (added, full)
        }
        _ => (String::new(), String::new()),
    }
}

fn added_lines(old: &str, new: &str) -> String {
    let olds: std::collections::HashSet<&str> = old.lines().collect();
    new.lines().filter(|l| !olds.contains(l)).collect::<Vec<_>>().join("\n")
}

pub fn knowledge_ids_for_path(db: &Db, rel: &str) -> Result<Vec<String>> {
    let mut st = db.conn.prepare(
        "SELECT DISTINCT e.src FROM edges e JOIN nodes t ON t.id=e.dst
         WHERE e.type='about' AND t.path=?1",
    )?;
    let v = st.query_map(params![rel], |r| r.get(0))?.collect::<Result<_, _>>()?;
    Ok(v)
}
