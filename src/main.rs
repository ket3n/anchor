use anchor::db::{Db, find_root, project_root};
use anchor::knowledge::{self, RememberInput};
use anchor::recall::{self, Situation};
use anchor::{hooks, index, install, mcp};
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::io::Read;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "anchor", version, about = "Agent-native project knowledge engine (no model inside)")]
struct Cli {
    /// Project root (default: auto-detect)
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create .anchor/ and index the project
    Init,
    /// Index (incremental unless --force)
    Index {
        #[arg(long)]
        force: bool,
    },
    /// Recall knowledge for a situation
    Recall {
        query: Vec<String>,
        #[arg(long)]
        file: Vec<String>,
        #[arg(long)]
        symbol: Vec<String>,
        #[arg(long, default_value_t = 1500)]
        budget: usize,
        #[arg(long, default_value = "all")]
        scope: String,
        #[arg(long)]
        json: bool,
    },
    /// Store knowledge
    Remember {
        content: String,
        #[arg(long, default_value = "note")]
        kind: String,
        #[arg(long)]
        about: Vec<String>,
        #[arg(long)]
        tag: Vec<String>,
        #[arg(long, default_value = "agent")]
        origin: String,
        #[arg(long)]
        supersedes: Option<String>,
        #[arg(long)]
        strict: bool,
        /// JSON check spec, e.g. '{"forbid":"unwrap\\(","paths":"src/handlers/**"}'
        #[arg(long)]
        check: Option<String>,
    },
    /// Review an item: verified|approve|broken|merge|forget|used|ignored
    Review {
        id: String,
        verdict: String,
        #[arg(long)]
        into: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// List knowledge items
    List {
        #[arg(long)]
        all: bool,
    },
    /// Counts: stale, disputed, quarantined, ...
    Health,
    /// Claude Code hook entry point (reads hook JSON on stdin)
    Hook { event: String },
    /// Run the MCP server on stdio
    Mcp,
    /// Register hooks + MCP server in one project (.claude/settings.json, .mcp.json)
    Install {
        #[arg(long)]
        project: Option<PathBuf>,
        /// Install for every Claude Code session (user settings + user-scope MCP)
        #[arg(long)]
        global: bool,
    },
    /// Remove Anchor's hooks + MCP entry from a project
    Uninstall {
        #[arg(long)]
        project: Option<PathBuf>,
        #[arg(long)]
        global: bool,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("anchor: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?;
    let root = cli.root.clone().unwrap_or_else(|| find_root(&cwd));
    match cli.cmd {
        Cmd::Hook { event } => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            let input: serde_json::Value = serde_json::from_str(&buf).unwrap_or(serde_json::Value::Null);
            // hooks must never break the agent: on error, log to stderr and exit 0
            let start = std::env::var("CLAUDE_PROJECT_DIR")
                .ok()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .or_else(|| input.get("cwd").and_then(|v| v.as_str()).map(PathBuf::from))
                .unwrap_or(cwd.clone());
            let Some(root) = cli.root.clone().or_else(|| project_root(&start)) else {
                return Ok(()); // not inside a project: stay silent, create nothing
            };
            match Db::open(&root).and_then(|db| hooks::handle(&db, &event, &input)) {
                Ok(v) if !v.is_null() => println!("{v}"),
                Ok(_) => {}
                Err(e) => eprintln!("anchor hook {event}: {e:#}"),
            }
            Ok(())
        }
        Cmd::Mcp => {
            let active = cli.root.clone().or_else(|| project_root(&cwd));
            mcp::serve(active)
        }
        Cmd::Init | Cmd::Index { .. } => {
            let force = matches!(cli.cmd, Cmd::Index { force: true });
            let db = Db::open(&root)?;
            let t = std::time::Instant::now();
            let r = index::index_all(&db, force)?;
            println!(
                "indexed {} of {} files, {} symbols, {} changed, {} removed, {} knowledge item(s) newly stale ({} ms) in {}",
                r.files_indexed, r.files_seen, r.symbols, r.changed_symbols.len(), r.removed_symbols.len(),
                r.newly_stale.len(), t.elapsed().as_millis(), root.display()
            );
            Ok(())
        }
        Cmd::Recall { query, file, symbol, budget, scope, json } => {
            let db = Db::open(&root)?;
            let res = recall::recall(
                &db,
                &Situation { text: query.join(" "), files: file, symbols: symbol, budget: Some(budget), scope: Some(scope), ..Default::default() },
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&res)?);
            } else if res.items.is_empty() {
                println!("(nothing relevant)");
            } else {
                println!("{}", recall::render(&res, "Recall result"));
                println!("-- {} items, ~{} tokens, {} candidates", res.items.len(), res.tokens, res.candidates);
            }
            Ok(())
        }
        Cmd::Remember { content, kind, about, tag, origin, supersedes, strict, check } => {
            let db = Db::open(&root)?;
            let check = match check {
                Some(c) => Some(serde_json::from_str(&c)?),
                None => None,
            };
            let v = knowledge::remember(
                &db,
                RememberInput { content, kind: Some(kind), about, tags: tag, origin: Some(origin), supersedes, strict, check, ..Default::default() },
            )?;
            println!("{}", serde_json::to_string_pretty(&v)?);
            Ok(())
        }
        Cmd::Review { id, verdict, into, note } => {
            let db = Db::open(&root)?;
            let v = knowledge::review(&db, &id, &verdict, into.as_deref(), note.as_deref())?;
            println!("{}", serde_json::to_string_pretty(&v)?);
            Ok(())
        }
        Cmd::List { all } => {
            let db = Db::open(&root)?;
            for v in knowledge::list(&db, all)? {
                println!("{}", v);
            }
            Ok(())
        }
        Cmd::Health => {
            let db = Db::open(&root)?;
            println!("{}", serde_json::to_string_pretty(&knowledge::health(&db)?)?);
            Ok(())
        }
        Cmd::Install { global: true, .. } => {
            let exe = std::env::current_exe()?;
            for l in install::install_global(&exe)? {
                println!("{l}");
            }
            println!("Anchor is now active in every git repository you open with Claude Code (ANCHOR_DISABLE=1 turns it off).");
            Ok(())
        }
        Cmd::Install { project, .. } => {
            let p = project.unwrap_or(root);
            let exe = std::env::current_exe()?;
            for f in install::install(&p, &exe)? {
                println!("updated {f}");
            }
            let db = Db::open(&p)?;
            let r = index::index_all(&db, false)?;
            println!("indexed {} files, {} symbols", r.files_indexed, r.symbols);
            Ok(())
        }
        Cmd::Uninstall { global: true, .. } => {
            for l in install::uninstall_global()? {
                println!("{l}");
            }
            Ok(())
        }
        Cmd::Uninstall { project, .. } => {
            install::uninstall(&project.unwrap_or(root))?;
            println!("removed Anchor hooks and MCP entry");
            Ok(())
        }
    }
}
