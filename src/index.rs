//! Code + docs index: tree-sitter symbols, normalized AST hashes, call graph,
//! doc sections, and anchor-based invalidation of linked knowledge.

use crate::db::{Db, now};
use crate::text::{self, Format};
use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use tree_sitter::{Language, Node, Parser};

#[derive(Debug, Clone)]
pub struct Symbol {
    pub id: String,
    pub qualified: String,
    pub short: String,
    pub kind: String,
    pub signature: String,
    pub body: String,
    pub hash: String,
    pub start: usize,
    pub end: usize,
    pub calls: BTreeSet<String>,
}

#[derive(Debug, Default)]
pub struct IndexReport {
    pub files_seen: usize,
    pub files_indexed: usize,
    pub symbols: usize,
    pub changed_symbols: Vec<String>,
    pub removed_symbols: Vec<String>,
    /// knowledge nodes newly marked stale: (id, stale level, content)
    pub newly_stale: Vec<(String, i64, String)>,
}

fn language_for(f: Format) -> Option<Language> {
    Some(match f {
        Format::Rust => tree_sitter_rust::LANGUAGE.into(),
        Format::Python => tree_sitter_python::LANGUAGE.into(),
        Format::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Format::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Format::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        _ => return None,
    })
}

fn symbol_label(kind: &str, node: &Node) -> Option<&'static str> {
    Some(match kind {
        // rust
        "function_item" | "function_signature_item" => "fn",
        "struct_item" => "struct",
        "enum_item" => "enum",
        "trait_item" => "trait",
        "impl_item" => "impl",
        "mod_item" => "mod",
        "const_item" | "static_item" => "const",
        "macro_definition" => "macro",
        "type_item" => "type",
        // python
        "function_definition" => "fn",
        "class_definition" => "class",
        // ts / js
        "function_declaration" | "generator_function_declaration" => "fn",
        "class_declaration" | "abstract_class_declaration" => "class",
        "method_definition" => "method",
        "interface_declaration" => "interface",
        "type_alias_declaration" => "type",
        "enum_declaration" => "enum",
        "variable_declarator" => {
            let v = node.child_by_field_name("value")?;
            match v.kind() {
                "arrow_function" | "function_expression" | "function" => "fn",
                _ => return None,
            }
        }
        _ => return None,
    })
}

fn is_call(kind: &str) -> bool {
    matches!(kind, "call_expression" | "call" | "macro_invocation" | "new_expression")
}

/// Hash of the syntax tree with comments and whitespace removed: formatting and
/// comment edits keep the hash; logic edits change it.
pub fn ast_hash(node: Node, src: &[u8]) -> String {
    let mut h = blake3::Hasher::new();
    hash_node(node, src, &mut h);
    h.finalize().to_hex()[..16].to_string()
}

fn hash_node(node: Node, src: &[u8], h: &mut blake3::Hasher) {
    let k = node.kind();
    if k.contains("comment") {
        return;
    }
    h.update(k.as_bytes());
    h.update(b"\x00");
    if node.child_count() == 0 {
        if let Ok(t) = node.utf8_text(src) {
            h.update(t.as_bytes());
            h.update(b"\x01");
        }
        return;
    }
    let mut cur = node.walk();
    for c in node.children(&mut cur) {
        hash_node(c, src, h);
    }
    h.update(b"\x02");
}

fn last_segment(s: &str) -> String {
    let s = s.split('<').next().unwrap_or(s);
    let s = s.trim_end_matches('!');
    s.rsplit([':', '.', '>', '-'])
        .next()
        .unwrap_or(s)
        .trim()
        .to_string()
}

fn collect_calls(node: Node, src: &[u8], out: &mut BTreeSet<String>) {
    if is_call(node.kind()) {
        let f = node
            .child_by_field_name("function")
            .or_else(|| node.child_by_field_name("macro"))
            .or_else(|| node.child_by_field_name("constructor"));
        if let Some(f) = f
            && let Ok(t) = f.utf8_text(src) {
                let name = last_segment(t);
                if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    out.insert(name);
                }
            }
    }
    let mut cur = node.walk();
    for c in node.children(&mut cur) {
        collect_calls(c, src, out);
    }
}

pub fn extract_symbols(path: &str, fmt: Format, src: &[u8]) -> Vec<Symbol> {
    let Some(lang) = language_for(fmt) else { return vec![] };
    let mut parser = Parser::new();
    if parser.set_language(&lang).is_err() {
        return vec![];
    }
    let Some(tree) = parser.parse(src, None) else { return vec![] };
    let mut out = Vec::new();
    walk(tree.root_node(), src, path, &mut Vec::new(), &mut out);
    // disambiguate duplicate ids (overloads, multiple impls)
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for s in out.iter_mut() {
        let n = seen.entry(s.id.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            s.id = format!("{}#{}", s.id, n);
        }
    }
    out
}

fn walk(node: Node, src: &[u8], path: &str, scope: &mut Vec<String>, out: &mut Vec<Symbol>) {
    let kind = node.kind();
    if let Some(label) = symbol_label(kind, &node) {
        let name_node = node
            .child_by_field_name("name")
            .or_else(|| node.child_by_field_name("type"));
        if let Some(nn) = name_node {
            let name = nn.utf8_text(src).unwrap_or("").to_string();
            let short = last_segment(&name);
            if !short.is_empty() {
                let mut qual = scope.clone();
                qual.push(name.clone());
                let qualified = qual.join("::");
                let text = node.utf8_text(src).unwrap_or("");
                let signature: String = text.lines().next().unwrap_or("").trim().chars().take(200).collect();
                let body: String = text.chars().take(4000).collect();
                let mut calls = BTreeSet::new();
                collect_calls(node, src, &mut calls);
                calls.remove(&short);
                out.push(Symbol {
                    id: format!("sym:{}::{}", path, qualified),
                    qualified: qualified.clone(),
                    short,
                    kind: label.to_string(),
                    signature,
                    body,
                    hash: ast_hash(node, src),
                    start: node.start_position().row + 1,
                    end: node.end_position().row + 1,
                    calls,
                });
                scope.push(name);
                let mut cur = node.walk();
                for c in node.children(&mut cur) {
                    walk(c, src, path, scope, out);
                }
                scope.pop();
                return;
            }
        }
    }
    let mut cur = node.walk();
    for c in node.children(&mut cur) {
        walk(c, src, path, scope, out);
    }
}

#[derive(Debug, Clone)]
pub struct DocSection {
    pub id: String,
    pub title: String,
    pub body: String,
    pub hash: String,
    pub start: usize,
}

pub fn extract_sections(path: &str, src: &str) -> Vec<DocSection> {
    let mut out = Vec::new();
    let mut title = String::from("(top)");
    let mut start = 1;
    let mut buf = String::new();
    let mut in_fence = false;
    let flush = |title: &str, start: usize, buf: &str, out: &mut Vec<DocSection>| {
        if buf.trim().is_empty() {
            return;
        }
        let norm: String = buf.split_whitespace().collect::<Vec<_>>().join(" ");
        let slug: String = title
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect();
        out.push(DocSection {
            id: format!("doc:{}#{}", path, slug),
            title: title.to_string(),
            body: buf.chars().take(4000).collect(),
            hash: blake3::hash(norm.as_bytes()).to_hex()[..16].to_string(),
            start,
        });
    };
    for (i, line) in src.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        if !in_fence && line.starts_with('#') {
            flush(&title, start, &buf, &mut out);
            title = line.trim_start_matches('#').trim().to_string();
            start = i + 1;
            buf.clear();
        }
        buf.push_str(line);
        buf.push('\n');
    }
    flush(&title, start, &buf, &mut out);
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for s in out.iter_mut() {
        let n = seen.entry(s.id.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            s.id = format!("{}-{}", s.id, n);
        }
    }
    out
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

/// Full or incremental index of the project.
pub fn index_all(db: &Db, force: bool) -> Result<IndexReport> {
    let mut report = IndexReport::default();
    let root = db.root.clone();
    let walker = ignore::WalkBuilder::new(&root)
        .hidden(true)
        .git_ignore(true)
        .filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            !(n == ".anchor" || n == ".git" || n == "target" || n == "node_modules" || n == ".venv")
        })
        .build();
    let tx = db.conn.unchecked_transaction()?;
    let mut present = BTreeSet::new();
    for entry in walker.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let p = entry.path();
        let r = rel(&root, p);
        present.insert(r.clone());
        report.files_seen += 1;
        if index_file_inner(db, p, &r, force, &mut report)? {
            report.files_indexed += 1;
        }
    }
    // files that disappeared
    let known: Vec<String> = {
        let mut st = db.conn.prepare("SELECT path FROM files")?;
        st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    for k in known {
        if !present.contains(&k) {
            remove_file(db, &k, &mut report)?;
        }
    }
    resolve_links(db)?;
    propagate(db, &mut report)?;
    tx.commit()?;
    db.meta_set("last_index", &now().to_string())?;
    Ok(report)
}

/// Re-index one file (used by PostToolUse after an edit).
pub fn index_one(db: &Db, abs: &Path) -> Result<IndexReport> {
    let mut report = IndexReport::default();
    let r = rel(&db.root, abs);
    let tx = db.conn.unchecked_transaction()?;
    if abs.exists() {
        if index_file_inner(db, abs, &r, false, &mut report)? {
            report.files_indexed += 1;
        }
    } else {
        remove_file(db, &r, &mut report)?;
    }
    resolve_links(db)?;
    propagate(db, &mut report)?;
    tx.commit()?;
    Ok(report)
}

fn index_file_inner(db: &Db, abs: &Path, r: &str, force: bool, report: &mut IndexReport) -> Result<bool> {
    let md = match std::fs::metadata(abs) {
        Ok(m) => m,
        Err(_) => return Ok(false),
    };
    if md.len() > 1_000_000 {
        return Ok(false);
    }
    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0);
    let size = md.len() as i64;
    if !force {
        let prev: Option<(i64, i64)> = db
            .conn
            .query_row("SELECT mtime, size FROM files WHERE path=?1", params![r], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()?;
        if prev == Some((mtime, size)) {
            return Ok(false);
        }
    }
    let bytes = std::fs::read(abs)?;
    let fmt = text::detect_format(abs, &bytes);
    let file_id = format!("file:{}", r);
    let t = now();

    // old symbol hashes for this file
    let old: BTreeMap<String, String> = {
        let mut st = db
            .conn
            .prepare("SELECT id, COALESCE(ast_hash,'') FROM nodes WHERE path=?1 AND origin='code'")?;
        st.query_map(params![r], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?
    };

    let mut new_ids: BTreeMap<String, String> = BTreeMap::new();
    
    let file_hash: String = match fmt {
        f if f.is_code() => {
            let syms = extract_symbols(r, f, &bytes);
            let mut fh = blake3::Hasher::new();
            for s in &syms {
                fh.update(s.hash.as_bytes());
                upsert_code_node(db, &s.id, "symbol", f.name(), &s.body, Some(&s.signature), r,
                    Some(&s.qualified), Some(&s.short), &s.hash, s.start, s.end, t)?;
                db.conn.execute("UPDATE nodes SET tags=?2 WHERE id=?1", params![s.id, s.kind])?;
                // store unresolved calls in edges with dst = "name:<short>"
                db.conn.execute("DELETE FROM edges WHERE src=?1 AND type IN ('calls_name')", params![s.id])?;
                for c in &s.calls {
                    db.conn.execute(
                        "INSERT OR IGNORE INTO edges(src,dst,type,weight) VALUES(?1,?2,'calls_name',0.5)",
                        params![s.id, format!("name:{}", c)],
                    )?;
                }
                db.conn.execute(
                    "INSERT OR IGNORE INTO edges(src,dst,type,weight) VALUES(?1,?2,'contains',0.6)",
                    params![file_id, s.id],
                )?;
                new_ids.insert(s.id.clone(), s.hash.clone());
            }
            report.symbols += syms.len();
            fh.finalize().to_hex()[..16].to_string()
        }
        Format::Markdown => {
            let src = String::from_utf8_lossy(&bytes);
            let secs = extract_sections(r, &src);
            let mut fh = blake3::Hasher::new();
            for s in &secs {
                fh.update(s.hash.as_bytes());
                upsert_code_node(db, &s.id, "doc_section", "markdown", &s.body, Some(&s.title), r,
                    Some(&s.title), None, &s.hash, s.start, s.start, t)?;
                db.conn.execute(
                    "INSERT OR IGNORE INTO edges(src,dst,type,weight) VALUES(?1,?2,'contains',0.6)",
                    params![file_id, s.id],
                )?;
                new_ids.insert(s.id.clone(), s.hash.clone());
            }
            fh.finalize().to_hex()[..16].to_string()
        }
        Format::Json | Format::Yaml | Format::Toml => {
            let src = String::from_utf8_lossy(&bytes);
            let norm: String = src.split_whitespace().collect::<Vec<_>>().join(" ");
            let h = blake3::hash(norm.as_bytes()).to_hex()[..16].to_string();
            let id = format!("cfg:{}", r);
            let body: String = src.chars().take(8000).collect();
            upsert_code_node(db, &id, "config", fmt.name(), &body, None, r, None, None, &h, 1, 1, t)?;
            db.conn.execute(
                "INSERT OR IGNORE INTO edges(src,dst,type,weight) VALUES(?1,?2,'contains',0.6)",
                params![file_id, id],
            )?;
            new_ids.insert(id, h.clone());
            h
        }
        _ => {
            blake3::hash(&bytes).to_hex()[..16].to_string()
        }
    };
    // file node (anchor target for file-level knowledge)
    upsert_code_node(db, &file_id, "file", fmt.name(), r, None, r, None, None, &file_hash, 1, 1, t)?;
    new_ids.insert(file_id.clone(), file_hash.clone());

    for (id, h) in &old {
        match new_ids.get(id) {
            None => {
                remove_node(db, id)?;
                report.removed_symbols.push(id.clone());
            }
            Some(nh) if nh != h => report.changed_symbols.push(id.clone()),
            _ => {}
        }
    }
    db.conn.execute(
        "INSERT INTO files(path,mtime,size,hash,format) VALUES(?1,?2,?3,?4,?5)
         ON CONFLICT(path) DO UPDATE SET mtime=excluded.mtime,size=excluded.size,hash=excluded.hash,format=excluded.format",
        params![r, mtime, size, file_hash, fmt.name()],
    )?;
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn upsert_code_node(db: &Db, id: &str, kind: &str, format: &str, content: &str, summary: Option<&str>,
    path: &str, symbol: Option<&str>, short: Option<&str>, hash: &str, start: usize, end: usize, t: i64) -> Result<()> {
    let changed: bool = db
        .conn
        .query_row("SELECT COALESCE(ast_hash,'') != ?2 FROM nodes WHERE id=?1", params![id, hash], |r| r.get(0))
        .optional()?
        .unwrap_or(true);
    db.conn.execute(
        "INSERT INTO nodes(id,kind,format,content,summary,trust,origin,path,symbol,short_name,ast_hash,start_line,end_line,simhash,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,'verified','code',?6,?7,?8,?9,?10,?11,0,?12,?12)
         ON CONFLICT(id) DO UPDATE SET content=excluded.content, summary=excluded.summary, format=excluded.format,
           ast_hash=excluded.ast_hash, start_line=excluded.start_line, end_line=excluded.end_line, updated_at=excluded.updated_at",
        params![id, kind, format, content, summary, path, symbol, short, hash, start as i64, end as i64, t],
    )?;
    if changed && kind != "file" {
        let mut body = String::new();
        if let Some(s) = symbol {
            body.push_str(s);
            body.push('\n');
        }
        body.push_str(path);
        body.push('\n');
        body.push_str(content);
        db.fts_put(id, &body)?;
    }
    Ok(())
}

fn remove_node(db: &Db, id: &str) -> Result<()> {
    db.conn.execute("DELETE FROM nodes WHERE id=?1", params![id])?;
    db.conn.execute(
        "DELETE FROM edges WHERE (src=?1 OR dst=?1) AND type IN ('calls','calls_name','contains','mentions')",
        params![id],
    )?;
    db.fts_delete(id)?;
    Ok(())
}

fn remove_file(db: &Db, r: &str, report: &mut IndexReport) -> Result<()> {
    let ids: Vec<String> = {
        let mut st = db.conn.prepare("SELECT id FROM nodes WHERE path=?1 AND origin='code'")?;
        st.query_map(params![r], |row| row.get(0))?.collect::<Result<_, _>>()?
    };
    for id in ids {
        remove_node(db, &id)?;
        report.removed_symbols.push(id);
    }
    db.conn.execute("DELETE FROM files WHERE path=?1", params![r])?;
    Ok(())
}

/// Resolve `calls_name` edges to concrete symbols, and doc→symbol `mentions`.
fn resolve_links(db: &Db) -> Result<()> {
    db.conn.execute("DELETE FROM edges WHERE type IN ('calls','mentions')", [])?;
    // calls: name -> up to 3 symbols with that short name
    db.conn.execute(
        "INSERT OR IGNORE INTO edges(src,dst,type,weight)
         SELECT e.src, n.id, 'calls', 0.5 FROM edges e
         JOIN nodes n ON n.short_name = substr(e.dst, 6) AND n.kind='symbol'
         WHERE e.type='calls_name'
           AND (SELECT COUNT(*) FROM nodes m WHERE m.short_name = substr(e.dst,6) AND m.kind='symbol') <= 3",
        [],
    )?;
    // mentions: doc sections that name a symbol (identifiers >= 4 chars, unambiguous)
    let docs: Vec<(String, String)> = {
        let mut st = db.conn.prepare("SELECT id, content FROM nodes WHERE kind='doc_section'")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
    };
    if docs.is_empty() {
        return Ok(());
    }
    let names: BTreeMap<String, Vec<String>> = {
        let mut st = db.conn.prepare("SELECT short_name, id FROM nodes WHERE kind='symbol' AND length(short_name) >= 4")?;
        let rows: Vec<(String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (n, id) in rows {
            m.entry(n).or_default().push(id);
        }
        m
    };
    for (doc, content) in docs {
        for ident in text::raw_identifiers(&content) {
            if let Some(ids) = names.get(&ident)
                && ids.len() <= 3 {
                    for id in ids {
                        db.conn.execute(
                            "INSERT OR IGNORE INTO edges(src,dst,type,weight) VALUES(?1,?2,'mentions',0.4)",
                            params![doc, id],
                        )?;
                    }
                }
        }
    }
    Ok(())
}

/// Mark knowledge stale when an anchor's hash no longer matches, and spread
/// a softer flag one hop (callers of changed code, knowledge that depends_on stale knowledge).
pub fn propagate(db: &Db, report: &mut IndexReport) -> Result<()> {
    // 1. direct: about-edge anchor hash differs from current target hash, or target is gone
    let direct: Vec<(String, String)> = {
        let mut st = db.conn.prepare(
            "SELECT DISTINCT k.id, k.content FROM edges e
             JOIN nodes k ON k.id = e.src
             LEFT JOIN nodes t ON t.id = e.dst
             WHERE e.type='about' AND k.origin != 'code' AND k.superseded_by IS NULL AND k.stale != 1
               AND e.anchor_hash IS NOT NULL
               AND (t.id IS NULL OR COALESCE(t.ast_hash,'') != e.anchor_hash)",
        )?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
    };
    for (id, c) in direct {
        db.conn.execute("UPDATE nodes SET stale=1, updated_at=?2 WHERE id=?1", params![id, now()])?;
        report.newly_stale.push((id, 1, c));
    }
    // 2. soft: knowledge about symbols that call a changed symbol
    for changed in report.changed_symbols.clone() {
        let soft: Vec<(String, String)> = {
            let mut st = db.conn.prepare(
                "SELECT DISTINCT k.id, k.content FROM edges c
                 JOIN edges a ON a.dst = c.src AND a.type='about'
                 JOIN nodes k ON k.id = a.src
                 WHERE c.type='calls' AND c.dst=?1 AND k.stale=0 AND k.superseded_by IS NULL",
            )?;
            st.query_map(params![changed], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
        };
        for (id, c) in soft {
            db.conn.execute("UPDATE nodes SET stale=2 WHERE id=?1 AND stale=0", params![id])?;
            report.newly_stale.push((id, 2, c));
        }
    }
    // 3. knowledge that depends_on stale knowledge
    let dep: Vec<(String, String)> = {
        let mut st = db.conn.prepare(
            "SELECT DISTINCT k.id, k.content FROM edges e
             JOIN nodes d ON d.id = e.dst AND d.stale = 1
             JOIN nodes k ON k.id = e.src
             WHERE e.type='depends_on' AND k.stale=0 AND k.superseded_by IS NULL",
        )?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
    };
    for (id, c) in dep {
        db.conn.execute("UPDATE nodes SET stale=2 WHERE id=?1 AND stale=0", params![id])?;
        report.newly_stale.push((id, 2, c));
    }
    Ok(())
}
