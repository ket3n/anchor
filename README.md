# Anchor

Agent-native project knowledge engine. **Rust remembers, computes and enforces; the host agent
(Claude Code, Codex, Cursor, a local-model agent) supplies meaning and judgment.** No model runs inside.

- Code index: tree-sitter symbols (Rust, Python, TypeScript/TSX, JavaScript), call graph, Markdown
  sections, config files, normalized AST hashes (formatting/comments don't count as changes).
- Knowledge: decisions, rules, preferences, gotchas, facts — linked to code with `about`, with
  provenance and trust (`user` > `verified` > `asserted` > `disputed`; external content `quarantined`).
- Invalidation: when linked code changes, knowledge goes `stale`; callers and `depends_on` items get a
  softer flag. `review … verified` re-anchors.
- Recall: seed (files/symbols named) → lexical (BM25 words + trigram fallback, any script) → graph
  spread (2 hops) → score (relevance, proximity, trust, utility, freshness) → dedupe → token budget.
- Guardrails: checks written as data (`{"forbid": regex, "require": regex, "paths": glob}`); only
  user-set `strict` rules block, everything else warns.
- Harnessing: hooks inject context and ask one checkpoint question per session; the agent decides.

## Build

```
cargo build --release          # binary: target/release/anchor
cargo test --release -- --nocapture --test-threads=1   # engine tests ENG-01..10
```

## Install

```
cargo build --release && cp target/release/anchor ~/.local/bin/anchor

# every Claude Code session (backs up ~/.claude/settings.json, adds a user-scope MCP server)
anchor install --global
anchor uninstall --global

# or one project only (.claude/settings.json + .mcp.json in that repo)
anchor install --project /path/to/repo
anchor uninstall --project /path/to/repo
```

Anchor only activates inside a git repository (or a folder with `.anchor/`); elsewhere hooks print
nothing and create nothing. `ANCHOR_DISABLE=1` turns it off everywhere.

CLI: `anchor init | index [--force] | recall <text> [--file f] [--symbol s] | remember <text> --kind decision --about <sym|file> [--origin user] [--check JSON] [--strict] | review <id> <verdict> | list | health`.

MCP tools (stdio, `anchor mcp`): `recall`, `remember`, `review`.

Hooks: SessionStart (index + rules briefing), UserPromptSubmit (recall → knowledge-only context;
`ANCHOR_PROMPT_SCOPE=all` adds related code), PreToolUse (guardrails + knowledge for the file being
edited), PostToolUse (re-index, staleness notice), Stop (optional checkpoint: `ANCHOR_CHECKPOINT=1`;
off by default — the pilot showed agents record corrections unprompted and the checkpoint cost a turn). Every event is logged to `.anchor/events.jsonl`
(phase-0 census log; stays local, includes prompt text).

Store: `<repo>/.anchor/anchor.db` (SQLite WAL; `.anchor/` is git-ignored by default).

## Benchmarks

`bench/run_bench.py` — pilot of DEC-01 (decision preservation) and COR-01 (correction retention),
arms A (plain), B (CLAUDE.md), E (Anchor). Headless Claude Code, scoped tool allowlist, user
settings excluded. Hidden checks in `bench/hidden/`.

Pilot results: [bench/results/2026-11-14-pilot.md](bench/results/2026-11-14-pilot.md).

## Known limits (v0.1)

No daemon yet (each hook/MCP process opens SQLite directly — measured fast enough); no vector
search; adapters for 4 code languages + Markdown/JSON/YAML/TOML; `check` supports regex only.
