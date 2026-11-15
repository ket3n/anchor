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
| Notes describe code that has since changed | Memories are anchored to files and symbols (tree-sitter, normalized AST hashes). An edit flags the memories about that code as stale; callers get a softer flag. |
| A corrected rule lives on next to the new one | A correction is stored with `supersedes`, which retires the old item; superseded items are never recalled. |
| Whole memory files are loaded every session | Recall ranks by name, lexical and call-graph closeness, and injects only the few items that matter (typically 1–5, ~100–600 tokens). |
| The agent reads memory as instructions | Items carry provenance and trust (`user` > `verified` > `asserted` > `disputed`); external content is quarantined. |
| Rules are only advice | A rule can carry a check (regex `forbid` / `require` on paths). Rules you mark `--strict` block the edit; others warn. |

How it plugs in: Claude Code hooks (session start, each prompt, before and after edits) and an MCP server
with three tools: `recall`, `remember`, `review`.

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
```

Kinds are free text; Anchor treats `decision`, `rule`, `preference` and `fact` as prescriptive (shown before
edits, re-anchored quietly). To correct an item, store the new one with `--supersedes <old id>` (the agent
does this through MCP when you correct it); the old one is never recalled again.

### Where things are stored

`<repo>/.anchor/anchor.db` (SQLite) and `<repo>/.anchor/events.jsonl` (a local log of hook events,
which includes your prompt text; it rotates at 10 MB). `.anchor/` is git-ignored automatically.

**Memory is per machine.** Each clone builds its own index and memory. Copy `.anchor/anchor.db` to move
it; syncing across machines is not built in yet.

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
- File-level memories go stale on any edit to that file, which can be noisy in busy files.
- No team or multi-machine sync.
- Guardrail checks are regex only.

## Development

```sh
cargo build --release
cargo clippy --release --all-targets -- -D warnings
cargo test --release -- --test-threads=1
git config core.hooksPath .githooks   # pre-push runs clippy and the tests
```

Tagging `vX.Y.Z` builds release binaries (`.github/workflows/release.yml`).
