//! Orchestration memory: a user-level store (`~/.anchor/orchestration.db`, or `$ANCHOR_HOME`) that lets a
//! Claude session manage other sessions the way the user does, across compactions and restarts.
//!
//! - An **episode** is one orchestration: a goal, the sessions involved, decisions, delegations, worker
//!   reports and an outcome. It outlives the orchestrator session, so any later orchestrator can resume it.
//! - A **pattern** is how the user handles a kind of situation ("workers run the tests before reporting
//!   done"). Anchor collects evidence (the user's own prompts in orchestrated sessions); Claude distils it into
//!   patterns; the user confirms them. Only confirmed patterns may be acted on without asking.
//! - A session becomes an orchestrator only when it calls `orchestration_start` (usually via /orchestrate,
//!   or after Claude asks the user). Other sessions are untouched, apart from a one-time nudge when they
//!   start messaging other sessions. Nothing here is created until the user first orchestrates.

use crate::text;
use anyhow::{Result, anyhow, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::PathBuf;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY, cwd TEXT, repo TEXT, first_prompt TEXT, last_prompt TEXT,
    prompts INTEGER NOT NULL DEFAULT 0, started_at INTEGER NOT NULL, last_seen INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS episodes (
    id TEXT PRIMARY KEY, goal TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'open', outcome TEXT,
    created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
);
-- role: orchestrator | worker
CREATE TABLE IF NOT EXISTS members (
    episode TEXT NOT NULL, session TEXT NOT NULL, role TEXT NOT NULL, label TEXT,
    PRIMARY KEY (episode, session)
);
-- kind: decision | delegation | report | note | outcome
CREATE TABLE IF NOT EXISTS entries (
    id INTEGER PRIMARY KEY, episode TEXT NOT NULL, ts INTEGER NOT NULL, kind TEXT NOT NULL,
    session TEXT, author TEXT NOT NULL, text TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS evidence (
    id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, session TEXT NOT NULL, episode TEXT, kind TEXT NOT NULL, text TEXT NOT NULL
);
-- trust: inferred | confirmed | rejected
CREATE TABLE IF NOT EXISTS patterns (
    id TEXT PRIMARY KEY, situation TEXT NOT NULL, action TEXT NOT NULL, trust TEXT NOT NULL DEFAULT 'inferred',
    evidence INTEGER NOT NULL DEFAULT 0, episode TEXT, superseded_by TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS session_files (session TEXT NOT NULL, file TEXT NOT NULL, PRIMARY KEY (session, file));
CREATE TABLE IF NOT EXISTS flags (session TEXT NOT NULL, flag TEXT NOT NULL, PRIMARY KEY (session, flag));
"#;

const AUTHORITY: &str = "Authority: you manage these sessions for the user. Act on CONFIRMED patterns; treat INFERRED \
patterns as suggestions; ask the user before anything new, risky or irreversible (deleting, pushing, migrations, \
spending, merging). Log decisions, delegations and the outcome with `orchestration_log` so a later orchestrator \
can continue.";

const BUDGET_CHARS: usize = 3500;

/// (id, situation, action, trust, score)
pub type PatternRow = (String, String, String, String, f64);

pub fn home() -> PathBuf {
    std::env::var_os("ANCHOR_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".anchor"))
}

pub fn db_path() -> PathBuf {
    home().join("orchestration.db")
}

pub struct Orch {
    pub conn: Connection,
}

fn now() -> i64 {
    crate::db::now()
}

fn short(s: &str, n: usize) -> String {
    crate::knowledge::short(s, n)
}

fn hid(prefix: &str, s: &str) -> String {
    format!("{prefix}:{}", &blake3::hash(s.as_bytes()).to_hex()[..10])
}

impl Orch {
    /// Open the store. With `create = false`, returns None when the user has never orchestrated.
    pub fn open(create: bool) -> Result<Option<Orch>> {
        let p = db_path();
        if !p.exists() && !create {
            return Ok(None);
        }
        std::fs::create_dir_all(home())?;
        let conn = Connection::open(&p)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        // v0.3.2: entries can be superseded (a reversed decision)
        let has: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('entries') WHERE name='superseded_by'")?
            .exists([])?;
        if !has {
            conn.execute_batch("ALTER TABLE entries ADD COLUMN superseded_by INTEGER")?;
        }
        Ok(Some(Orch { conn }))
    }

    // ---------- sessions ----------
    pub fn touch_session(&self, session: &str, cwd: Option<&str>, repo: Option<&str>, prompt: Option<&str>) -> Result<()> {
        let t = now();
        self.conn.execute(
            "INSERT INTO sessions(id, cwd, repo, first_prompt, last_prompt, prompts, started_at, last_seen)
             VALUES(?1, ?2, ?3, ?4, ?4, ?5, ?6, ?6)
             ON CONFLICT(id) DO UPDATE SET cwd=COALESCE(excluded.cwd, cwd), repo=COALESCE(excluded.repo, repo),
               first_prompt=COALESCE(first_prompt, excluded.first_prompt), last_prompt=COALESCE(excluded.last_prompt, last_prompt),
               prompts=prompts + excluded.prompts, last_seen=excluded.last_seen",
            params![session, cwd, repo, prompt.map(|p| short(p, 160)), prompt.is_some() as i64, t],
        )?;
        Ok(())
    }

    /// The open episode this session belongs to, and its role there.
    pub fn membership(&self, session: &str) -> Result<Option<(String, String)>> {
        Ok(self
            .conn
            .query_row(
                "SELECT m.episode, m.role FROM members m JOIN episodes e ON e.id = m.episode
                 WHERE m.session = ?1 AND e.status = 'open' ORDER BY m.role = 'orchestrator' DESC, e.updated_at DESC LIMIT 1",
                params![session],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    fn flag_once(&self, session: &str, flag: &str) -> Result<bool> {
        Ok(self.conn.execute("INSERT OR IGNORE INTO flags(session, flag) VALUES(?1, ?2)", params![session, flag])? == 1)
    }

    // ---------- episodes ----------
    fn terms(s: &str) -> BTreeSet<String> {
        text::query_terms(s)
            .into_iter()
            .map(|t| crate::recall::stem(&t).unwrap_or(t))
            .collect()
    }

    fn episode_text(&self, id: &str) -> Result<String> {
        let goal: String = self.conn.query_row("SELECT goal FROM episodes WHERE id=?1", params![id], |r| r.get(0))?;
        let mut st = self.conn.prepare("SELECT text FROM entries WHERE episode=?1 AND superseded_by IS NULL AND kind IN ('decision','delegation','outcome')")?;
        let rest: Vec<String> = st.query_map(params![id], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(format!("{goal}\n{}", rest.join("\n")))
    }

    /// Past episodes similar to this situation: shared sessions and repositories count most, then goal words.
    pub fn similar(&self, goal: &str, sessions: &[String], repos: &[String]) -> Result<Vec<(String, f64, String, String)>> {
        let q = Self::terms(goal);
        let ids: Vec<(String, String, String)> = {
            let mut st = self.conn.prepare("SELECT id, goal, status FROM episodes ORDER BY updated_at DESC LIMIT 500")?;
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?
        };
        let mut out = vec![];
        for (id, g, status) in ids {
            let t = Self::terms(&self.episode_text(&id)?);
            let overlap = q.intersection(&t).count() as f64;
            let words = if q.is_empty() { 0.0 } else { overlap / (q.len() as f64).max(3.0) };
            let mut st = self.conn.prepare(
                "SELECT m.session, s.repo FROM members m LEFT JOIN sessions s ON s.id = m.session WHERE m.episode=?1",
            )?;
            let mem: Vec<(String, Option<String>)> = st.query_map(params![id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
            let shared_s = mem.iter().filter(|(s, _)| sessions.iter().any(|x| x == s)).count() as f64;
            let shared_r = mem.iter().filter_map(|(_, r)| r.as_ref()).filter(|r| repos.contains(r)).collect::<BTreeSet<_>>().len() as f64;
            let score = words + 0.6 * shared_s + 0.3 * shared_r;
            if score >= 0.5 {
                out.push((id, score, g, status));
            }
        }
        out.sort_by(|a, b| b.1.total_cmp(&a.1));
        out.truncate(3);
        Ok(out)
    }

    pub fn create_episode(&self, goal: &str) -> Result<String> {
        let t = now();
        let id = hid("e", &format!("{goal}\x00{t}\x00{}", std::process::id()));
        self.conn.execute(
            "INSERT INTO episodes(id, goal, status, created_at, updated_at) VALUES(?1, ?2, 'open', ?3, ?3)",
            params![id, goal.trim(), t],
        )?;
        Ok(id)
    }

    pub fn reopen(&self, id: &str) -> Result<()> {
        let n = self.conn.execute("UPDATE episodes SET status='open', updated_at=?2 WHERE id=?1", params![id, now()])?;
        if n == 0 {
            bail!("no episode {id}");
        }
        Ok(())
    }

    pub fn link(&self, episode: &str, session: &str, role: &str, label: Option<&str>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO members(episode, session, role, label) VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(episode, session) DO UPDATE SET role=CASE WHEN role='orchestrator' THEN role ELSE excluded.role END,
               label=COALESCE(excluded.label, label)",
            params![episode, session, role, label],
        )?;
        self.touch_session(session, None, None, None)?;
        Ok(())
    }

    pub fn log(&self, episode: &str, kind: &str, text: &str, session: Option<&str>, author: &str) -> Result<i64> {
        self.log_superseding(episode, kind, text, session, author, None)
    }

    /// Log an entry; with `supersedes`, the earlier entry (e.g. a reversed decision) is retired.
    pub fn log_superseding(&self, episode: &str, kind: &str, text: &str, session: Option<&str>, author: &str, supersedes: Option<i64>) -> Result<i64> {
        if !matches!(kind, "decision" | "delegation" | "report" | "note" | "outcome") {
            bail!("kind must be decision | delegation | report | note | outcome | end");
        }
        self.conn.execute(
            "INSERT INTO entries(episode, ts, kind, session, author, text) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![episode, now(), kind, session, author, short(text, 1500)],
        )?;
        let id = self.conn.last_insert_rowid();
        if let Some(old) = supersedes {
            let n = self.conn.execute("UPDATE entries SET superseded_by=?2 WHERE id=?1 AND episode=?3", params![old, id, episode])?;
            if n == 0 {
                bail!("no entry #{old} in {episode}");
            }
        }
        self.conn.execute("UPDATE episodes SET updated_at=?2 WHERE id=?1", params![episode, now()])?;
        Ok(id)
    }

    pub fn end(&self, episode: &str, outcome: &str) -> Result<()> {
        self.log(episode, "outcome", outcome, None, "orchestrator")?;
        self.conn.execute("UPDATE episodes SET status='closed', outcome=?2, updated_at=?3 WHERE id=?1", params![episode, outcome, now()])?;
        Ok(())
    }

    fn resolve_episode(&self, given: Option<&str>) -> Result<String> {
        if let Some(e) = given.filter(|e| !e.is_empty()) {
            return Ok(e.to_string());
        }
        let mut st = self.conn.prepare("SELECT id, goal FROM episodes WHERE status='open' ORDER BY updated_at DESC")?;
        let open: Vec<(String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        match open.len() {
            0 => bail!("no open orchestration episode; call orchestration_start first"),
            1 => Ok(open[0].0.clone()),
            _ => bail!(
                "several episodes are open; pass `episode`: {}",
                open.iter().map(|(i, g)| format!("{i} ({})", short(g, 60))).collect::<Vec<_>>().join(", ")
            ),
        }
    }

    // ---------- patterns ----------
    pub fn remember_pattern(&self, situation: &str, action: &str, evidence: i64, episode: Option<&str>, supersedes: Option<&str>) -> Result<String> {
        let id = hid("p", &format!("{}\x00{}", situation.trim().to_lowercase(), action.trim().to_lowercase()));
        let t = now();
        self.conn.execute(
            "INSERT INTO patterns(id, situation, action, trust, evidence, episode, created_at, updated_at)
             VALUES(?1, ?2, ?3, 'inferred', ?4, ?5, ?6, ?6)
             ON CONFLICT(id) DO UPDATE SET evidence=MAX(evidence, excluded.evidence), updated_at=excluded.updated_at",
            params![id, situation.trim(), action.trim(), evidence, episode, t],
        )?;
        if let Some(old) = supersedes {
            self.conn.execute("UPDATE patterns SET superseded_by=?2, updated_at=?3 WHERE id=?1 AND id != ?2", params![old, id, t])?;
        }
        Ok(id)
    }

    pub fn review_pattern(&self, id: &str, verdict: &str) -> Result<()> {
        let trust = match verdict {
            "confirm" | "confirmed" => "confirmed",
            "reject" | "rejected" | "forget" => "rejected",
            "inferred" => "inferred",
            _ => bail!("verdict must be confirm | reject"),
        };
        if self.conn.execute("UPDATE patterns SET trust=?2, updated_at=?3 WHERE id=?1", params![id, trust, now()])? == 0 {
            bail!("no pattern {id}");
        }
        Ok(())
    }

    /// Patterns relevant to a situation (confirmed first). Rejected and superseded ones are never returned.
    pub fn relevant_patterns(&self, situation: &str, limit: usize) -> Result<Vec<PatternRow>> {
        let q = Self::terms(situation);
        let mut st = self.conn.prepare(
            "SELECT id, situation, action, trust FROM patterns WHERE trust != 'rejected' AND superseded_by IS NULL",
        )?;
        let rows: Vec<(String, String, String, String)> =
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?;
        let mut out: Vec<_> = rows
            .into_iter()
            .map(|(id, s, a, t)| {
                let p = Self::terms(&format!("{s} {a}"));
                let overlap = q.intersection(&p).count() as f64 / (p.len().clamp(1, 8) as f64);
                let w = if t == "confirmed" { 1.0 } else { 0.0 };
                (id, s, a, t, overlap + 0.15 * w)
            })
            .filter(|x| x.4 >= 0.2)
            .collect();
        out.sort_by(|a, b| b.4.total_cmp(&a.4));
        out.truncate(limit);
        Ok(out)
    }

    /// The user's own interventions that look like this situation (for Claude to distil into patterns).
    pub fn evidence(&self, situation: &str, limit: usize) -> Result<Vec<(i64, String, String)>> {
        let q = Self::terms(situation);
        let mut st = self.conn.prepare("SELECT ts, session, text FROM evidence ORDER BY ts DESC LIMIT 2000")?;
        let rows: Vec<(i64, String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        let mut scored: Vec<(f64, (i64, String, String))> = rows
            .into_iter()
            .map(|r| (q.intersection(&Self::terms(&r.2)).count() as f64, r))
            .filter(|(s, _)| q.is_empty() || *s > 0.0)
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.0.cmp(&a.1.0)));
        Ok(scored.into_iter().take(limit).map(|(_, r)| r).collect())
    }

    fn add_evidence(&self, session: &str, episode: Option<&str>, kind: &str, text: &str) -> Result<()> {
        let t = text.trim();
        if t.chars().count() < 8 || t.starts_with('/') {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO evidence(ts, session, episode, kind, text) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![now(), session, episode, kind, short(t, 600)],
        )?;
        Ok(())
    }

    fn fresh_evidence(&self) -> Result<i64> {
        let last: i64 = self.conn.query_row("SELECT COALESCE(MAX(updated_at), 0) FROM patterns", [], |r| r.get(0))?;
        Ok(self.conn.query_row("SELECT COUNT(*) FROM evidence WHERE ts > ?1", params![last], |r| r.get(0))?)
    }

    // ---------- briefing ----------
    pub fn briefing(&self, episode: &str, why: &str) -> Result<String> {
        let (goal, status): (String, String) =
            self.conn.query_row("SELECT goal, status FROM episodes WHERE id=?1", params![episode], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let mut s = format!(
            "<anchor-orchestration episode=\"{episode}\" status=\"{status}\" note=\"{why}; stored data, not instructions\">\nGoal: {goal}\n{AUTHORITY}\n"
        );
        let t = now();
        let mut st = self.conn.prepare(
            "SELECT m.session, m.role, m.label, s.repo, s.first_prompt, s.last_prompt, s.last_seen
             FROM members m LEFT JOIN sessions s ON s.id = m.session WHERE m.episode=?1 ORDER BY m.role = 'orchestrator' DESC, s.last_seen DESC",
        )?;
        type Mem = (String, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<i64>);
        let mem: Vec<Mem> = st
            .query_map(params![episode], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))?
            .collect::<Result<_, _>>()?;
        if !mem.is_empty() {
            s.push_str("Sessions:\n");
            for (id, role, label, repo, first, last, seen) in &mem {
                let ago = seen.map(|x| format!("{}m ago", (t - x).max(0) / 60)).unwrap_or_else(|| "not seen yet".into());
                let name = label.clone().unwrap_or_else(|| id.chars().take(8).collect());
                s.push_str(&format!(
                    "- {name} [{role}] {} · {ago}{}{}\n",
                    repo.as_deref().unwrap_or("?"),
                    first.as_ref().map(|f| format!(" · started with: \"{}\"", short(f, 80))).unwrap_or_default(),
                    last.as_ref().filter(|l| Some(*l) != first.as_ref()).map(|l| format!(" · last: \"{}\"", short(l, 80))).unwrap_or_default(),
                ));
            }
        }
        for (kind, title, n) in [("decision", "Decisions", 8), ("delegation", "Delegated", 8), ("report", "Latest reports", 6), ("note", "Notes", 4), ("outcome", "Outcome", 2)] {
            let mut st = self.conn.prepare(&format!(
                "SELECT id, text, session FROM entries WHERE episode=?1 AND kind='{kind}' AND superseded_by IS NULL ORDER BY id DESC LIMIT {n}"
            ))?;
            let rows: Vec<(i64, String, Option<String>)> = st.query_map(params![episode], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
            if !rows.is_empty() {
                s.push_str(&format!("{title}:\n"));
                for (id, text, sess) in rows.iter().rev() {
                    let who = sess.as_ref().map(|x| format!("[{}] ", x.chars().take(8).collect::<String>())).unwrap_or_default();
                    s.push_str(&format!("- #{id} {who}{}\n", short(text, 240)));
                }
            }
        }
        // patterns for this kind of situation first, then the user's other patterns (how they orchestrate in general)
        let mut pats = self.relevant_patterns(&self.episode_text(episode)?, 6)?;
        for p in all_patterns(self)? {
            if pats.len() >= 6 {
                break;
            }
            if !pats.iter().any(|x| x.0 == p.0) {
                pats.push(p);
            }
        }
        if !pats.is_empty() {
            s.push_str("Your user's patterns for situations like this:\n");
            for (id, sit, act, trust, _) in pats {
                s.push_str(&format!("- [{}] {id} when {}: {}\n", trust.to_uppercase(), short(&sit, 100), short(&act, 160)));
            }
        }
        let fresh = self.fresh_evidence()?;
        if fresh >= 5 {
            s.push_str(&format!(
                "{fresh} new user interventions since patterns were last updated: when convenient, call `pattern` with op \"evidence\" and record any recurring way the user handles things (op \"remember\").\n"
            ));
        }
        s.push_str("</anchor-orchestration>");
        if s.chars().count() > BUDGET_CHARS {
            s = s.chars().take(BUDGET_CHARS - 30).collect::<String>() + "\n…</anchor-orchestration>";
        }
        Ok(s)
    }
}

// ---------- MCP ----------

pub fn is_tool(name: &str) -> bool {
    name.starts_with("orchestration_") || name == "pattern"
}

pub fn tools() -> Vec<Value> {
    vec![
        json!({"name": "orchestration_start",
            "description": "Become the orchestrator of an orchestration episode, using the user's orchestration memory: past episodes, decisions and the user's patterns for managing sessions. Call it when the user asks you, in any words, to coordinate, manage or orchestrate other sessions or workers for them (\"you are coordinating these sessions\", \"keep track of the workers\", \"pick up coordination\", /orchestrate), or agrees after you ask. That request is the permission; do not start it when nobody asked you to coordinate anything. Then log decisions, delegations and worker reports with `orchestration_log` as they happen. If similar past episodes exist and neither `resume` nor `new` is given, they are listed and nothing is created: ask the user, then call again.",
            "inputSchema": {"type": "object", "properties": {
                "goal": {"type": "string", "description": "What this orchestration is for, in one or two sentences."},
                "sessions": {"type": "array", "items": {"type": "string"}, "description": "Ids of the sessions you will manage, if known."},
                "repos": {"type": "array", "items": {"type": "string"}, "description": "Repository paths involved, if known."},
                "resume": {"type": "string", "description": "Id of a past episode (e:...) to continue."},
                "new": {"type": "boolean", "description": "Start a new episode even if similar ones exist."}
            }, "required": ["goal"]}}),
        json!({"name": "orchestration_context",
            "description": "The current state of an orchestration episode: goal, sessions and their last activity, decisions, delegations, worker reports, and the user's relevant patterns. Use it before deciding what to do next, or when unsure what was already done.",
            "inputSchema": {"type": "object", "properties": {"episode": {"type": "string"}}}}),
        json!({"name": "orchestration_log",
            "description": "Record orchestration state so a later orchestrator can continue: kind decision (what was decided and why), delegation (what you asked which session to do), report (a session's result), note, or end (close the episode with its outcome). Link a session to the episode with kind \"link\" and `session` (its id) and optional `label`. When a decision changes, log the new one with `supersedes` = the old entry's #number, so the old one is no longer shown.",
            "inputSchema": {"type": "object", "properties": {
                "kind": {"type": "string", "enum": ["decision", "delegation", "report", "note", "end", "link"]},
                "text": {"type": "string"},
                "session": {"type": "string", "description": "The session this is about (id)."},
                "label": {"type": "string", "description": "A short name for a linked session."},
                "supersedes": {"type": "integer", "description": "The #number of an earlier entry this one replaces (e.g. a reversed decision)."},
                "episode": {"type": "string", "description": "Episode id; defaults to the only open one."}
            }, "required": ["kind"]}}),
        json!({"name": "pattern",
            "description": "The user's orchestration patterns: how they handle recurring situations. op evidence: the user's own past interventions similar to `situation` (to learn from). op remember: record a pattern you see repeated (`situation`, `action`, `evidence` count, optional `supersedes`); it starts INFERRED. op review: confirm or reject a pattern (`id`, `verdict`) only after the user explicitly says so. op list: patterns relevant to `situation` (or all).",
            "inputSchema": {"type": "object", "properties": {
                "op": {"type": "string", "enum": ["evidence", "remember", "review", "list"]},
                "situation": {"type": "string"}, "action": {"type": "string"},
                "evidence": {"type": "integer"}, "supersedes": {"type": "string"},
                "id": {"type": "string"}, "verdict": {"type": "string", "enum": ["confirm", "reject"]}
            }, "required": ["op"]}}),
    ]
}

fn strs(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default()
}

/// Handle an orchestration tool call. Works anywhere (no project needed).
pub fn call(name: &str, args: &Value) -> Result<String> {
    let o = Orch::open(true)?.ok_or_else(|| anyhow!("cannot open orchestration store"))?;
    let s = |k: &str| args.get(k).and_then(|v| v.as_str()).map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
    match name {
        "orchestration_start" => {
            let goal = s("goal").ok_or_else(|| anyhow!("`goal` is required"))?;
            let sessions = strs(args.get("sessions"));
            let repos = strs(args.get("repos"));
            let id = if let Some(e) = s("resume") {
                o.reopen(&e)?;
                e
            } else {
                let sim = o.similar(&goal, &sessions, &repos)?;
                if !sim.is_empty() && !args.get("new").and_then(|v| v.as_bool()).unwrap_or(false) {
                    let mut t = String::from("Similar past orchestrations exist. Ask the user whether to resume one (call again with `resume`) or start new (`new: true`):\n");
                    for (id, score, g, status) in &sim {
                        t.push_str(&format!("- {id} ({status}, match {score:.2}): {}\n", short(g, 120)));
                    }
                    // the state of the closest match, so a session picking up coordination has the facts now
                    t.push_str(&format!("\nState of the closest match ({}):\n{}", sim[0].0, o.briefing(&sim[0].0, "closest past orchestration")?));
                    return Ok(t);
                }
                o.create_episode(&goal)?
            };
            for x in &sessions {
                o.link(&id, x, "worker", None)?;
            }
            // the PostToolUse hook binds the calling session as orchestrator from this marker
            Ok(format!("ORCHESTRATOR_OF {id}\n{}", o.briefing(&id, "orchestration started")?))
        }
        "orchestration_context" => {
            let e = o.resolve_episode(s("episode").as_deref())?;
            o.briefing(&e, "current state")
        }
        "orchestration_log" => {
            let kind = s("kind").ok_or_else(|| anyhow!("`kind` is required"))?;
            let e = o.resolve_episode(s("episode").as_deref())?;
            match kind.as_str() {
                "end" => {
                    o.end(&e, &s("text").unwrap_or_else(|| "ended".into()))?;
                    Ok(format!("closed {e}"))
                }
                "link" => {
                    let sess = s("session").ok_or_else(|| anyhow!("`session` is required to link"))?;
                    o.link(&e, &sess, "worker", s("label").as_deref())?;
                    Ok(format!("linked {sess} to {e}"))
                }
                k => {
                    let text = s("text").ok_or_else(|| anyhow!("`text` is required"))?;
                    let sup = args.get("supersedes").and_then(|v| v.as_i64());
                    let id = o.log_superseding(&e, k, &text, s("session").as_deref(), "orchestrator", sup)?;
                    Ok(format!("logged {k} #{id} in {e}{}", sup.map(|x| format!(" (replaces #{x})")).unwrap_or_default()))
                }
            }
        }
        "pattern" => match s("op").as_deref().unwrap_or("") {
            "evidence" => {
                let rows = o.evidence(&s("situation").unwrap_or_default(), 15)?;
                if rows.is_empty() {
                    return Ok("No matching interventions recorded yet.".into());
                }
                Ok(rows.iter().map(|(ts, sess, t)| format!("- [{ts} {}] {}", sess.chars().take(8).collect::<String>(), short(t, 300))).collect::<Vec<_>>().join("\n"))
            }
            "remember" => {
                let sit = s("situation").ok_or_else(|| anyhow!("`situation` is required"))?;
                let act = s("action").ok_or_else(|| anyhow!("`action` is required"))?;
                let ev = args.get("evidence").and_then(|v| v.as_i64()).unwrap_or(0);
                let id = o.remember_pattern(&sit, &act, ev, None, s("supersedes").as_deref())?;
                Ok(format!("stored {id} as INFERRED (the user can confirm it with /patterns)"))
            }
            "review" => {
                let id = s("id").ok_or_else(|| anyhow!("`id` is required"))?;
                o.review_pattern(&id, &s("verdict").unwrap_or_default())?;
                Ok(format!("{id} reviewed"))
            }
            "list" => {
                let rows = if let Some(sit) = s("situation") { o.relevant_patterns(&sit, 20)? } else { all_patterns(&o)? };
                if rows.is_empty() {
                    return Ok("No patterns yet.".into());
                }
                Ok(rows.iter().map(|(id, sit, act, t, _)| format!("- [{}] {id} when {sit}: {act}", t.to_uppercase())).collect::<Vec<_>>().join("\n"))
            }
            op => bail!("unknown op {op:?} (evidence | remember | review | list)"),
        },
        _ => bail!("unknown tool {name}"),
    }
}

pub fn all_patterns(o: &Orch) -> Result<Vec<PatternRow>> {
    let mut st = o.conn.prepare(
        "SELECT id, situation, action, trust FROM patterns WHERE superseded_by IS NULL AND trust != 'rejected' ORDER BY trust='confirmed' DESC, updated_at DESC",
    )?;
    Ok(st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, 0.0)))?.collect::<Result<_, _>>()?)
}

// ---------- hooks ----------

const MANAGE_TOOLS: &[&str] = &["SendMessage", "ListAgents"];

/// Does the user's prompt ask for coordination of sessions/workers? Word-based, no model.
pub fn coordination_intent(p: &str) -> bool {
    let l = p.to_lowercase();
    let verb = ["coordinat", "orchestrat", "manage", "managing", "keep track of", "pick up", "picking up", "take over", "oversee"];
    let obj = ["session", "worker", "agents", "orchestrat", "coordination"];
    verb.iter().any(|v| l.contains(v)) && obj.iter().any(|o| l.contains(o))
}

/// Orchestration side of a hook event. Returns extra context to add (plain text), or None.
/// Never creates the store: until the user first orchestrates, this does nothing but the one-time nudge.
pub fn hook(event: &str, input: &Value, repo: Option<&std::path::Path>, project_flag_once: &mut dyn FnMut() -> bool) -> Result<Option<String>> {
    let session = input.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
    if session.is_empty() {
        return Ok(None);
    }
    let tool = input.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
    let store = Orch::open(false)?;
    let member = match &store {
        Some(o) => o.membership(session)?,
        None => None,
    };
    let is_orch = member.as_ref().is_some_and(|m| m.1 == "orchestrator");

    // bind: the orchestration_start call came from this session
    if event == "PostToolUse" && tool.ends_with("orchestration_start") {
        let resp = input.get("tool_response").map(|v| v.to_string()).unwrap_or_default();
        let re = regex::Regex::new(r"ORCHESTRATOR_OF (e:[0-9a-f]{10})")?;
        if let (Some(c), Some(o)) = (re.captures(&resp), Orch::open(true)?) {
            o.link(&c[1], session, "orchestrator", None)?;
            o.conn.execute("UPDATE members SET role='orchestrator' WHERE episode=?1 AND session=?2", params![&c[1], session])?;
            o.touch_session(session, input.get("cwd").and_then(|v| v.as_str()), repo.map(|r| r.to_string_lossy()).as_deref(), None)?;
        }
        return Ok(None);
    }

    // a session that is not orchestrating starts managing other sessions: suggest it once
    if !is_orch && event == "PostToolUse" && MANAGE_TOOLS.contains(&tool) {
        let first = match &store {
            Some(o) => o.flag_once(session, "nudged")?,
            None => project_flag_once(),
        };
        if first {
            return Ok(Some("Anchor: this session is now coordinating other sessions. If you are managing them for the user, ask the user once whether to use their orchestration memory (the `orchestration_start` tool, or /orchestrate). Do not switch to orchestrating on your own.".into()));
        }
        return Ok(None);
    }

    // the user asks, in their own words, for coordination of sessions: point to orchestration memory once
    if event == "UserPromptSubmit" && !is_orch
        && let Some(p) = input.get("prompt").and_then(|v| v.as_str())
        && coordination_intent(p)
    {
        let first = match &store {
            Some(o) => o.flag_once(session, "intent")?,
            None => project_flag_once(),
        };
        if first {
            return Ok(Some("Anchor: this request is about coordinating other sessions or workers. Use the orchestration memory: call `orchestration_start` with the goal (it finds or resumes past orchestrations), then log decisions, delegations and worker reports with `orchestration_log`.".into()));
        }
    }

    let Some(o) = store else { return Ok(None) };
    let cwd = input.get("cwd").and_then(|v| v.as_str());
    let repo_s = repo.map(|r| r.to_string_lossy().to_string());
    let prompt = input.get("prompt").and_then(|v| v.as_str());
    if event == "UserPromptSubmit" && std::env::var("ANCHOR_ORCH_CATALOG").as_deref() != Ok("0") {
        o.touch_session(session, cwd, repo_s.as_deref(), prompt)?;
    }
    let Some((episode, role)) = member else { return Ok(None) };
    match (event, role.as_str()) {
        ("SessionStart", "orchestrator") => {
            let why = match input.get("source").and_then(|v| v.as_str()) {
                Some("compact") => "restored after compaction",
                Some("resume") => "resumed session",
                _ => "orchestrator session started",
            };
            Ok(Some(o.briefing(&episode, why)?))
        }
        ("PreCompact", "orchestrator") => Ok(Some(format!(
            "Orchestration (Anchor): this session orchestrates episode {episode}. In the summary keep the current plan, open \
             delegations and anything decided but not yet logged; Anchor restores the logged state after compaction."
        ))),
        ("UserPromptSubmit", _) => {
            // the user's own words to an orchestrated session are evidence of how they orchestrate
            if let Some(p) = prompt {
                o.add_evidence(session, Some(&episode), if role == "orchestrator" { "to_orchestrator" } else { "to_worker" }, p)?;
            }
            Ok(None)
        }
        ("PostToolUse", "orchestrator") if tool == "SendMessage" => {
            let ti = input.get("tool_input").cloned().unwrap_or(Value::Null);
            let to = ti.get("to").and_then(|v| v.as_str()).unwrap_or("?");
            let msg = ti.get("message").and_then(|v| v.as_str()).unwrap_or("");
            o.log(&episode, "delegation", &format!("to {to}: {}", short(msg, 400)), None, "orchestrator")?;
            Ok(None)
        }
        ("PostToolUse", "worker") if matches!(tool, "Edit" | "Write" | "MultiEdit") => {
            if let Some(f) = input.pointer("/tool_input/file_path").and_then(|v| v.as_str()) {
                let rel = repo.and_then(|r| std::path::Path::new(f).strip_prefix(r).ok()).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| f.to_string());
                o.conn.execute("INSERT OR IGNORE INTO session_files(session, file) VALUES(?1, ?2)", params![session, rel])?;
            }
            Ok(None)
        }
        ("Stop", "worker") => {
            let mut st = o.conn.prepare("SELECT file FROM session_files WHERE session=?1 ORDER BY file")?;
            let files: Vec<String> = st.query_map(params![session], |r| r.get(0))?.collect::<Result<_, _>>()?;
            if !files.is_empty() {
                let last = o.conn.query_row("SELECT last_prompt FROM sessions WHERE id=?1", params![session], |r| r.get::<_, Option<String>>(0)).ok().flatten();
                let text = format!(
                    "edited {}{}",
                    files.iter().take(12).cloned().collect::<Vec<_>>().join(", "),
                    last.map(|l| format!(" (task: \"{}\")", short(&l, 120))).unwrap_or_default()
                );
                o.log(&episode, "report", &text, Some(session), "anchor")?;
                o.conn.execute("DELETE FROM session_files WHERE session=?1", params![session])?;
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}
