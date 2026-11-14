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
