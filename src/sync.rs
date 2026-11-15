//! Text copy of the knowledge base that can be committed: `.anchor/knowledge.jsonl`.
//!
//! One JSON object per line, sorted by id, so diffs are reviewable and merges are line-based. Ids are content
//! hashes, so the same item has the same id on every machine. Code anchors are not exported: each machine
//! re-anchors items to its own checkout, and staleness is recomputed locally.
//! Enabled by `anchor sync --enable` (which also lets git track the file); while the file exists, every
//! remember/review rewrites it and each session start imports changes that arrived with a pull.

use crate::db::{Db, now};
use crate::{knowledge, text};
use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::path::PathBuf;

pub const FILE: &str = "knowledge.jsonl";
const GITIGNORE: &str = "# Anchor: the database and logs stay local; knowledge.jsonl is shared through git\n*\n!.gitignore\n!knowledge.jsonl\n";

pub fn path(db: &Db) -> PathBuf {
    db.root.join(".anchor").join(FILE)
}

pub fn enabled(db: &Db) -> bool {
    path(db).exists()
}

/// Turn sharing on: let git track knowledge.jsonl and write it.
pub fn enable(db: &Db) -> Result<usize> {
    std::fs::write(db.root.join(".anchor").join(".gitignore"), GITIGNORE)?;
    if path(db).exists() {
        import(db)?;
    }
    export(db)
}

fn records(db: &Db) -> Result<Vec<Value>> {
    let mut st = db.conn.prepare(
        "SELECT id, kind, content, summary, tags, trust, origin, strict, check_spec, superseded_by, created_at, updated_at
         FROM nodes WHERE origin != 'code' ORDER BY id",
    )?;
    let rows: Vec<Value> = st
        .query_map([], |r| {
            Ok(json!({
                "id": r.get::<_, String>(0)?, "kind": r.get::<_, String>(1)?, "content": r.get::<_, String>(2)?,
                "summary": r.get::<_, Option<String>>(3)?, "tags": r.get::<_, String>(4)?,
                "trust": r.get::<_, String>(5)?, "origin": r.get::<_, String>(6)?, "strict": r.get::<_, i64>(7)? != 0,
                "check": r.get::<_, Option<String>>(8)?, "superseded_by": r.get::<_, Option<String>>(9)?,
                "created_at": r.get::<_, i64>(10)?, "updated_at": r.get::<_, i64>(11)?,
            }))
        })?
        .collect::<Result<_, _>>()?;
    let mut out = vec![];
    for mut v in rows {
        let id = v["id"].as_str().unwrap_or_default().to_string();
        for (field, ty) in [("about", "about"), ("depends_on", "depends_on")] {
            let mut e = db.conn.prepare("SELECT dst FROM edges WHERE src=?1 AND type=?2 ORDER BY dst")?;
            let dst: Vec<String> = e.query_map(params![id, ty], |r| r.get(0))?.collect::<Result<_, _>>()?;
            v[field] = json!(dst);
        }
        out.push(v);
    }
    Ok(out)
}

/// Write knowledge.jsonl (atomically). Returns the number of items.
pub fn export(db: &Db) -> Result<usize> {
    let recs = records(db)?;
    let body: String = recs.iter().map(|r| format!("{r}\n")).collect();
    let p = path(db);
    let tmp = p.with_extension("jsonl.tmp");
    std::fs::write(&tmp, &body)?;
    std::fs::rename(&tmp, &p)?;
    db.meta_set("sync_hash", &blake3::hash(body.as_bytes()).to_hex())?;
    Ok(recs.len())
}

#[derive(Debug, Default, serde::Serialize)]
pub struct ImportReport {
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
}

/// Merge knowledge.jsonl into the store: new items are added; an item present on both sides takes the
/// version with the later `updated_at`. Nothing is deleted (a forgotten item is kept as superseded/archived).
pub fn import(db: &Db) -> Result<ImportReport> {
    let mut rep = ImportReport::default();
    let Ok(body) = std::fs::read_to_string(path(db)) else { return Ok(rep) };
    for line in body.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(r) = serde_json::from_str::<Value>(line) else { continue };
        let Some(id) = r["id"].as_str() else { continue };
        let theirs = r["updated_at"].as_i64().unwrap_or(0);
        let ours: Option<i64> = db
            .conn
            .query_row("SELECT updated_at FROM nodes WHERE id=?1", params![id], |x| x.get(0))
            .optional()?;
        match ours {
            Some(t) if t >= theirs => {
                rep.unchanged += 1;
                continue;
            }
            Some(_) => rep.updated += 1,
            None => rep.added += 1,
        }
        let s = |k: &str| r[k].as_str().map(String::from);
        let content = s("content").unwrap_or_default();
        let kind = s("kind").unwrap_or_else(|| "note".into());
        db.conn.execute(
            "INSERT INTO nodes(id,kind,format,content,summary,tags,trust,origin,strict,check_spec,superseded_by,simhash,created_at,updated_at)
             VALUES(?1,?2,'text',?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
             ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, content=excluded.content, summary=excluded.summary,
               tags=excluded.tags, trust=excluded.trust, origin=excluded.origin, strict=excluded.strict,
               check_spec=excluded.check_spec, superseded_by=excluded.superseded_by, updated_at=excluded.updated_at",
            params![
                id, kind, content, s("summary"), s("tags").unwrap_or_default(), s("trust").unwrap_or_else(|| "asserted".into()),
                s("origin").unwrap_or_else(|| "agent".into()), r["strict"].as_bool().unwrap_or(false) as i64, s("check"),
                s("superseded_by"), text::simhash(&content) as i64, r["created_at"].as_i64().unwrap_or_else(now), theirs
            ],
        )?;
        db.fts_put(id, &format!("{}\n{}\n{}\n{}", kind, content, s("summary").unwrap_or_default(), s("tags").unwrap_or_default()))?;
        db.conn.execute("DELETE FROM edges WHERE src=?1 AND type IN ('about','depends_on')", params![id])?;
        for (field, ty, w) in [("about", "about", 1.0), ("depends_on", "depends_on", 0.8)] {
            for dst in r[field].as_array().into_iter().flatten().filter_map(|d| d.as_str()) {
                // anchored to this checkout's code; a target not indexed yet gets its anchor at the next index
                let anchor = if ty == "about" { knowledge::anchor_value(db, dst, &content, &kind)? } else { None };
                db.conn.execute(
                    "INSERT OR IGNORE INTO edges(src,dst,type,weight,anchor_hash) VALUES(?1,?2,?3,?4,?5)",
                    params![id, dst, ty, w, anchor],
                )?;
            }
        }
    }
    db.meta_set("sync_hash", &blake3::hash(body.as_bytes()).to_hex())?;
    Ok(rep)
}

/// After a write: keep knowledge.jsonl in step (only when sharing is enabled).
pub fn autosave(db: &Db) {
    if enabled(db) {
        let _ = export(db);
    }
}

/// At session start: import if knowledge.jsonl changed since we last wrote or read it (e.g. after a pull).
pub fn autoload(db: &Db) -> Option<ImportReport> {
    if !enabled(db) {
        return None;
    }
    let body = std::fs::read(path(db)).ok()?;
    let h = blake3::hash(&body).to_hex().to_string();
    if db.meta_get("sync_hash").ok().flatten().as_deref() == Some(h.as_str()) {
        return None;
    }
    let rep = import(db).ok()?;
    let _ = export(db); // write back the merged result so both sides converge
    Some(rep)
}
