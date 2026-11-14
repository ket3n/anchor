//! Knowledge written by the agent or user: remember / review, dedupe, conflicts, trust.

use crate::db::{Db, now};
use crate::text;
use anyhow::{Result, anyhow, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct RememberInput {
    pub content: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub about: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub supersedes: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub strict: bool,
    #[serde(default)]
    pub check: Option<Value>,
    /// user | agent | external
    #[serde(default)]
    pub origin: Option<String>,
    #[serde(default)]
    pub session: Option<String>,
}

/// Resolve an `about` reference to concrete node ids + their current hash.
pub fn resolve_about(db: &Db, target: &str) -> Result<Vec<(String, Option<String>)>> {
    let t = target.trim().trim_start_matches("./");
    if t.is_empty() {
        return Ok(vec![]);
    }
    let one = |id: &str| -> Result<Option<(String, Option<String>)>> {
        Ok(db
            .conn
            .query_row("SELECT id, ast_hash FROM nodes WHERE id=?1", params![id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?)
    };
    for prefix in ["", "sym:", "file:", "doc:", "cfg:"] {
        if let Some(x) = one(&format!("{prefix}{t}"))? {
            return Ok(vec![x]);
        }
    }
    // path::Qualified or Qualified or short name
    let mut st = db.conn.prepare(
        "SELECT id, ast_hash FROM nodes WHERE kind='symbol' AND
           (symbol=?1 OR short_name=?1 OR symbol LIKE '%::' || ?1 OR (path || '::' || symbol)=?1)
         ORDER BY length(id) LIMIT 3",
    )?;
    let v: Vec<(String, Option<String>)> =
        st.query_map(params![t], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    if !v.is_empty() {
        return Ok(v);
    }
    // a directory prefix → its file nodes (cap)
    let mut st = db.conn.prepare(
        "SELECT id, ast_hash FROM nodes WHERE kind='file' AND path LIKE ?1 || '%' ORDER BY path LIMIT 20",
    )?;
    let v: Vec<(String, Option<String>)> =
        st.query_map(params![t.trim_end_matches('/')], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    Ok(v)
}

fn validate_check(check: &Value) -> Result<()> {
    let obj = check.as_object().ok_or_else(|| anyhow!("check must be an object"))?;
    let has_forbid = obj.get("forbid").and_then(|v| v.as_str());
    let has_require = obj.get("require").and_then(|v| v.as_str());
    if has_forbid.is_none() && has_require.is_none() {
        bail!("check needs `forbid` or `require` (a regex)");
    }
    for re in [has_forbid, has_require].into_iter().flatten() {
        regex::Regex::new(re).map_err(|e| anyhow!("invalid regex in check: {e}"))?;
    }
    if let Some(g) = obj.get("paths").and_then(|v| v.as_str()) {
        globset::Glob::new(g).map_err(|e| anyhow!("invalid paths glob: {e}"))?;
    }
    Ok(())
}

pub fn remember(db: &Db, inp: RememberInput) -> Result<Value> {
    let content = inp.content.trim().to_string();
    if content.is_empty() {
        bail!("content is empty");
    }
    let kind = inp.kind.clone().unwrap_or_else(|| "note".into()).to_lowercase();
    let origin = inp.origin.clone().unwrap_or_else(|| "agent".into()).to_lowercase();
    let origin = match origin.as_str() {
        "user" | "agent" | "external" => origin,
        _ => "agent".into(),
    };
    let trust = match origin.as_str() {
        "user" => "user",
        "external" => "quarantined",
        _ => "asserted",
    };
    let mut warnings: Vec<String> = Vec::new();
    let strict = if inp.strict && origin != "user" {
        warnings.push("strict ignored: only rules the user set (origin=user) can block edits".into());
        false
    } else {
        inp.strict
    };
    if let Some(c) = &inp.check {
        validate_check(c)?;
    }
    let norm: String = content.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let id = format!("k:{}", &blake3::hash(format!("{kind}\x00{norm}").as_bytes()).to_hex()[..10]);
    let t = now();
    let tags = inp.tags.join(" ");

    let exists: bool = db
        .conn
        .query_row("SELECT 1 FROM nodes WHERE id=?1", params![id], |_| Ok(true))
        .optional()?
        .unwrap_or(false);

    let tx = db.conn.unchecked_transaction()?;
    if !exists {
        let sh = text::simhash(&content) as i64;
        db.conn.execute(
            "INSERT INTO nodes(id,kind,format,content,summary,tags,trust,origin,strict,check_spec,simhash,session,created_at,updated_at)
             VALUES(?1,?2,'text',?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?12)",
            params![id, kind, content, inp.summary, tags, trust, origin, strict as i64,
                inp.check.as_ref().map(|c| c.to_string()), sh, inp.session, t],
        )?;
    } else {
        // exact duplicate: merge tags, keep the higher trust
        db.conn.execute(
            "UPDATE nodes SET tags = trim(tags || ' ' || ?2), updated_at=?3,
               trust = CASE WHEN ?4='user' THEN 'user' ELSE trust END,
               strict = MAX(strict, ?5)
             WHERE id=?1",
            params![id, tags, t, trust, strict as i64],
        )?;
    }
    let fts_body = format!("{}\n{}\n{}\n{}", kind, content, inp.summary.clone().unwrap_or_default(), tags);
    db.fts_put(&id, &fts_body)?;

    let mut linked = Vec::new();
    let mut unresolved = Vec::new();
    for a in &inp.about {
        if a.starts_with("k:") {
            db.conn.execute(
                "INSERT OR IGNORE INTO edges(src,dst,type,weight) VALUES(?1,?2,'depends_on',0.8)",
                params![id, a],
            )?;
            linked.push(a.clone());
            continue;
        }
        let targets = resolve_about(db, a)?;
        if targets.is_empty() {
            unresolved.push(a.clone());
        }
        for (tid, h) in targets {
            db.conn.execute(
                "INSERT INTO edges(src,dst,type,weight,anchor_hash) VALUES(?1,?2,'about',1.0,?3)
                 ON CONFLICT(src,dst,type) DO UPDATE SET anchor_hash=excluded.anchor_hash",
                params![id, tid, h],
            )?;
            linked.push(tid);
        }
    }
    for d in &inp.depends_on {
        db.conn.execute(
            "INSERT OR IGNORE INTO edges(src,dst,type,weight) VALUES(?1,?2,'depends_on',0.8)",
            params![id, d],
        )?;
    }
    if let Some(old) = &inp.supersedes {
        let n = db.conn.execute(
            "UPDATE nodes SET superseded_by=?2, updated_at=?3 WHERE id=?1 AND id != ?2",
            params![old, id, t],
        )?;
        if n == 0 {
            warnings.push(format!("supersedes: {old} not found"));
        } else {
            db.conn.execute(
                "INSERT OR IGNORE INTO edges(src,dst,type,weight) VALUES(?1,?2,'supersedes',0.3)",
                params![id, old],
            )?;
        }
    }
    if let Some(s) = &inp.session {
        db.bump_session(s, "remembers")?;
    }

    // near-duplicates and possible conflicts → tasks for the agent (it decides)
    let mut tasks = Vec::new();
    let sh = text::simhash(&content);
    {
        // only compare against the top lexical matches (keeps writes O(log n), not O(n))
        let terms = text::query_terms(&content);
        let q = terms.iter().take(24).map(|t| format!("\"{}\"", t.replace('"', ""))).collect::<Vec<_>>().join(" OR ");
        let mut rows: Vec<(String, String, i64)> = Vec::new();
        if !q.is_empty() {
            let mut st = db.conn.prepare(
                "SELECT n.id, n.content, n.simhash FROM fts_words f JOIN nodes n ON n.id = f.node_id
                 WHERE fts_words MATCH ?1 AND n.origin != 'code' AND n.superseded_by IS NULL AND n.id != ?2
                 ORDER BY bm25(fts_words) LIMIT 30",
            )?;
            rows = st
                .query_map(params![q, id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .filter_map(|x| x.ok())
                .collect();
        }
        for (oid, oc, osh) in rows {
            if text::hamming(sh, osh as u64) <= 8 {
                tasks.push(format!(
                    "Possible duplicate of {oid} (\"{}\"). If the same, call review(id=\"{id}\", verdict=\"merge\", into=\"{oid}\").",
                    short(&oc, 80)
                ));
            }
        }
    }
    if matches!(kind.as_str(), "decision" | "rule" | "fact" | "preference") && !linked.is_empty() {
        let mut st = db.conn.prepare(
            "SELECT DISTINCT k.id, k.content FROM edges e JOIN nodes k ON k.id=e.src
             WHERE e.type='about' AND e.dst IN (SELECT dst FROM edges WHERE src=?1 AND type='about')
               AND k.id != ?1 AND k.superseded_by IS NULL AND k.kind=?2 LIMIT 3",
        )?;
        let rows: Vec<(String, String)> =
            st.query_map(params![id, kind], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        for (oid, oc) in rows {
            tasks.push(format!(
                "Related {kind} {oid} on the same code: \"{}\". If they conflict, keep the right one: remember(..., supersedes=\"{oid}\") or review(id, verdict=\"broken\").",
                short(&oc, 80)
            ));
        }
    }
    tx.commit()?;
    Ok(json!({
        "id": id,
        "status": if exists { "merged_with_existing" } else { "stored" },
        "trust": trust,
        "strict": strict,
        "linked": linked,
        "unresolved_about": unresolved,
        "warnings": warnings,
        "tasks": tasks,
    }))
}

pub fn short(s: &str, n: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= n {
        one
    } else {
        format!("{}…", one.chars().take(n).collect::<String>())
    }
}

pub fn review(db: &Db, id: &str, verdict: &str, into: Option<&str>, note: Option<&str>) -> Result<Value> {
    let exists: Option<(String, String)> = db
        .conn
        .query_row("SELECT trust, origin FROM nodes WHERE id=?1", params![id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let Some((trust, origin)) = exists else { bail!("unknown id {id}") };
    if origin == "code" && !matches!(verdict, "used" | "ignored") {
        bail!("{id} is indexed code; review applies to knowledge items");
    }
    let t = now();
    match verdict {
        "verified" => {
            let new_trust = if trust == "user" { "user" } else if trust == "quarantined" { "quarantined" } else { "verified" };
            db.conn.execute("UPDATE nodes SET trust=?2, stale=0, updated_at=?3 WHERE id=?1", params![id, new_trust, t])?;
            // re-anchor to the current code
            db.conn.execute(
                "UPDATE edges SET anchor_hash = (SELECT ast_hash FROM nodes WHERE nodes.id = edges.dst)
                 WHERE src=?1 AND type='about'",
                params![id],
            )?;
        }
        "approve" => {
            db.conn.execute(
                "UPDATE nodes SET trust=CASE WHEN origin='user' THEN 'user' ELSE 'verified' END, stale=0, updated_at=?2 WHERE id=?1",
                params![id, t],
            )?;
            db.conn.execute(
                "UPDATE edges SET anchor_hash = (SELECT ast_hash FROM nodes WHERE nodes.id = edges.dst)
                 WHERE src=?1 AND type='about'",
                params![id],
            )?;
        }
        "broken" | "disputed" => {
            db.conn.execute("UPDATE nodes SET trust='disputed', updated_at=?2 WHERE id=?1", params![id, t])?;
        }
        "updated" => {
            bail!("to update content call remember(content=<new text>, supersedes=\"{id}\")");
        }
        "merge" => {
            let into = into.ok_or_else(|| anyhow!("merge needs `into`"))?;
            db.conn.execute("UPDATE nodes SET superseded_by=?2, updated_at=?3 WHERE id=?1", params![id, into, t])?;
            db.conn.execute(
                "INSERT OR IGNORE INTO edges(src,dst,type,weight,anchor_hash)
                 SELECT ?2, dst, type, weight, anchor_hash FROM edges WHERE src=?1 AND type IN ('about','depends_on')",
                params![id, into],
            )?;
        }
        "forget" => {
            db.conn.execute("UPDATE nodes SET superseded_by='archived', updated_at=?2 WHERE id=?1", params![id, t])?;
        }
        "used" => {
            db.conn.execute("UPDATE nodes SET used=used+1 WHERE id=?1", params![id])?;
        }
        "ignored" => {
            db.conn.execute("UPDATE nodes SET ignored=ignored+1 WHERE id=?1", params![id])?;
        }
        other => bail!("unknown verdict {other}; use verified|approve|broken|merge|forget|used|ignored"),
    }
    if let Some(n) = note
        && !n.trim().is_empty() {
            db.conn.execute(
                "UPDATE nodes SET summary = trim(COALESCE(summary,'') || ' [review: ' || ?2 || ']') WHERE id=?1",
                params![id, n],
            )?;
        }
    let (trust, stale, sup): (String, i64, Option<String>) = db.conn.query_row(
        "SELECT trust, stale, superseded_by FROM nodes WHERE id=?1",
        params![id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    Ok(json!({"id": id, "trust": trust, "stale": stale, "superseded_by": sup}))
}

pub fn health(db: &Db) -> Result<Value> {
    let q = |sql: &str| -> Result<i64> { Ok(db.conn.query_row(sql, [], |r| r.get(0))?) };
    Ok(json!({
        "knowledge": q("SELECT COUNT(*) FROM nodes WHERE origin!='code' AND superseded_by IS NULL")?,
        "user_rules": q("SELECT COUNT(*) FROM nodes WHERE origin!='code' AND trust='user' AND superseded_by IS NULL")?,
        "verified": q("SELECT COUNT(*) FROM nodes WHERE origin!='code' AND trust='verified' AND superseded_by IS NULL")?,
        "asserted": q("SELECT COUNT(*) FROM nodes WHERE origin!='code' AND trust='asserted' AND superseded_by IS NULL")?,
        "stale": q("SELECT COUNT(*) FROM nodes WHERE origin!='code' AND stale=1 AND superseded_by IS NULL")?,
        "neighbour_changed": q("SELECT COUNT(*) FROM nodes WHERE origin!='code' AND stale=2 AND superseded_by IS NULL")?,
        "disputed": q("SELECT COUNT(*) FROM nodes WHERE origin!='code' AND trust='disputed' AND superseded_by IS NULL")?,
        "quarantined": q("SELECT COUNT(*) FROM nodes WHERE trust='quarantined' AND superseded_by IS NULL")?,
        "symbols": q("SELECT COUNT(*) FROM nodes WHERE kind='symbol'")?,
        "doc_sections": q("SELECT COUNT(*) FROM nodes WHERE kind='doc_section'")?,
        "files": q("SELECT COUNT(*) FROM files")?,
        "edges": q("SELECT COUNT(*) FROM edges")?,
    }))
}

pub fn list(db: &Db, include_superseded: bool) -> Result<Vec<Value>> {
    let sql = if include_superseded {
        "SELECT id, kind, trust, stale, strict, content, superseded_by FROM nodes WHERE origin!='code' ORDER BY created_at"
    } else {
        "SELECT id, kind, trust, stale, strict, content, superseded_by FROM nodes WHERE origin!='code' AND superseded_by IS NULL ORDER BY created_at"
    };
    let mut st = db.conn.prepare(sql)?;
    let rows = st
        .query_map([], |r| {
            Ok(json!({
                "id": r.get::<_, String>(0)?, "kind": r.get::<_, String>(1)?, "trust": r.get::<_, String>(2)?,
                "stale": r.get::<_, i64>(3)?, "strict": r.get::<_, i64>(4)? == 1, "content": r.get::<_, String>(5)?,
                "superseded_by": r.get::<_, Option<String>>(6)?,
            }))
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}
