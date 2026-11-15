//! Integration tests for global-install safety, hook silence outside projects, and MCP protocol.

use anchor::db::project_root;
use anchor::mcp::{Ctx, handle_message};
use serde_json::json;
use std::io::Write;
use std::process::{Command, Stdio};

const EXE: &str = env!("CARGO_BIN_EXE_anchor");

fn hook(event: &str, cwd: &std::path::Path, input: serde_json::Value) -> (String, bool) {
    let mut c = Command::new(EXE)
        .args(["hook", event])
        .env_remove("ANCHOR_ROOT")
        .env_remove("CLAUDE_PROJECT_DIR")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(input.to_string().as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    (String::from_utf8_lossy(&o.stdout).to_string(), o.status.success())
}

#[test]
fn project_root_rules() {
    let d = tempfile::tempdir().unwrap();
    let plain = d.path().join("plain/sub");
    std::fs::create_dir_all(&plain).unwrap();
    assert_eq!(project_root(&plain), None, "non-git folder must be inactive");
    let repo = d.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("src/deep")).unwrap();
    assert_eq!(project_root(&repo.join("src/deep")).as_deref(), Some(repo.as_path()));
}

#[test]
fn hooks_silent_outside_projects() {
    let d = tempfile::tempdir().unwrap();
    let input = json!({"session_id": "s", "cwd": d.path(), "prompt": "hello", "hook_event_name": "UserPromptSubmit"});
    for ev in ["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop"] {
        let (out, ok) = hook(ev, d.path(), input.clone());
        assert!(ok, "{ev} must exit 0");
        assert!(out.trim().is_empty(), "{ev} must print nothing outside a project");
    }
    assert!(!d.path().join(".anchor").exists(), "no store may be created outside a project");
}

#[test]
fn hooks_active_in_git_repo() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join(".git")).unwrap();
    std::fs::write(d.path().join("lib.py"), "def pay(x):\n    return x\n").unwrap();
    let input = json!({"session_id": "s", "cwd": d.path(), "hook_event_name": "SessionStart"});
    let (out, ok) = hook("SessionStart", d.path(), input);
    assert!(ok);
    assert!(out.contains("Anchor project memory is active"));
    assert!(d.path().join(".anchor/anchor.db").exists());
}

#[test]
fn global_settings_merge_keeps_user_hooks() {
    let mut settings = json!({
        "model": "opus",
        "hooks": {"PostToolUse": [{"matcher": "Edit", "hooks": [{"type": "command", "command": "my-linter.sh"}]}]}
    });
    anchor::install::add_hooks(&mut settings, "/x/anchor");
    anchor::install::add_hooks(&mut settings, "/x/anchor"); // idempotent
    let post = settings["hooks"]["PostToolUse"].as_array().unwrap();
    assert_eq!(post.len(), 2, "user hook kept + one anchor hook (no duplicates)");
    assert!(post.iter().any(|h| h.to_string().contains("my-linter.sh")));
    assert_eq!(settings["model"], "opus");
    assert_eq!(settings["hooks"]["SessionStart"].as_array().unwrap().len(), 1);
}

#[test]
fn mcp_protocol_and_inactive_mode() {
    let mut ctx = Ctx::new(None);
    let init = handle_message(&mut ctx, &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}})).unwrap();
    assert_eq!(init["result"]["serverInfo"]["name"], "anchor");
    assert!(handle_message(&mut ctx, &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none());
    let list = handle_message(&mut ctx, &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).unwrap();
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 3);
    let call = handle_message(&mut ctx, &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "recall", "arguments": {"query": "x"}}})).unwrap();
    assert_eq!(call["result"]["isError"], true);
    assert!(call["result"]["content"][0]["text"].as_str().unwrap().contains("inactive"));
    let bad = handle_message(&mut ctx, &json!({"jsonrpc": "2.0", "id": 4, "method": "nope"})).unwrap();
    assert_eq!(bad["error"]["code"], -32601);
}

#[test]
fn mcp_roundtrip_in_repo() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join(".git")).unwrap();
    std::fs::write(d.path().join("pay.py"), "def charge(card):\n    return card\n").unwrap();
    let mut ctx = Ctx::new(Some(d.path().to_path_buf()));
    let r = handle_message(&mut ctx, &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "remember",
        "arguments": {"content": "charge must be idempotent because the gateway retries", "kind": "decision", "about": ["charge"], "origin": "user"}}})).unwrap();
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("\"trust\": \"user\""), "{text}");
    let r = handle_message(&mut ctx, &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "recall", "arguments": {"query": "change the charge function"}}})).unwrap();
    assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("idempotent"));
}

#[test]
fn post_edit_reanchors_seen_decisions_but_flags_facts() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join(".git")).unwrap();
    let f = d.path().join("feeds.py");
    std::fs::write(&f, "def fetch_all(urls, t):\n    return [t(u) for u in urls]\n").unwrap();
    let db = anchor::db::Db::open(d.path()).unwrap();
    anchor::index::index_all(&db, false).unwrap();
    let rem = |c: &str, k: &str| anchor::knowledge::remember(&db, anchor::knowledge::RememberInput {
        content: c.into(), kind: Some(k.into()), about: vec!["feeds.py".into()], origin: Some("user".into()), ..Default::default()
    }).unwrap()["id"].as_str().unwrap().to_string();
    let dec = rem("fetch_all must stay sequential because the gateway bans parallel clients", "decision");
    let fact = rem("fetch_all returns a list in input order", "fact");
    // the agent saw both in this session
    let sit = anchor::recall::Situation { text: "fetch_all sequential order".into(), session: Some("s1".into()), ..Default::default() };
    anchor::recall::recall(&db, &sit).unwrap();
    std::fs::write(&f, "def fetch_all(urls, t):\n    out = []\n    for u in urls:\n        out.append(t(u))\n    return out\n").unwrap();
    let input = json!({"session_id": "s1", "cwd": d.path(), "tool_name": "Edit", "tool_input": {"file_path": f}});
    let out = anchor::hooks::handle(&db, "PostToolUse", &input).unwrap().to_string();
    let stale = |id: &str| -> i64 { db.conn.query_row("SELECT stale FROM nodes WHERE id=?1", [id], |r| r.get(0)).unwrap() };
    assert_eq!(stale(&dec), 0, "seen decision re-anchored silently");
    assert_eq!(stale(&fact), 1, "descriptive fact stays stale");
    assert!(out.contains(&fact) && !out.contains(&dec), "notice lists only the fact: {out}");
}

#[test]
fn report_counts_hook_activity() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join(".git")).unwrap();
    std::fs::write(d.path().join("pay.py"), "def pay(x):\n    return x\n").unwrap();
    let ok = Command::new(EXE)
        .args(["--root", d.path().to_str().unwrap(), "remember",
               "pay() in pay.py must round amounts to cents before returning", "--kind", "rule", "--about", "pay.py", "--origin", "user"])
        .status().unwrap().success();
    assert!(ok);
    hook("SessionStart", d.path(), json!({"session_id": "s1", "cwd": d.path(), "hook_event_name": "SessionStart"}));
    hook("UserPromptSubmit", d.path(), json!({"session_id": "s1", "cwd": d.path(), "hook_event_name": "UserPromptSubmit",
        "prompt": "change pay so it rounds amounts in pay.py"}));
    let o = Command::new(EXE).args(["--root", d.path().to_str().unwrap(), "report", "--json"]).output().unwrap();
    assert!(o.status.success());
    let r: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(r["sessions"], 1);
    assert_eq!(r["prompts"], 1);
    assert_eq!(r["context"]["prompts_with_injection"], 1, "the rule about pay.py should be shown: {r}");
    assert_eq!(r["knowledge"]["from_user"], 1);
    let text = Command::new(EXE).args(["--root", d.path().to_str().unwrap(), "report"]).output().unwrap();
    assert!(String::from_utf8_lossy(&text.stdout).contains("not proof it helped"));
}

#[test]
fn knowledge_travels_between_machines_through_git() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    for d in [a.path(), b.path()] {
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::write(d.join("feeds.py"), "def fetch_all(urls):\n    return [u for u in urls]\n").unwrap();
    }
    let cli = |root: &std::path::Path, args: &[&str]| {
        let o = Command::new(EXE).arg("--root").arg(root).args(args).output().unwrap();
        assert!(o.status.success(), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).to_string()
    };
    cli(a.path(), &["index"]);
    let out = cli(a.path(), &["remember", "fetch_all must stay sequential: the gateway bans parallel clients",
                              "--kind", "decision", "--about", "feeds.py", "--origin", "user"]);
    let old_id = serde_json::from_str::<serde_json::Value>(&out).unwrap()["id"].as_str().unwrap().to_string();
    cli(a.path(), &["sync", "--enable"]);
    let gi = std::fs::read_to_string(a.path().join(".anchor/.gitignore")).unwrap();
    assert!(gi.contains("!knowledge.jsonl"), "git must be allowed to track the shared file");
    let shared = std::fs::read_to_string(a.path().join(".anchor/knowledge.jsonl")).unwrap();
    assert!(shared.contains(&old_id) && shared.contains("\"about\":[\"file:feeds.py\"]"), "{shared}");

    // machine B: the file arrives with a pull; the next session imports it and anchors it to B's code
    std::fs::create_dir_all(b.path().join(".anchor")).unwrap();
    std::fs::write(b.path().join(".anchor/knowledge.jsonl"), &shared).unwrap();
    std::fs::write(b.path().join(".anchor/.gitignore"), &gi).unwrap();
    hook("SessionStart", b.path(), json!({"session_id": "b1", "cwd": b.path(), "hook_event_name": "SessionStart"}));
    cli(b.path(), &["index"]);
    let listed = cli(b.path(), &["list"]);
    assert!(listed.contains("must stay sequential"), "B must have A's decision: {listed}");
    let rec = cli(b.path(), &["recall", "make fetch_all faster", "--file", "feeds.py"]);
    assert!(rec.contains(&old_id), "B recalls it for the file: {rec}");
    assert!(!cli(b.path(), &["health"]).contains("\"stale\": 1"), "imported item anchored fresh on B");

    // B corrects it; the file is rewritten automatically; A pulls and sees the correction
    std::thread::sleep(std::time::Duration::from_millis(1100)); // updated_at has 1 s resolution
    cli(b.path(), &["remember", "fetch_all may use at most 2 parallel connections (gateway limit raised)",
                    "--kind", "decision", "--about", "feeds.py", "--origin", "user", "--supersedes", &old_id]);
    let shared_b = std::fs::read_to_string(b.path().join(".anchor/knowledge.jsonl")).unwrap();
    assert!(shared_b.contains("at most 2 parallel"), "B's write must update the shared file");
    std::fs::write(a.path().join(".anchor/knowledge.jsonl"), &shared_b).unwrap();
    let synced = cli(a.path(), &["sync"]);
    assert!(synced.contains("1 added") && synced.contains("1 updated"), "{synced}");
    let listed_a = cli(a.path(), &["list"]);
    assert!(listed_a.contains("at most 2 parallel") && !listed_a.contains("must stay sequential"),
            "A shows only the correction: {listed_a}");
}

#[test]
fn import_claude_md_into_linked_items() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join(".git")).unwrap();
    std::fs::create_dir_all(d.path().join("src")).unwrap();
    std::fs::write(d.path().join("src/feeds.py"), "def fetch_all(urls):\n    return list(urls)\n").unwrap();
    std::fs::write(d.path().join("src/exporter.py"), "BATCH_SIZE = 500\n\ndef export(xs):\n    return xs\n").unwrap();
    std::fs::write(d.path().join("CLAUDE.md"), "# Project\n\n## Rules\n\
        - Never parallelize `fetch_all` in src/feeds.py: the gateway bans parallel clients.\n\
        - We decided to keep BATCH_SIZE at 500 because the ledger rejects bigger payloads.\n\
        - Prefer small pure functions over classes.\n\n```sh\nmake test\n```\n\n| a | b |\n| - | - |\n").unwrap();
    let run = |args: &[&str]| {
        let o = Command::new(EXE).arg("--root").arg(d.path()).args(args).current_dir(d.path()).output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).to_string()
    };
    let dry = run(&["import", "--dry-run"]);
    assert!(dry.contains("would import 3 items (2 linked to code)"), "{dry}");
    assert!(run(&["list"]).trim().is_empty(), "dry run stores nothing");
    let out = run(&["import"]);
    assert!(out.contains("[rule] (fetch_all, src/feeds.py)"), "{out}");
    assert!(out.contains("[decision] (src/exporter.py)"), "constant resolved to its file: {out}");
    assert!(out.contains("[preference]"), "{out}");
    run(&["import"]); // idempotent: same text, same id
    assert_eq!(run(&["list"]).lines().count(), 3, "re-import must not duplicate");
    let rec = run(&["recall", "make fetch_all faster", "--file", "src/feeds.py"]);
    assert!(rec.contains("Never parallelize"), "{rec}");
}

#[test]
fn compaction_steers_summary_and_restores_working_set() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join(".git")).unwrap();
    std::fs::write(d.path().join("drain.py"), "def drain(jobs):\n    return len(jobs)\n").unwrap();
    let cli = |args: &[&str]| {
        let o = Command::new(EXE).arg("--root").arg(d.path()).args(args).output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).to_string()
    };
    let ev = |name: &str, extra: serde_json::Value| {
        let mut v = json!({"session_id": "s1", "cwd": d.path(), "hook_event_name": name});
        v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        hook(name, d.path(), v).0
    };
    cli(&["index"]);
    ev("SessionStart", json!({"source": "startup"}));
    std::thread::sleep(std::time::Duration::from_millis(1100)); // created_at has 1 s resolution
    let old = cli(&["remember", "Progress messages in workers must use log.info", "--kind", "rule", "--about", "drain.py", "--origin", "user"]);
    let old_id = serde_json::from_str::<serde_json::Value>(&old).unwrap()["id"].as_str().unwrap().to_string();
    let new = cli(&["remember", "Progress messages in workers must use log.debug with the prefix [fleet]", "--kind", "rule",
                    "--about", "drain.py", "--origin", "user", "--supersedes", &old_id]);
    let new_id = serde_json::from_str::<serde_json::Value>(&new).unwrap()["id"].as_str().unwrap().to_string();

    let steer = ev("PreCompact", json!({"trigger": "auto", "custom_instructions": null}));
    assert!(!steer.trim_start().starts_with('{'), "PreCompact prints plain text: {steer}");
    assert!(steer.contains(&new_id) && steer.contains("log.debug with the prefix [fleet]"), "{steer}");
    assert!(!steer.contains(&old_id), "the superseded rule is not offered as current: {steer}");

    let restored = ev("SessionStart", json!({"source": "compact"}));
    let ctx: serde_json::Value = serde_json::from_str(restored.trim()).unwrap();
    let text = ctx["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
    assert!(text.contains("restored verbatim after compaction") && text.contains("log.debug with the prefix [fleet]"), "{text}");
    assert!(text.contains("replaces the earlier") && text.contains("log.info"), "says what it replaced: {text}");

    let normal = ev("SessionStart", json!({"source": "startup"}));
    assert!(!normal.contains("restored verbatim"), "no restore block on a normal start");
}
