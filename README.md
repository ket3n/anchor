# Anchor

**Verified project memory for coding agents.** Anchor gives Claude Code (and any MCP client) a memory of
your project's decisions, rules and facts that is *linked to the code*: when the code a memory describes
changes, the memory is flagged stale instead of silently going wrong; when you correct a rule, the old
one is retired and never shown again.

Anchor runs no model. It is one Rust binary with a local SQLite store per repository. Nothing leaves
your machine, there is no API key, and it adds no model cost of its own.

> **Status: early (v0.1).** It works and has been benchmarked, but only on a synthetic repository with
> 4–12 runs per result. Treat the numbers below as promising, not proven, and judge it on your own work
> with [EVALUATING.md](EVALUATING.md).

## What it does

| Problem with plain memory | What Anchor does |
| --- | --- |
| Notes describe code that has since changed | Memories are anchored to files and symbols (tree-sitter, normalized AST hashes). A memory goes stale only when the code it names changes (a function's body, a constant's line); callers get a softer flag. Facts about a whole file go stale on any logic change to it. |
| A corrected rule lives on next to the new one | A correction is stored with `supersedes`, which retires the old item; superseded items are never recalled. |
| Whole memory files are loaded every session | Recall ranks by name, lexical and call-graph closeness, and injects only the few items that matter (typically 1–5, ~100–600 tokens). |
| The agent reads memory as instructions | Items carry provenance and trust (`user` > `verified` > `asserted` > `disputed`); external content is quarantined. |
| Rules are only advice | A rule can carry a check (regex `forbid` / `require` on paths). Rules you mark `--strict` block the edit; others warn. |

How it plugs in: Claude Code hooks (session start, each prompt, before and after edits, compaction) and an MCP
server with `recall`, `remember`, `review`, plus four orchestration tools (`orchestration_start`,
`orchestration_context`, `orchestration_log`, `pattern`).

## Install

### 1. Get the binary (each machine)

Prebuilt (Linux x86_64, macOS Apple Silicon) from the
[latest release](https://github.com/ket3n/anchor/releases/latest):

```sh
# Linux; use anchor-macos-arm64 on a Mac
curl -LO https://github.com/ket3n/anchor/releases/latest/download/anchor-linux-x86_64.tar.gz
tar -xzf anchor-linux-x86_64.tar.gz
mkdir -p ~/.local/bin && mv anchor ~/.local/bin/
```

Or build from source (Rust 1.85+; a C compiler is needed for tree-sitter and SQLite):

```sh
cargo install --git https://github.com/ket3n/anchor --locked
```

### 2. Turn it on for Claude Code

```sh
anchor install --global      # every repo you open with Claude Code
# or only one repository:
anchor install --project /path/to/repo
```

`--global` backs up `~/.claude/settings.json`, adds Anchor's hooks (your own hooks are kept), and
registers the `anchor` MCP server at user scope. Restart Claude Code afterwards.

Anchor only activates inside a git repository (or a folder with `.anchor/`). Elsewhere, including your
home folder, its hooks print nothing and create nothing.

### Turn it off or remove it

```sh
ANCHOR_DISABLE=1 claude            # one session without Anchor
anchor uninstall --global          # remove hooks and MCP entry
anchor uninstall --project /path/to/repo
```

## Using it

### Start from what you already have

```sh
cd /path/to/repo
anchor import --dry-run      # reads CLAUDE.md / AGENTS.md (or the files you name) and shows what it would store
anchor import                # stores each bullet or short paragraph as a rule / decision / preference / note,
                             # linked to the files, symbols and constants it mentions
```

You can keep your CLAUDE.md; importing adds linking, staleness and corrections on top.

### Day to day

Mostly you don't do anything: work as usual. The agent sees a short "Anchor project memory" block when
something relevant is stored, and saves decisions and rules with `remember` when you state them
("from now on…", "we decided…", "don't…").

You can also manage memory yourself, from inside the repository:

```sh
anchor remember "fetch_all must stay sequential: the gateway bans parallel clients" \
  --kind decision --about src/feeds.py --origin user
anchor remember "Money is summed with Decimal, never float" --kind rule --about src/billing \
  --origin user --check '{"forbid": "float\\(", "paths": "src/billing/**"}' --strict
anchor recall "make the nightly sync faster"      # what would be shown for this task
anchor list                                       # active items (--all includes superseded)
anchor review <id> verified|broken|forget|used|ignored
anchor health                                     # stale, disputed, quarantined counts
anchor report [--days 7] [--json]                 # what Anchor did in this repo (see EVALUATING.md)
anchor import [FILE...] [--dry-run]                # bring in CLAUDE.md / AGENTS.md / docs
anchor sync [--enable]                             # share knowledge through git
```

Kinds are free text; Anchor treats `decision`, `rule`, `preference` and `fact` as prescriptive (shown before
edits, re-anchored quietly). To correct an item, store the new one with `--supersedes <old id>` (the agent
does this through MCP when you correct it); the old one is never recalled again.

### Orchestrating other sessions

When one Claude session manages others for you, Anchor keeps the orchestration's memory outside that
session, so it survives compaction and restarts, and learns how *you* orchestrate.

```text
/orchestrate migrate auth to OAuth across api and web     # start, or resume a similar past one
/orchestrate end both migrated, tokens rotate daily        # close it with its outcome
/patterns                                                  # review what Anchor learned about how you manage sessions
```

- **Episodes** hold the goal, the sessions involved, decisions, delegations (the orchestrator's `SendMessage`
  calls are logged automatically), worker reports (files a linked worker edited, posted when it stops) and the
  outcome. After a compaction or in a fresh session, the orchestrator gets the state back; a similar later
  situation offers to resume instead of starting over.
- **Patterns** are how you handle recurring situations ("make a worker run the full test suite before it
  reports done"). Anchor collects your own prompts in orchestrated sessions as evidence; Claude distils
  patterns from it; they stay INFERRED until you confirm them with `/patterns` or `anchor orch confirm <id>`.
- **Authority:** the orchestrator acts on confirmed patterns only, treats inferred ones as suggestions, and
  asks you before anything new, risky or irreversible.
- Nothing changes for other sessions. A session that starts messaging other sessions is nudged once to ask
  you whether to use orchestration memory; it never switches on its own.
- Stored at user level in `~/.anchor/orchestration.db` (`ANCHOR_HOME` to move it), created the first time you
  orchestrate. `ANCHOR_ORCH_CATALOG=0` stops it from noting which sessions exist. CLI:
  `anchor orch [episodes | show <id> | patterns | confirm <id> | reject <id>]`.

### Where things are stored

`<repo>/.anchor/anchor.db` (SQLite) and `<repo>/.anchor/events.jsonl` (a local log of hook events,
which includes your prompt text; it rotates at 10 MB). `.anchor/` is git-ignored automatically.

### Same memory on several machines (or a team)

```sh
anchor sync --enable          # writes .anchor/knowledge.jsonl and lets git track it
git add .anchor/.gitignore .anchor/knowledge.jsonl && git commit -m "Share Anchor knowledge"
```

From then on every `remember`/`review` rewrites `knowledge.jsonl` (one item per line, sorted, so diffs are
readable), and each Claude Code session imports what arrived with a `git pull`. On another machine, install
Anchor and pull: that's all. Where both sides changed an item, the later edit wins; nothing is deleted.
`anchor sync` does the same by hand. Code anchors are not shared: each machine anchors items to its own
checkout, so staleness is always judged against the code you have.

> **Privacy:** a committed `knowledge.jsonl` is visible to everyone who can read the repository. In a public
> repo, your memories become public. Leave sharing off there, or review the file before committing.
> The database and `events.jsonl` (which holds prompt text) are never shared.

### Settings (environment variables)

| Variable | Effect |
| --- | --- |
| `ANCHOR_DISABLE=1` | Anchor does nothing (hooks and MCP are inert) |
| `ANCHOR_PROMPT_SCOPE=all` | At prompt time, inject related code as well as knowledge |
| `ANCHOR_PROMPT_BUDGET=<tokens>` | Token budget for prompt-time injection |
| `ANCHOR_CHECKPOINT=1` | At the end of a session with edits, ask the agent once whether anything is worth remembering |
| `ANCHOR_ROOT=<dir>` | Force the project root |

Languages indexed: Rust, Python, TypeScript/TSX, JavaScript, plus Markdown, JSON, YAML and TOML files.
In other languages, memories still work at file level.

## Benchmark results so far

Headless Claude Code (Sonnet), Claude's built-in memory on in every arm, every arm given the same notes,
4 runs per arm per task per iteration, strict hidden checks, every failure read by hand. Harness and raw
results are in [`bench/`](bench/) ([protocol](bench/PROTOCOL.md), [log](bench/LOOP_LOG.md)).

| Task | Anchor | Claude built-in memory | CLAUDE.md | MCP memory server | basic-memory |
| --- | --- | --- | --- | --- | --- |
| Keep a past decision, 400 notes (DEC-04) | 8/8 | 8/8 | 8/8 | 6/8 | 8/8 |
| User corrects a rule mid-way (COR-04) | 4/4 | 3/4 | 3/4 | 4/4 | 4/4 |
| Correction + gap session, 400 notes (COR-05) | 4/4 | 1/4 | 2/4 | 4/4 | 2/4 |
| Mean cost per run vs built-in memory | 11–53% cheaper on every task | — | — | — | — |

claude-mem and Mem0 are being added; see the log for their latest numbers. No single difference is
statistically significant yet (Fisher p > 0.1). The repository is synthetic; real-world value is what
[EVALUATING.md](EVALUATING.md) helps you measure.

## Known limits

- No embedding search: recall is lexical plus code structure, so a memory worded very differently from
  the task can be missed.
- Sync is file-based through git (last edit wins per item); there is no server or live sync.
- `import` guesses kinds from wording (must/never → rule, "we decided" → decision); check with `--dry-run`.
- Guardrail checks are regex only.

## Development

```sh
cargo build --release
cargo clippy --release --all-targets -- -D warnings
cargo test --release -- --test-threads=1
git config core.hooksPath .githooks   # pre-push runs clippy and the tests
```

Tagging `vX.Y.Z` builds release binaries (`.github/workflows/release.yml`).
