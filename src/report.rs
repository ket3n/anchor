//! `anchor report`: what Anchor actually did in this project, from `.anchor/events.jsonl` and the store.
//! Meant for judging Anchor in real work: it counts events and never claims an outcome it cannot see.

use crate::db::Db;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};

#[derive(Default)]
struct Tally {
    sessions: HashSet<String>,
    prompts: u64,
    prompt_injections: u64,
    prompt_tokens: u64,
    edit_injections: u64,
    injected_items: HashMap<String, u64>,
    warnings: u64,
    blocks: u64,
    edits: u64,
    newly_stale: u64,
    remembers: u64,
    first_ts: Option<i64>,
    last_ts: Option<i64>,
}

fn read_events(db: &Db, since: i64) -> Tally {
    let mut t = Tally::default();
    let dir = db.root.join(".anchor");
    for name in ["events.1.jsonl", "events.jsonl"] {
        let Ok(f) = std::fs::File::open(dir.join(name)) else { continue };
        for line in BufReader::new(f).lines().map_while(|l| l.ok()) {
            let Ok(e) = serde_json::from_str::<Value>(&line) else { continue };
            let ts = e["ts"].as_i64().unwrap_or(0);
            if ts < since {
                continue;
            }
            t.first_ts = Some(t.first_ts.map_or(ts, |f| f.min(ts)));
            t.last_ts = Some(t.last_ts.map_or(ts, |l| l.max(ts)));
            if let Some(s) = e["session"].as_str() {
                t.sessions.insert(s.to_string());
            }
            let x = &e["extra"];
            let injected: Vec<String> = x["injected"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            for id in &injected {
                *t.injected_items.entry(id.clone()).or_default() += 1;
            }
            match e["event"].as_str().unwrap_or("") {
                "UserPromptSubmit" => {
                    t.prompts += 1;
                    if !injected.is_empty() {
                        t.prompt_injections += 1;
                    }
                    t.prompt_tokens += x["tokens"].as_u64().unwrap_or(0);
                }
                "PreToolUse" => {
                    if !injected.is_empty() {
                        t.edit_injections += 1;
                    }
                    t.warnings += x["warnings"].as_array().map_or(0, |a| a.len() as u64);
                    if x.get("blocked").is_some_and(|b| !b.is_null()) {
                        t.blocks += 1;
                    }
                }
                "PostToolUse" => {
                    if x.get("changed").is_some() {
                        t.edits += 1;
                    }
                    t.newly_stale += match &x["newly_stale"] {
                        Value::Array(a) => a.len() as u64,
                        Value::Number(n) => n.as_u64().unwrap_or(0),
                        _ => 0,
                    };
                }
                "Stop" => t.remembers += x["remembers"].as_u64().unwrap_or(0),
                _ => {}
            }
        }
    }
    t
}

/// Machine-readable report (also what the text view prints from).
pub fn report(db: &Db, days: Option<u32>) -> Result<Value> {
    let since = days.map_or(0, |d| crate::db::now() - i64::from(d) * 86_400);
    let t = read_events(db, since);
    let q = |sql: &str| -> Result<i64> { Ok(db.conn.query_row(sql, [], |r| r.get(0))?) };
    let mut by_kind = serde_json::Map::new();
    {
        let mut st = db.conn.prepare(
            "SELECT kind, COUNT(*) FROM nodes WHERE origin!='code' AND superseded_by IS NULL GROUP BY kind ORDER BY 2 DESC",
        )?;
        for row in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
            let (k, n) = row?;
            by_kind.insert(k, json!(n));
        }
    }
    let mut top: Vec<(&String, &u64)> = t.injected_items.iter().collect();
    top.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let mut top_items = vec![];
    for (id, n) in top.into_iter().take(5) {
        let text: Option<String> = db
            .conn
            .query_row("SELECT substr(content, 1, 90) FROM nodes WHERE id=?1", [id], |r| r.get(0))
            .ok();
        top_items.push(json!({"id": id, "times_shown": n, "text": text}));
    }
    Ok(json!({
        "period": {"days": days, "first_event": t.first_ts, "last_event": t.last_ts},
        "sessions": t.sessions.len(),
        "prompts": t.prompts,
        "context": {
            "prompts_with_injection": t.prompt_injections,
            "edits_with_injection": t.edit_injections,
            "tokens_injected_at_prompts": t.prompt_tokens,
            "avg_tokens_per_prompt": t.prompt_tokens.checked_div(t.prompts).unwrap_or(0),
            "distinct_items_shown": t.injected_items.len(),
            "most_shown": top_items,
        },
        "guardrails": {"warnings": t.warnings, "blocked_edits": t.blocks},
        "staleness": {"edits_seen": t.edits, "items_marked_stale_by_edits": t.newly_stale,
                       "stale_now": q("SELECT COUNT(*) FROM nodes WHERE origin!='code' AND stale>0 AND superseded_by IS NULL")?},
        "knowledge": {
            "active": q("SELECT COUNT(*) FROM nodes WHERE origin!='code' AND superseded_by IS NULL")?,
            "by_kind": by_kind,
            "from_user": q("SELECT COUNT(*) FROM nodes WHERE origin='user' AND superseded_by IS NULL")?,
            "from_agent": q("SELECT COUNT(*) FROM nodes WHERE origin='agent' AND superseded_by IS NULL")?,
            "superseded": q("SELECT COUNT(*) FROM nodes WHERE origin!='code' AND superseded_by IS NOT NULL")?,
            "remembered_in_sessions": t.remembers,
            "marked_used": q("SELECT COALESCE(SUM(used),0) FROM nodes WHERE origin!='code'")?,
            "marked_ignored": q("SELECT COALESCE(SUM(ignored),0) FROM nodes WHERE origin!='code'")?,
        },
    }))
}

/// Human-readable view of [`report`].
pub fn render(r: &Value) -> String {
    let c = &r["context"];
    let k = &r["knowledge"];
    let mut out = String::new();
    let days = r["period"]["days"].as_u64().map_or("all time".to_string(), |d| format!("last {d} days"));
    out += &format!("Anchor report ({days})\n\n");
    out += &format!("Sessions: {}   prompts: {}\n", r["sessions"], r["prompts"]);
    out += &format!(
        "Context shown: at {} of {} prompts and before {} edits; {} distinct items; {} tokens at prompts (avg {} per prompt)\n",
        c["prompts_with_injection"], r["prompts"], c["edits_with_injection"], c["distinct_items_shown"],
        c["tokens_injected_at_prompts"], c["avg_tokens_per_prompt"]
    );
    if let Some(top) = c["most_shown"].as_array().filter(|a| !a.is_empty()) {
        out += "Most shown:\n";
        for t in top {
            out += &format!("  {}x  {}  {}\n", t["times_shown"], t["id"].as_str().unwrap_or(""), t["text"].as_str().unwrap_or("(deleted)"));
        }
    }
    out += &format!(
        "Guardrails: {} warnings, {} edits blocked\n",
        r["guardrails"]["warnings"], r["guardrails"]["blocked_edits"]
    );
    out += &format!(
        "Staleness: {} edits seen, {} items flagged stale by them, {} stale now\n",
        r["staleness"]["edits_seen"], r["staleness"]["items_marked_stale_by_edits"], r["staleness"]["stale_now"]
    );
    out += &format!(
        "Knowledge: {} active ({} from you, {} from the agent), {} superseded by corrections; {} remembered during sessions\n",
        k["active"], k["from_user"], k["from_agent"], k["superseded"], k["remembered_in_sessions"]
    );
    out += &format!("Feedback: {} marked used, {} marked ignored\n", k["marked_used"], k["marked_ignored"]);
    out += "\nThese are counts of what Anchor did, not proof it helped. Compare with sessions run with ANCHOR_DISABLE=1 (see EVALUATING.md).\n";
    out
}
