//! Storage: one SQLite file per project (WAL mode, safe for many sessions).

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS nodes (
    id            TEXT PRIMARY KEY,
    kind          TEXT NOT NULL,
    format        TEXT NOT NULL DEFAULT 'text',
    content       TEXT NOT NULL,
    summary       TEXT,
    tags          TEXT NOT NULL DEFAULT '',
    trust         TEXT NOT NULL,            -- user | verified | asserted | disputed | quarantined
    stale         INTEGER NOT NULL DEFAULT 0, -- 0 fresh, 1 anchor changed, 2 neighbour changed
    origin        TEXT NOT NULL,            -- code | user | agent | external
    strict        INTEGER NOT NULL DEFAULT 0,
    check_spec    TEXT,                     -- JSON
    path          TEXT,                     -- for code/doc nodes
    symbol        TEXT,                     -- qualified symbol name for code nodes
    short_name    TEXT,
    ast_hash      TEXT,
    start_line    INTEGER,
    end_line      INTEGER,
    superseded_by TEXT,
    simhash       INTEGER NOT NULL DEFAULT 0,
    session       TEXT,
    recalled      INTEGER NOT NULL DEFAULT 0,
    used          INTEGER NOT NULL DEFAULT 0,
    ignored       INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS nodes_kind  ON nodes(kind);
CREATE INDEX IF NOT EXISTS nodes_path  ON nodes(path);
CREATE INDEX IF NOT EXISTS nodes_short ON nodes(short_name);
CREATE INDEX IF NOT EXISTS nodes_rules ON nodes(id) WHERE check_spec IS NOT NULL;

CREATE TABLE IF NOT EXISTS edges (
    src         TEXT NOT NULL,
    dst         TEXT NOT NULL,
    type        TEXT NOT NULL,
    weight      REAL NOT NULL DEFAULT 1.0,
    anchor_hash TEXT,          -- hash of dst when the link was made (for invalidation)
    PRIMARY KEY (src, dst, type)
);
CREATE INDEX IF NOT EXISTS edges_dst ON edges(dst);

CREATE TABLE IF NOT EXISTS fts_map (rid INTEGER PRIMARY KEY AUTOINCREMENT, node_id TEXT NOT NULL UNIQUE);
CREATE VIRTUAL TABLE IF NOT EXISTS fts_words USING fts5(node_id UNINDEXED, body, tokenize='unicode61', prefix='4 5 6 7 8');
CREATE VIRTUAL TABLE IF NOT EXISTS fts_words_v USING fts5vocab(fts_words, 'row');
CREATE VIRTUAL TABLE IF NOT EXISTS fts_tri   USING fts5(node_id UNINDEXED, body, tokenize='trigram');

CREATE TABLE IF NOT EXISTS files (
    path     TEXT PRIMARY KEY,
    mtime    INTEGER NOT NULL,
    size     INTEGER NOT NULL,
    hash     TEXT NOT NULL,
    format   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    id                TEXT PRIMARY KEY,
    started_at        INTEGER NOT NULL,
    edits             INTEGER NOT NULL DEFAULT 0,
    remembers         INTEGER NOT NULL DEFAULT 0,
    recalls           INTEGER NOT NULL DEFAULT 0,
    checkpoint_asked  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS shown (
    session TEXT NOT NULL,
    node    TEXT NOT NULL,
    PRIMARY KEY (session, node)
);

CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL);
"#;

type FtsRow = (String, String, String, Option<String>, Option<String>, Option<String>, String, String);

pub struct Db {
    pub conn: Connection,
    pub root: PathBuf,
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Find the project root: ANCHOR_ROOT, CLAUDE_PROJECT_DIR, nearest dir with `.anchor`,
/// nearest git root, else the start dir.
pub fn find_root(start: &Path) -> PathBuf {
    if let Ok(r) = std::env::var("ANCHOR_ROOT") {
        return PathBuf::from(r);
    }
    if let Ok(r) = std::env::var("CLAUDE_PROJECT_DIR")
        && !r.is_empty() {
            return PathBuf::from(r);
        }
    let mut cur = Some(start);
    while let Some(d) = cur {
        if d.join(".anchor").is_dir() {
            return d.to_path_buf();
        }
        cur = d.parent();
    }
    let mut cur = Some(start);
    while let Some(d) = cur {
        if d.join(".git").exists() {
            return d.to_path_buf();
        }
        cur = d.parent();
    }
    start.to_path_buf()
}

/// Root of an *active* project, or None. Used by hooks and the MCP server so that a
/// global install never creates stores outside projects (e.g. in the home folder).
/// Active = ANCHOR_ROOT set, or a `.anchor/` dir, or a git repo — found by walking up.
/// ANCHOR_DISABLE=1 turns Anchor off everywhere.
pub fn project_root(start: &Path) -> Option<PathBuf> {
    if std::env::var("ANCHOR_DISABLE").map(|v| v == "1").unwrap_or(false) {
        return None;
    }
    if let Ok(r) = std::env::var("ANCHOR_ROOT") {
        return Some(PathBuf::from(r));
    }
    let home = std::env::var("HOME").map(PathBuf::from).ok();
    let mut cur = Some(start);
    while let Some(d) = cur {
        if d.join(".anchor").is_dir() && d.join(".anchor").join("anchor.db").exists() {
            return Some(d.to_path_buf());
        }
        if d.join(".git").exists() {
            // never treat the home folder itself as a project, even if it is a git repo
            if home.as_deref() == Some(d) {
                return None;
            }
            return Some(d.to_path_buf());
        }
        cur = d.parent();
    }
    None
}

impl Db {
    pub fn open(root: &Path) -> Result<Db> {
        let dir = root.join(".anchor");
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        // keep the store out of git by default
        let gi = dir.join(".gitignore");
        if !gi.exists() {
            let _ = std::fs::write(&gi, "*\n");
        }
        let conn = Connection::open(dir.join("anchor.db"))?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(SCHEMA)?;
        let db = Db { conn, root: root.to_path_buf() };
        db.migrate()?;
        Ok(db)
    }

    /// Schema 2: word index gains prefix indexes (fast stem queries). Rebuild FTS once.
    fn migrate(&self) -> Result<()> {
        const VERSION: &str = "2";
        let cur = self.meta_get("schema")?;
        if cur.as_deref() == Some(VERSION) {
            return Ok(());
        }
        let fresh: i64 = self.conn.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))?;
        if fresh > 0 {
            let tx = rusqlite::Transaction::new_unchecked(&self.conn, rusqlite::TransactionBehavior::Immediate)?;
            self.conn.execute_batch(
                "DROP TABLE IF EXISTS fts_words; DROP TABLE IF EXISTS fts_tri; DELETE FROM fts_map;",
            )?;
            self.conn.execute_batch(SCHEMA)?;
            let rows: Vec<FtsRow> = {
                let mut st = self.conn.prepare(
                    "SELECT id, kind, content, summary, symbol, path, tags, origin FROM nodes WHERE kind != 'file'",
                )?;
                st.query_map([], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?))
                })?
                .collect::<Result<_, _>>()?
            };
            for (id, kind, content, summary, symbol, path, tags, origin) in rows {
                let body = if origin == "code" {
                    format!("{}\n{}\n{}", symbol.unwrap_or_default(), path.unwrap_or_default(), content)
                } else {
                    format!("{}\n{}\n{}\n{}", kind, content, summary.unwrap_or_default(), tags)
                };
                self.fts_put(&id, &body)?;
            }
            tx.commit()?;
        }
        self.meta_set("schema", VERSION)?;
        Ok(())
    }

    pub fn open_in_memory_root(root: &Path) -> Result<Db> {
        Self::open(root)
    }

    pub fn meta_get(&self, k: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT v FROM meta WHERE k=?1", params![k], |r| r.get(0))
            .optional()?)
    }

    pub fn meta_set(&self, k: &str, v: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
            params![k, v],
        )?;
        Ok(())
    }

    pub fn ensure_session(&self, id: &str) -> Result<()> {
        if id.is_empty() {
            return Ok(());
        }
        self.conn.execute(
            "INSERT OR IGNORE INTO sessions(id, started_at) VALUES(?1, ?2)",
            params![id, now()],
        )?;
        Ok(())
    }

    pub fn bump_session(&self, id: &str, col: &str) -> Result<()> {
        if id.is_empty() {
            return Ok(());
        }
        self.ensure_session(id)?;
        let sql = match col {
            "edits" => "UPDATE sessions SET edits=edits+1 WHERE id=?1",
            "remembers" => "UPDATE sessions SET remembers=remembers+1 WHERE id=?1",
            "recalls" => "UPDATE sessions SET recalls=recalls+1 WHERE id=?1",
            "checkpoint_asked" => "UPDATE sessions SET checkpoint_asked=1 WHERE id=?1",
            _ => return Ok(()),
        };
        self.conn.execute(sql, params![id])?;
        Ok(())
    }

    /// Index text for both lexical indexes.
    /// FTS rows are keyed by a stable rowid from `fts_map`, so updates and deletes
    /// are O(log n) instead of scanning the index.
    pub fn fts_put(&self, node_id: &str, body: &str) -> Result<()> {
        self.conn.execute("INSERT OR IGNORE INTO fts_map(node_id) VALUES(?1)", params![node_id])?;
        let rid: i64 = self.conn.query_row("SELECT rid FROM fts_map WHERE node_id=?1", params![node_id], |r| r.get(0))?;
        self.conn.execute("DELETE FROM fts_words WHERE rowid=?1", params![rid])?;
        self.conn.execute("DELETE FROM fts_tri WHERE rowid=?1", params![rid])?;
        let words = crate::text::identifier_expand(body);
        self.conn.execute(
            "INSERT INTO fts_words(rowid, node_id, body) VALUES(?1, ?2, ?3)",
            params![rid, node_id, words],
        )?;
        self.conn.execute(
            "INSERT INTO fts_tri(rowid, node_id, body) VALUES(?1, ?2, ?3)",
            params![rid, node_id, body],
        )?;
        Ok(())
    }

    pub fn fts_delete(&self, node_id: &str) -> Result<()> {
        let rid: Option<i64> = self
            .conn
            .query_row("SELECT rid FROM fts_map WHERE node_id=?1", params![node_id], |r| r.get(0))
            .optional()?;
        if let Some(rid) = rid {
            self.conn.execute("DELETE FROM fts_words WHERE rowid=?1", params![rid])?;
            self.conn.execute("DELETE FROM fts_tri WHERE rowid=?1", params![rid])?;
            self.conn.execute("DELETE FROM fts_map WHERE rid=?1", params![rid])?;
        }
        Ok(())
    }
}
