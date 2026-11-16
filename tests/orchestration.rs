//! Orchestration memory: user-level episodes, session binding, briefings across compaction, worker
//! reports, evidence and patterns. Every test uses its own ANCHOR_HOME (never the real ~/.anchor).

use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const EXE: &str = env!("CARGO_BIN_EXE_anchor");

struct Env {
    _t: tempfile::TempDir,
    home: std::path::PathBuf,
    repo: std::path::PathBuf, // a git repo (worker)
    plain: std::path::PathBuf, // not a repo (where an orchestrator may run)
}

fn env() -> Env {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("anchor-home");
    let repo = t.path().join("api");
    let plain = t.path().join("desk");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/auth.py"), "def login(u):\n    return u\n").unwrap();
    std::fs::create_dir_all(&plain).unwrap();
    Env { _t: t, home, repo, plain }
}

fn hook(e: &Env, event: &str, cwd: &Path, session: &str, extra: Value) -> String {
    let mut input = json!({"session_id": session, "cwd": cwd, "hook_event_name": event});
    input.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    let mut c = Command::new(EXE)
        .args(["hook", event])
        .env("ANCHOR_HOME", &e.home)
        .env_remove("CLAUDE_PROJECT_DIR")
        .env_remove("ANCHOR_ROOT")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(input.to_string().as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success(), "hook {event} failed: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn ctx_text(out: &str) -> String {
    serde_json::from_str::<Value>(out.trim())
        .ok()
        .and_then(|v| v["hookSpecificOutput"]["additionalContext"].as_str().map(String::from))
        .unwrap_or_default()
}

/// One tool call through a real `anchor mcp` process (the way Claude Code calls it).
fn mcp(e: &Env, cwd: &Path, tool: &str, args: Value) -> (String, bool) {
    let mut c = Command::new(EXE)
        .arg("mcp")
        .env("ANCHOR_HOME", &e.home)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let msgs = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": tool, "arguments": args}}),
    ];
    let mut stdin = c.stdin.take().unwrap();
    for m in msgs {
        writeln!(stdin, "{m}").unwrap();
    }
    drop(stdin);
    let out = c.wait_with_output().unwrap();
    let last: Value = serde_json::from_str(String::from_utf8_lossy(&out.stdout).lines().last().unwrap()).unwrap();
    let text = last["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string();
    (text, last["result"]["isError"] == true)
}

fn cli(e: &Env, args: &[&str]) -> String {
    let o = Command::new(EXE).args(args).env("ANCHOR_HOME", &e.home).current_dir(&e.plain).output().unwrap();
    assert!(o.status.success(), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).to_string()
}

/// Start an orchestration from `cwd` as session `sid`, exactly as Claude Code would: the MCP call, then the
/// PostToolUse hook for it (which carries the session id and the tool's response).
fn start(e: &Env, cwd: &Path, sid: &str, args: Value) -> String {
    let (text, err) = mcp(e, cwd, "orchestration_start", args.clone());
    assert!(!err, "{text}");
    hook(e, "PostToolUse", cwd, sid, json!({"tool_name": "mcp__anchor__orchestration_start", "tool_input": args,
        "tool_response": [{"type": "text", "text": text}]}));
    text
}

fn episode_id(text: &str) -> String {
    regex::Regex::new(r"ORCHESTRATOR_OF (e:[0-9a-f]{10})").unwrap().captures(text).map(|c| c[1].to_string()).unwrap_or_default()
}

#[test]
fn nothing_is_created_until_the_user_orchestrates() {
    let e = env();
    for ev in ["SessionStart", "UserPromptSubmit", "Stop", "PreCompact"] {
        hook(&e, ev, &e.repo, "w1", json!({"prompt": "fix the login bug", "source": "startup"}));
        hook(&e, ev, &e.plain, "x1", json!({"prompt": "hello", "source": "startup"}));
    }
    assert!(!e.home.join("orchestration.db").exists(), "no user-level store before the first orchestration");
    assert!(!e.plain.join(".anchor").exists(), "nothing created outside a project");

    // a session in a repo that starts messaging other sessions is nudged once, and only once
    let first = ctx_text(&hook(&e, "PostToolUse", &e.repo, "w1", json!({"tool_name": "SendMessage", "tool_input": {"to": "web", "message": "run tests"}})));
    assert!(first.contains("ask the user once") && first.contains("orchestration_start"), "{first}");
    let second = ctx_text(&hook(&e, "PostToolUse", &e.repo, "w1", json!({"tool_name": "SendMessage", "tool_input": {"to": "web", "message": "again"}})));
    assert!(!second.contains("orchestration_start"), "nudge must not repeat: {second}");
    // spawning a subagent is ordinary work, not orchestration
    let agent = ctx_text(&hook(&e, "PostToolUse", &e.repo, "w2", json!({"tool_name": "Agent", "tool_input": {}})));
    assert!(!agent.contains("orchestration_start"));
    assert!(!e.home.join("orchestration.db").exists(), "the nudge itself creates no user-level store");
}

#[test]
fn orchestrator_is_bound_briefed_and_restored_after_compaction() {
    let e = env();
    let text = start(&e, &e.plain, "orc1", json!({"goal": "Migrate auth to OAuth across api and web", "sessions": ["w-api"]}));
    let ep = episode_id(&text);
    assert!(!ep.is_empty(), "{text}");
    assert!(text.contains("Authority:") && text.contains("CONFIRMED"), "briefing carries the authority rules: {text}");

    let (r, err) = mcp(&e, &e.plain, "orchestration_log", json!({"kind": "decision", "text": "api goes first; web waits for the token endpoint"}));
    assert!(!err, "{r}");
    // the orchestrator's SendMessage calls are logged as delegations automatically
    hook(&e, "PostToolUse", &e.plain, "orc1", json!({"tool_name": "SendMessage", "tool_input": {"to": "w-api", "message": "add the /token endpoint and its tests"}}));

    // compaction: PreCompact steers the summary (plain text); SessionStart(compact) restores the state verbatim
    let pre = hook(&e, "PreCompact", &e.plain, "orc1", json!({"trigger": "auto"}));
    assert!(pre.contains(&ep) && !pre.trim_start().starts_with('{'), "{pre}");
    let restored = ctx_text(&hook(&e, "SessionStart", &e.plain, "orc1", json!({"source": "compact"})));
    assert!(restored.contains("restored after compaction"), "{restored}");
    assert!(restored.contains("Migrate auth to OAuth"), "goal: {restored}");
    assert!(restored.contains("api goes first"), "decision: {restored}");
    assert!(restored.contains("to w-api: add the /token endpoint"), "delegation: {restored}");
    assert!(restored.contains("w-api [worker]"), "linked session: {restored}");

    // another session in the same folder is not the orchestrator and gets nothing
    let other = ctx_text(&hook(&e, "SessionStart", &e.plain, "someone-else", json!({"source": "startup"})));
    assert!(other.is_empty(), "{other}");

    // ending closes the episode; the briefing no longer appears at the next start
    let (r, _) = mcp(&e, &e.plain, "orchestration_log", json!({"kind": "end", "text": "both migrated, tokens rotate daily"}));
    assert!(r.contains("closed"));
    assert!(ctx_text(&hook(&e, "SessionStart", &e.plain, "orc1", json!({"source": "resume"}))).is_empty());
    assert!(cli(&e, &["orch", "episodes"]).contains("closed"));
}

#[test]
fn similar_situations_offer_resume_instead_of_starting_over() {
    let e = env();
    let a = episode_id(&start(&e, &e.plain, "orc1", json!({"goal": "Migrate auth to OAuth across the api and web sessions", "sessions": ["w-api", "w-web"]})));
    mcp(&e, &e.plain, "orchestration_log", json!({"kind": "end", "text": "paused: web blocked on design"}));

    // a later, similar situation: Anchor lists the past episode and creates nothing until the user chooses
    let (offer, _) = mcp(&e, &e.plain, "orchestration_start", json!({"goal": "continue the OAuth auth migration", "sessions": ["w-web"]}));
    assert!(offer.contains("Similar past orchestrations") && offer.contains(&a) && !offer.contains("ORCHESTRATOR_OF"), "{offer}");
    assert_eq!(cli(&e, &["orch", "episodes"]).lines().count(), 1, "nothing created yet");

    // resume: the same episode reopens with its history
    let resumed = start(&e, &e.plain, "orc2", json!({"goal": "continue the OAuth auth migration", "resume": a}));
    assert_eq!(episode_id(&resumed), a);
    assert!(resumed.contains("paused: web blocked on design"), "history comes back: {resumed}");

    // an unrelated goal starts a new episode directly
    let other = start(&e, &e.plain, "orc3", json!({"goal": "Prepare the quarterly billing report"}));
    assert!(!episode_id(&other).is_empty() && episode_id(&other) != a, "{other}");
    // new: true forces a fresh episode even when similar ones exist
    let forced = start(&e, &e.plain, "orc4", json!({"goal": "OAuth auth migration, second attempt", "new": true}));
    assert!(!episode_id(&forced).is_empty() && episode_id(&forced) != a);
}

#[test]
fn workers_report_and_user_interventions_become_patterns() {
    let e = env();
    let ep = episode_id(&start(&e, &e.plain, "orc1", json!({"goal": "Ship the login fix in api"})));
    let (r, _) = mcp(&e, &e.plain, "orchestration_log", json!({"kind": "link", "session": "w-api", "label": "api"}));
    assert!(r.contains("linked"));

    // the user works in the worker session directly: their words are evidence; edits are reported at Stop
    hook(&e, "UserPromptSubmit", &e.repo, "w-api", json!({"prompt": "Before you say it's done, run the full test suite and paste the summary"}));
    hook(&e, "UserPromptSubmit", &e.repo, "w-api", json!({"prompt": "fix the login redirect bug"}));
    hook(&e, "PostToolUse", &e.repo, "w-api", json!({"tool_name": "Edit", "tool_input": {"file_path": e.repo.join("src/auth.py")}}));
    hook(&e, "Stop", &e.repo, "w-api", json!({}));
    let brief = cli(&e, &["orch", "show", &ep]);
    assert!(brief.contains("Latest reports") && brief.contains("edited src/auth.py"), "{brief}");
    assert!(brief.contains("api [worker]"), "{brief}");
    // an unlinked session's prompts are not evidence
    hook(&e, "UserPromptSubmit", &e.repo, "unrelated", json!({"prompt": "Before you say it's done, run the linter on everything"}));

    let (ev, _) = mcp(&e, &e.plain, "pattern", json!({"op": "evidence", "situation": "worker says the task is done"}));
    assert!(ev.contains("run the full test suite"), "{ev}");
    assert!(!ev.contains("run the linter"), "unlinked sessions are not mined: {ev}");

    // Claude distils a pattern: INFERRED until the user confirms it
    let (stored, _) = mcp(&e, &e.plain, "pattern", json!({"op": "remember", "situation": "a worker reports a task done",
        "action": "ask it to run the full test suite and paste the summary first", "evidence": 1}));
    let pid = regex::Regex::new(r"p:[0-9a-f]{10}").unwrap().find(&stored).unwrap().as_str().to_string();
    assert!(cli(&e, &["orch", "show", &ep]).contains(&format!("[INFERRED] {pid}")));
    cli(&e, &["orch", "confirm", &pid]);
    let after = cli(&e, &["orch", "show", &ep]);
    assert!(after.contains(&format!("[CONFIRMED] {pid}")), "{after}");

    // a correction supersedes it; a rejected pattern never comes back
    let (newer, _) = mcp(&e, &e.plain, "pattern", json!({"op": "remember", "situation": "a worker reports a task done",
        "action": "ask for the test summary and the diff stat", "supersedes": pid}));
    let pid2 = regex::Regex::new(r"p:[0-9a-f]{10}").unwrap().find(&newer).unwrap().as_str().to_string();
    let listed = cli(&e, &["orch", "patterns"]);
    assert!(listed.contains(&pid2) && !listed.contains(&pid), "{listed}");
    cli(&e, &["orch", "reject", &pid2]);
    assert!(!cli(&e, &["orch", "patterns"]).contains(&pid2));
}

#[test]
fn commands_are_installed_without_overwriting_the_users_own() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("commands");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("patterns.md"), "my own command").unwrap();
    let written = anchor::install::install_commands(&dir).unwrap();
    assert_eq!(written.len(), 1, "only orchestrate.md: {written:?}");
    let orch = std::fs::read_to_string(dir.join("orchestrate.md")).unwrap();
    assert!(orch.contains("orchestration_start") && orch.contains("$ARGUMENTS"));
    assert_eq!(std::fs::read_to_string(dir.join("patterns.md")).unwrap(), "my own command");
    anchor::install::uninstall_commands(&dir);
    assert!(!dir.join("orchestrate.md").exists());
    assert!(dir.join("patterns.md").exists(), "the user's file is never removed");
}

#[test]
fn coordination_requests_in_plain_words_point_to_orchestration_memory() {
    let e = env();
    let hint = ctx_text(&hook(&e, "UserPromptSubmit", &e.repo, "s1", json!({"prompt": "You are coordinating three worker sessions for me: api, web and docs."})));
    assert!(hint.contains("orchestration_start"), "{hint}");
    let again = ctx_text(&hook(&e, "UserPromptSubmit", &e.repo, "s1", json!({"prompt": "keep track of the workers please"})));
    assert!(!again.contains("orchestration_start"), "only once per session: {again}");
    let plain = ctx_text(&hook(&e, "UserPromptSubmit", &e.repo, "s2", json!({"prompt": "Fix the flaky login test in api"})));
    assert!(!plain.contains("orchestration_start"), "ordinary work gets no hint: {plain}");
    assert!(anchor::orch::coordination_intent("You're picking up coordination of the OAuth migration workers"));
    assert!(!anchor::orch::coordination_intent("manage the database connection pool"));

    // a session picking up coordination sees the closest past episode's state before choosing to resume
    let a = episode_id(&start(&e, &e.plain, "orc1", json!({"goal": "OAuth migration across api web docs workers"})));
    mcp(&e, &e.plain, "orchestration_log", json!({"kind": "decision", "text": "docs may push the changelog straight to main"}));
    let (offer, _) = mcp(&e, &e.plain, "orchestration_start", json!({"goal": "pick up coordination of the OAuth migration workers"}));
    assert!(offer.contains(&a) && offer.contains("docs may push the changelog straight to main"), "{offer}");
}

#[test]
fn a_reversed_decision_replaces_the_old_one() {
    let e = env();
    let ep = episode_id(&start(&e, &e.plain, "orc1", json!({"goal": "OAuth migration across api web docs"})));
    let (first, _) = mcp(&e, &e.plain, "orchestration_log", json!({"kind": "decision", "text": "docs may push the changelog straight to main"}));
    let n: i64 = regex::Regex::new(r"#(\d+)").unwrap().captures(&first).unwrap()[1].parse().unwrap();
    let brief = cli(&e, &["orch", "show", &ep]);
    assert!(brief.contains(&format!("#{n} docs may push")), "entries are numbered: {brief}");
    let (second, err) = mcp(&e, &e.plain, "orchestration_log", json!({"kind": "decision", "text": "docs changes go through a PR again (main is protected)", "supersedes": n}));
    assert!(!err && second.contains(&format!("replaces #{n}")), "{second}");
    let after = cli(&e, &["orch", "show", &ep]);
    assert!(after.contains("go through a PR again") && !after.contains("may push the changelog"), "{after}");
    let (bad, err) = mcp(&e, &e.plain, "orchestration_log", json!({"kind": "decision", "text": "x", "supersedes": 9999}));
    assert!(err && bad.contains("no entry #9999"), "{bad}");
}

#[test]
fn the_users_words_to_the_orchestrator_are_recorded_verbatim() {
    let e = env();
    // the first request arrives before any store exists; at binding, Anchor reads it from the transcript
    let tr = e.plain.join("t.jsonl");
    std::fs::write(&tr, format!("{}\n{}\n",
        json!({"type": "user", "message": {"role": "user", "content": "You are coordinating api, web and docs for me. docs may push the changelog straight to main."}}),
        json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "content": "x"}]}}))).unwrap();
    let args = json!({"goal": "OAuth migration across api web docs"});
    let (text, _) = mcp(&e, &e.plain, "orchestration_start", args.clone());
    hook(&e, "PostToolUse", &e.plain, "orc1", json!({"tool_name": "mcp__anchor__orchestration_start", "tool_input": args,
        "tool_response": [{"type": "text", "text": text}], "transcript_path": tr}));
    // later messages in the orchestrator are recorded as they come, newest last
    let ack = ctx_text(&hook(&e, "UserPromptSubmit", &e.plain, "orc1", json!({"prompt": "Change of plan: docs may NOT push to main any more, use a PR."})));
    assert!(ack.contains("recorded verbatim") && ack.contains("Do not log it again"), "{ack}");
    let restored = ctx_text(&hook(&e, "SessionStart", &e.plain, "orc1", json!({"source": "compact"})));
    let a = restored.find("docs may push the changelog straight to main").expect(&restored);
    let b = restored.find("docs may NOT push to main any more").expect(&restored);
    assert!(a < b, "oldest first, so the later message reads as the current rule: {restored}");
    assert!(restored.contains("later messages override earlier ones"));

    // a new session asking to pick up coordination gets the closest episode's state injected at once
    let inj = ctx_text(&hook(&e, "UserPromptSubmit", &e.repo, "fresh", json!({"prompt": "You're picking up coordination of the OAuth migration workers"})));
    assert!(inj.contains("docs may NOT push to main any more") && inj.contains("resume="), "{inj}");
}
