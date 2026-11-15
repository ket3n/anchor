//! Register Anchor in ONE project (never global): .claude/settings.json hooks + .mcp.json.

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::Path;

const EVENTS: &[(&str, Option<&str>)] = &[
    ("SessionStart", None),
    ("UserPromptSubmit", None),
    ("PreToolUse", Some("*")),
    ("PostToolUse", Some("*")),
    ("Stop", None),
    ("PreCompact", None),
    ("SessionEnd", None),
];

fn read_json(p: &Path) -> Result<Value> {
    if p.exists() {
        let s = std::fs::read_to_string(p)?;
        if s.trim().is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_str(&s).with_context(|| format!("parsing {}", p.display()))
    } else {
        Ok(json!({}))
    }
}

fn is_anchor(h: &Value) -> bool {
    h.get("hooks")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().any(|x| x.get("command").and_then(|c| c.as_str()).map(|c| c.contains(" hook ") && c.contains("anchor")).unwrap_or(false)))
        .unwrap_or(false)
}

const CMD_MARK: &str = "<!-- installed by anchor -->";
const CMD_ORCHESTRATE: &str = "---
description: Orchestrate other Claude sessions with Anchor's orchestration memory
argument-hint: [goal | end <outcome>]
---
<!-- installed by anchor -->
You are orchestrating other sessions for the user, using Anchor's orchestration memory.

1. If the arguments start with `end`, call the `orchestration_log` tool with kind `end` and the rest as the outcome, confirm in one line, and stop.
2. Otherwise call `orchestration_start` with goal `$ARGUMENTS` (if that is empty, ask the user for the goal in one line first), plus the ids of the sessions involved and the repositories, if you know them.
3. If it lists similar past orchestrations, show them in one line each and ask the user whether to resume one or start new; then call it again with `resume` or `new: true`.
4. Follow the briefing it returns, including its authority rules. Log decisions, delegations and the outcome with `orchestration_log` as you go.
";
const CMD_PATTERNS: &str = "---
description: Review the orchestration patterns Anchor has learned about how you manage sessions
---
<!-- installed by anchor -->
Call the `pattern` tool with op `list`. Show the patterns grouped by CONFIRMED and INFERRED, one line each with its id.
Ask the user which INFERRED patterns to confirm or reject. Apply only what the user explicitly says, with op `review`.
";

/// Install /orchestrate and /patterns into a Claude commands folder (never overwrites a user's own file).
pub fn install_commands(dir: &Path) -> Result<Vec<String>> {
    std::fs::create_dir_all(dir)?;
    let mut out = vec![];
    for (name, body) in [("orchestrate.md", CMD_ORCHESTRATE), ("patterns.md", CMD_PATTERNS)] {
        let p = dir.join(name);
        let ours = std::fs::read_to_string(&p).map(|c| c.contains(CMD_MARK)).unwrap_or(true);
        if ours {
            std::fs::write(&p, body)?;
            out.push(p.display().to_string());
        }
    }
    Ok(out)
}

pub fn uninstall_commands(dir: &Path) {
    for name in ["orchestrate.md", "patterns.md"] {
        let p = dir.join(name);
        if std::fs::read_to_string(&p).is_ok_and(|c| c.contains(CMD_MARK)) {
            let _ = std::fs::remove_file(p);
        }
    }
}

pub fn install(project: &Path, exe: &Path) -> Result<Vec<String>> {
    let mut changed = Vec::new();
    let claude_dir = project.join(".claude");
    std::fs::create_dir_all(&claude_dir)?;
    let settings_path = claude_dir.join("settings.json");
    let mut settings = read_json(&settings_path)?;
    let exe_s = exe.display().to_string();
    {
        let hooks = settings.as_object_mut().unwrap().entry("hooks").or_insert(json!({}));
        for (ev, matcher) in EVENTS {
            let arr = hooks.as_object_mut().unwrap().entry(*ev).or_insert(json!([]));
            let a = arr.as_array_mut().unwrap();
            a.retain(|h| !is_anchor(h));
            let mut group = json!({"hooks": [{"type": "command", "command": format!("\"{exe_s}\" hook {ev}"), "timeout": 30}]});
            if let Some(m) = matcher {
                group["matcher"] = json!(m);
            }
            a.push(group);
        }
    }
    {
        let obj = settings.as_object_mut().unwrap();
        let en = obj.entry("enabledMcpjsonServers").or_insert(json!([]));
        if let Some(a) = en.as_array_mut()
            && !a.iter().any(|x| x == "anchor") {
                a.push(json!("anchor"));
            }
    }
    std::fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;
    changed.push(settings_path.display().to_string());

    let mcp_path = project.join(".mcp.json");
    let mut mcp = read_json(&mcp_path)?;
    mcp.as_object_mut()
        .unwrap()
        .entry("mcpServers")
        .or_insert(json!({}))
        .as_object_mut()
        .unwrap()
        .insert("anchor".into(), json!({"command": exe_s, "args": ["mcp"]}));
    std::fs::write(&mcp_path, serde_json::to_string_pretty(&mcp)?)?;
    changed.push(mcp_path.display().to_string());
    changed.extend(install_commands(&claude_dir.join("commands"))?);
    Ok(changed)
}

pub fn uninstall(project: &Path) -> Result<()> {
    uninstall_commands(&project.join(".claude").join("commands"));
    let settings_path = project.join(".claude").join("settings.json");
    if settings_path.exists() {
        let mut settings = read_json(&settings_path)?;
        if let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) {
            for (_, arr) in hooks.iter_mut() {
                if let Some(a) = arr.as_array_mut() {
                    a.retain(|h| !is_anchor(h));
                }
            }
            hooks.retain(|_, v| v.as_array().map(|a| !a.is_empty()).unwrap_or(true));
        }
        if let Some(a) = settings.get_mut("enabledMcpjsonServers").and_then(|v| v.as_array_mut()) {
            a.retain(|x| x != "anchor");
        }
        std::fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;
    }
    let mcp_path = project.join(".mcp.json");
    if mcp_path.exists() {
        let mut mcp = read_json(&mcp_path)?;
        if let Some(s) = mcp.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
            s.remove("anchor");
        }
        std::fs::write(&mcp_path, serde_json::to_string_pretty(&mcp)?)?;
    }
    Ok(())
}

fn user_settings_path() -> Result<std::path::PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(std::path::PathBuf::from(home).join(".claude").join("settings.json"))
}

pub fn add_hooks(settings: &mut Value, exe: &str) {
    let hooks = settings.as_object_mut().unwrap().entry("hooks").or_insert(json!({}));
    for (ev, matcher) in EVENTS {
        let arr = hooks.as_object_mut().unwrap().entry(*ev).or_insert(json!([]));
        let a = arr.as_array_mut().unwrap();
        a.retain(|h| !is_anchor(h));
        let mut group = json!({"hooks": [{"type": "command", "command": format!("\"{exe}\" hook {ev}"), "timeout": 30}]});
        if let Some(m) = matcher {
            group["matcher"] = json!(m);
        }
        a.push(group);
    }
}

/// Global install: hooks in ~/.claude/settings.json (backed up first) + user-scope MCP server.
/// Anchor stays inactive outside git repos, so this never creates stores elsewhere.
pub fn install_global(exe: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let path = user_settings_path()?;
    if path.exists() {
        let backup = path.with_file_name(format!("settings.json.anchor-backup-{}", crate::db::now()));
        std::fs::copy(&path, &backup)?;
        out.push(format!("backup {}", backup.display()));
    }
    let mut settings = read_json(&path)?;
    add_hooks(&mut settings, &exe.display().to_string());
    std::fs::write(&path, serde_json::to_string_pretty(&settings)?)?;
    out.push(format!("updated {}", path.display()));
    let have = std::process::Command::new("claude").args(["mcp", "get", "anchor"]).output();
    let exists = have.map(|o| o.status.success()).unwrap_or(false);
    if !exists {
        let st = std::process::Command::new("claude")
            .args(["mcp", "add", "--scope", "user", "anchor", "--"])
            .arg(exe)
            .arg("mcp")
            .output()
            .context("running `claude mcp add`")?;
        if !st.status.success() {
            anyhow::bail!("claude mcp add failed: {}", String::from_utf8_lossy(&st.stderr));
        }
        out.push("registered MCP server `anchor` (user scope)".into());
    } else {
        out.push("MCP server `anchor` already registered".into());
    }
    let cmds = user_settings_path()?.with_file_name("commands");
    for f in install_commands(&cmds)? {
        out.push(format!("installed command {f}"));
    }
    Ok(out)
}

pub fn uninstall_global() -> Result<Vec<String>> {
    let mut out = Vec::new();
    uninstall_commands(&user_settings_path()?.with_file_name("commands"));
    let path = user_settings_path()?;
    if path.exists() {
        let mut settings = read_json(&path)?;
        if let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) {
            for (_, arr) in hooks.iter_mut() {
                if let Some(a) = arr.as_array_mut() {
                    a.retain(|h| !is_anchor(h));
                }
            }
            hooks.retain(|_, v| v.as_array().map(|a| !a.is_empty()).unwrap_or(true));
        }
        std::fs::write(&path, serde_json::to_string_pretty(&settings)?)?;
        out.push(format!("removed hooks from {}", path.display()));
    }
    let _ = std::process::Command::new("claude").args(["mcp", "remove", "--scope", "user", "anchor"]).output();
    out.push("removed MCP server `anchor` (user scope)".into());
    Ok(out)
}
