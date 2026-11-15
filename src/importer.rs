//! `anchor import FILE...`: turn existing instruction files (CLAUDE.md, AGENTS.md, docs, ADRs) into
//! Anchor items. Each bullet or short paragraph becomes one item; its kind is guessed from wording, and it
//! is linked to the files and symbols it mentions that exist in this repository. Items are stored as the
//! user's (origin=user): the user wrote these files. Long prose sections are skipped, not summarized.

use crate::db::Db;
use crate::knowledge::{self, RememberInput};
use crate::text;
use anyhow::Result;
use serde_json::{Value, json};
use std::path::Path;

const MAX_ITEM_CHARS: usize = 600;

/// Split Markdown into candidate items: list items and short paragraphs, with their nearest heading.
pub fn split(md: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    let mut heading = String::new();
    let mut buf: Vec<String> = vec![];
    let mut in_code = false;
    let flush = |buf: &mut Vec<String>, heading: &str, out: &mut Vec<(String, String)>| {
        let t = buf.join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
        if t.chars().count() >= 12 && t.chars().count() <= MAX_ITEM_CHARS {
            out.push((heading.to_string(), t));
        }
        buf.clear();
    };
    for line in md.lines() {
        let l = line.trim_end();
        if l.trim_start().starts_with("```") {
            in_code = !in_code;
            flush(&mut buf, &heading, &mut out);
            continue;
        }
        if in_code {
            continue;
        }
        let t = l.trim_start();
        if let Some(h) = t.strip_prefix('#') {
            flush(&mut buf, &heading, &mut out);
            heading = h.trim_start_matches('#').trim().to_string();
        } else if t.is_empty() || t.starts_with('|') || t.starts_with("---") {
            flush(&mut buf, &heading, &mut out);
        } else if let Some(item) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| {
            let d = t.find(". ")?;
            t[..d].chars().all(|c| c.is_ascii_digit()).then(|| &t[d + 2..])
        }) {
            flush(&mut buf, &heading, &mut out);
            buf.push(item.trim_start_matches("[ ] ").trim_start_matches("[x] ").to_string());
        } else {
            buf.push(t.to_string());
        }
    }
    flush(&mut buf, &heading, &mut out);
    out
}

/// Guess the kind from wording. Conservative: anything unclear is a `note`.
pub fn guess_kind(t: &str) -> &'static str {
    let l = t.to_lowercase();
    let has = |ws: &[&str]| ws.iter().any(|w| l.contains(w));
    if has(&["we decided", "we chose", "decision:", "we use ", "we went with", "instead of", "because "]) && !has(&["never ", "must ", "always "]) {
        "decision"
    } else if has(&["never ", "must ", "always ", "don't ", "do not ", "avoid ", "only ", "required", "should "]) {
        "rule"
    } else if has(&["prefer", "i like", "style:"]) {
        "preference"
    } else {
        "note"
    }
}

/// Paths and symbols mentioned in `t` that exist in this repository's index.
fn mentions(db: &Db, t: &str) -> Result<Vec<String>> {
    let mut about = vec![];
    for w in text::raw_identifiers(t) {
        let w = w.trim_matches('`').to_string();
        let looks_like_code = w.contains('/') || w.contains('.') || w.contains('_') || w.contains("::")
            || w.chars().skip(1).any(|c| c.is_uppercase());
        if !looks_like_code || about.contains(&w) {
            continue;
        }
        if !knowledge::resolve_about(db, &w)?.is_empty() {
            about.push(w);
        } else if !w.contains('/') && !w.contains('.') {
            // a constant or other name the index does not hold as a symbol: link the one file defining it
            if let Some(f) = only_file_containing(db, &w)?
                && !about.contains(&f)
            {
                about.push(f);
            }
        }
    }
    Ok(about)
}

/// The single indexed source file mentioning `word` as a whole word (None if zero or several).
fn only_file_containing(db: &Db, word: &str) -> Result<Option<String>> {
    let re = regex::Regex::new(&format!(r"\b{}\b", regex::escape(word)))?;
    let mut st = db.conn.prepare(
        "SELECT path FROM nodes WHERE kind='file' AND format NOT IN ('markdown','text','json','yaml','toml') LIMIT 5000",
    )?;
    let paths: Vec<String> = st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let mut hit = None;
    for p in paths {
        if std::fs::read_to_string(db.root.join(&p)).is_ok_and(|s| re.is_match(&s)) {
            if hit.is_some() {
                return Ok(None);
            }
            hit = Some(p);
        }
    }
    Ok(hit)
}

/// Import the given files. With `dry_run`, only report what would be stored.
pub fn import_files(db: &Db, files: &[impl AsRef<Path>], dry_run: bool) -> Result<Value> {
    let mut items = vec![];
    for f in files {
        let f = f.as_ref();
        let md = std::fs::read_to_string(f)?;
        for (heading, t) in split(&md) {
            let kind = guess_kind(&t);
            let about = mentions(db, &t)?;
            let mut rec = json!({"file": f.display().to_string(), "heading": heading, "kind": kind, "about": about, "text": t});
            if !dry_run {
                let tags = if heading.is_empty() { vec!["imported".to_string()] } else { vec!["imported".to_string(), heading.to_lowercase()] };
                let v = knowledge::remember(
                    db,
                    RememberInput {
                        content: t.clone(),
                        kind: Some(kind.into()),
                        about: about.clone(),
                        tags,
                        origin: Some("user".into()),
                        ..Default::default()
                    },
                )?;
                rec["id"] = v["id"].clone();
            }
            items.push(rec);
        }
    }
    let by_kind = items.iter().fold(serde_json::Map::new(), |mut m, i| {
        let k = i["kind"].as_str().unwrap_or("note").to_string();
        let n = m.get(&k).and_then(|v| v.as_u64()).unwrap_or(0) + 1;
        m.insert(k, json!(n));
        m
    });
    let linked = items.iter().filter(|i| i["about"].as_array().is_some_and(|a| !a.is_empty())).count();
    Ok(json!({"items": items.len(), "linked_to_code": linked, "by_kind": by_kind, "dry_run": dry_run, "detail": items}))
}
