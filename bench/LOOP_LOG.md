# Improvement loop log

Autonomous loop (user-authorized 2026-11-14): benchmark → analyze → improve Anchor → re-benchmark,
pausing when the Claude quota runs out and resuming after reset. Each entry: what ran, what was learned,
what changed. Results live in bench/results/.

## Iteration 1 — benchmark v2 baseline (started 2026-11-14 ~20:00)
- Run dir: /tmp/claude-1000/-home-ketan/1985f893-3261-4bcf-89dd-d5811ca527f4/scratchpad/v2run
- Arms M,B,C1,C2,E × cases dec03,cor03,stale03,neu01 × 4 runs. Quota guard at 90%.
- Status: RUNNING
- 20:06 quota guard stopped at 91% session usage; 51/80 results. Waiting for reset (00:09).
- Partial result (51/80 runs, 20:06): all arms near ceiling except C1 (official MCP memory: cor03 1/3,
  dec03 2/3). E ties M everywhere (dec03 3/3 vs 3/3, cor03 3/3 vs 3/3, stale03 2/2 vs 3/3, neu01 2/2 vs 2/2).
  Tokens: E 165k vs M 128k on dec03 (worse), 56k vs 61k on cor03 (better). No harm on neu01 for any arm.
  Interpretation: with 41 notes, native memory puts everything in context and that is enough; v2 does not
  separate Anchor from native memory. Finish the v2 runs for the record, then iteration 2 must test where
  native memory should break, without rigging it:
  1. Scale: 400-note knowledge base (native memory loads only the MEMORY.md index, truncated after ~200 lines;
     the relevant note sits beyond the cut in a realistic order: notes appended over time).
  2. STALE-04: quick question answered from memory without editing code ("what batch size does the billing
     exporter use?"): native memory has no invalidation; Anchor flags stale.
  3. Cost: dec03 shows E spends more tokens than M; investigate what Anchor injects there (events.jsonl) and trim.
- 20:37 tick: waiting for quota (session 92%, week 63%; resets 00:10). Note for ticks: check the benchmark with `pgrep -af "python3 run.py"` — `pgrep -f "run.py /tmp/claude"` matches its own shell and falsely reports RUNNING.
- 00:37 quota reset; resumed v2 (29 runs left) with guard at 90%.

## Iteration 2 — prepared during iteration 1 (not yet run)
- Finding from v2 events.jsonl: Anchor's prompt hook injected 22 items (~1,100 tokens) on the DEC-03 prompt;
  21 were distractors sharing the word "partner". Fix: relative cutoff (>= 0.75 of best match) and max 5
  knowledge items per hook; explicit recall keeps 0.5. ENG-12 added: 1 item, target first, 48 tokens.
  Built in target-dev so the running benchmark keeps the iteration-1 binary; committed, not yet pushed/installed.
- New cases (added, existing cases untouched; v2 distractor set verified identical):
  - DEC-04: DEC-03 task with a 400-note knowledge base; the target's position is drawn uniformly at random
    per run (seed 1000+run) so no arm is favoured by placement.
  - STALE-04: quick question answered from memory, no code change ("what batch size / how many tries?");
    graded on the answer: must state 200 and 2; mentioning 500 only counts against an arm when nothing in the
    answer marks it as outdated (same grader for all arms; grader unit-checked on 5 phrasings).
- Plan: after v2 completes, install the new binary, run iteration 2 = dec04, stale04, dec03 (re-check tokens),
  neu01 across M,B,C1,C2,E × 4 runs.
- Iteration 1 COMPLETE (80/80). Results: bench/results/2026-11-15-iter1-v2.
  Success: E 16/16 = M 16/16 = B 16/16 = C2 16/16; C1 12/16 (cor03 1/4, dec03 3/4). No harm on neu01.
  E vs M cost per task: cor03 $0.087 vs $0.108, neu01 $0.049 vs $0.060, stale03 $0.054 vs $0.064,
  dec03 $0.106 vs $0.102 (E 167k vs M 129k tokens: the over-injection fixed for iteration 2).
  Verdict: on 41-note knowledge bases Anchor matches native memory's success and is cheaper on 3/4 tasks.
- 00:41 Iteration 2 STARTED: dec04,stale04,dec03,neu01 × M,B,C1,C2,E × 4 (binary with precision fix). Out: scratchpad/iter2
- Iteration 2 COMPLETE (80/80). Results: bench/results/2026-11-15-iter2.
  Success saturated for every arm except C1 (dec03 3/4, dec04 3/4): even with 400 notes and a stale
  question, Sonnet checks the code. Grader fix: two STALE-04 answers (M run 0, C2 run 1) were correct but
  said the stored note was "out of date" — a phrase missing from the contrast list. False negative, not a
  strictness change; all stale04 rows re-graded with the same grader for every arm.
  Cost E vs M: stale04 $0.047 vs $0.087 (-46%), dec04 $0.109 vs $0.120 (-9%), neu01 $0.042 vs $0.056 (-25%),
  dec03 $0.110 vs $0.099 (+11%). B (CLAUDE.md) is the most expensive at 400 notes (dec04 $0.168, 246k tokens).
- Improvement: on dec03 the prompt injection was already 1 item/96 tokens, but after each edit to feeds.py
  the PostToolUse staleness notice made the agent call review (E 14-17 turns vs M 7-13). Fix: a prescriptive
  item (decision/rule/preference) already shown in the session is re-anchored silently; facts and unseen
  items still get the notice. Integration test added.

## Iteration 3
- Cases: cor04 (NEW: user changes a rule in session 2; session 3 must follow the new one), dec03, dec04
  (re-measure cost after the re-anchor fix). Arms M,B,C1,C2,E × 4.
- Status: NOT STARTED — waiting for quota (session 83% at 01:2x, resets 05:29). Start with:
  `cd ~/projects/anchor/bench/v2 && nohup python3 run.py /tmp/claude-1000/-home-ketan/1985f893-3261-4bcf-89dd-d5811ca527f4/scratchpad/iter3 --runs 4 --parallel 3 --cases cor04,dec03,dec04 > /tmp/claude-1000/-home-ketan/1985f893-3261-4bcf-89dd-d5811ca527f4/scratchpad/iter3.log 2>&1 &`
  then arm the quota guard (90%).
- 02:37 waiting for quota (session 85%, resets 05:30; week reset to 0%).
- 04:37 waiting for quota (session 86%, resets 05:29).
- 06:37 Iteration 3 STARTED (quota reset to 0%). Out: scratchpad/iter3, 60 runs planned.
- Quota guard now runs detached (nohup, log scratchpad/guard3.log); Monitor expiry had killed the previous one. 29/60 done, early E vs M: dec03 $0.089 vs $0.113, dec04 $0.082 vs $0.118, cor04 $0.036 vs $0.052 (partial).
- Iteration 3 COMPLETE (60/60). Results: bench/results/2026-11-15-iter3.
  Success: E 12/12. M: dec03 4/4, dec04 4/4, cor04 3/4. B cor04 3/4. C1 dec03 2/4, dec04 3/4. C2 12/12.
  Failures checked by hand, none are harness errors:
  - cor04 M run 2: native memory rewrote the rule file to log.debug but left the MEMORY.md index line
    saying log.info; session 3 followed the index and wrote log.info.
  - cor04 B run 2: the session-2 agent never updated CLAUDE.md, which still said log.info.
  - Anchor: in all 4 runs the old rule got superseded_by=<new id>; session 3 was injected only the new rule.
  Cost E vs M: dec03 $0.087 vs $0.098 (-11%; was +11% in iter2, so the silent re-anchor fix worked),
  dec04 $0.091 vs $0.135 (-33%), cor04 $0.153 vs $0.195 (-22%). E has the fewest input tokens on dec03 and dec04.
  No case where E loses or ties M at higher cost, so no Anchor change this iteration.
- Harness: NEW case COR-05 (harder; same final task and checker as COR-04). 400-note knowledge base,
  4 sessions: rule set, rule corrected, an unrelated billing session with its own new rule, then the drain task.
  Why: success is saturated; COR-04 exposed stale-memory failures in M and B, and a longer gap after the
  correction plus a bigger knowledge base tests that update path harder. It applies equally to every arm.
  Also fixed notes_for: large cases without a target no longer insert None.
- Iteration 4 STARTED: cor05 × M,B,C1,C2,E × 4 (80 sessions). Out: scratchpad/iter4. Quota guard at 90%, detached.
- Iteration 4 RESTARTED with a stricter COR-05. In the first attempt, B run 0 passed although its CLAUDE.md
  held no logging rule at all: an agent picks log.debug unprompted often enough, so "debug not info" does not
  tell memory from luck. COR-05's correction now also requires every progress message to start with "[fleet] "
  (cannot be guessed); new checker hidden/check_cor05.py, self-tested on 2 passing and 2 failing samples.
  The 5 runs from the first attempt are discarded (scratchpad/iter4-discarded), and the rule applies to every arm.
  COR-04 is unchanged so earlier results stay comparable; its results should be read as an upper bound.
- 10:37 iter4 paused by quota guard at 10/20 (session 100% at 08:45, resets 11:29; week 9%). Resume with the same iter4 command + detached guard.
- 11:53 iter4 RESUMED at 10/20 (session reset to 5%). Partial cor05: E 2/2, M 1/2, B 1/2, C1 2/2, C2 0/2.
- 12:0x Claude Doc report updated (rev 17): new section 'Comparison benchmark results (v2)' with iter1-4 success, E vs M cost, failure analysis, Anchor changes and benchmark corrections.
- Iteration 4 COMPLETE (20/20). Results: bench/results/2026-11-15-iter4 (COR-05, 400 notes, 4 sessions).
  Success: E 4/4, C1 4/4, B 2/4, C2 2/4, M 1/4 (E vs M Fisher p=0.14). Cost: E $0.209 (lowest), C2 $0.232,
  C1 $0.269, M $0.447, B $0.485. E also used the fewest input tokens (63k vs M 120k).
  Every failure wrote log.info (the superseded rule). Checked by hand, all genuine:
  - M runs 0 and 2: the rule file was rewritten to log.debug, but the MEMORY.md index line still said log.info;
    M run 3 never updated it.
  - B runs 1 and 3: CLAUDE.md still held the log.info line.
  - C2 runs 0 and 1: neither rule was in basic-memory's notes; the agent used the old rule from built-in memory.
  First case where memory tools clearly differ. No Anchor change needed from this iteration.
- NEW arm C3 = claude-mem 15.18.4 (github.com/thedotmack/claude-mem, Claude Code plugin; user approved adding it).
  Isolation: loaded per session via --plugin-dir from scratchpad/ext; its own data dir (.cmdata), worker port and
  settings per run; telemetry off (DO_NOT_TRACK=1, CLAUDE_MEM_TELEMETRY=0); auto-update and cloud sync off.
  Its search MCP server is added to the run's MCP config, because --strict-mcp-config drops plugin MCP servers.
  Notes seeded through its own POST /api/memory/save (the API its save_memory tool uses). Its background
  observer runs on Haiku; those tokens are logged and priced into the arm's cost (c3_observer_cost_usd).
  Side effects outside the scratchpad: bun 1.4.4 installed via mise (global mise config unchanged; hooks get
  MISE_BUN_VERSION), and the claude-mem launcher binary at ~/.local/bin/claude-mem (hard-coded location),
  to be removed when the C3 runs are done. The early smoke runs had telemetry on.
  Smoke test (built-in memory off): claude-mem captured the rule, but stored it as "a CLAUDE.md rule exists",
  so session 2 looked for a CLAUDE.md, found none, and did not apply the rule.
- Iteration 5 STARTED: arms C3,E,M × cases dec04,cor05,stale04,neu01 × 4 (E and M re-run alongside so the
  comparison is from the same time). Out: scratchpad/iter5.
- NEW arm C4 = Mem0 (official Claude Code plugin 0.3.3 from github.com/mem0ai/mem0, Mem0 Platform; the user supplied
  a platform API key, kept in a private scratchpad file, never in the repo or logs). Loaded per session via
  --plugin-dir; its MCP server is added to the run's config (dropped by --strict-mcp-config otherwise); telemetry off.
  Each run gets its own Mem0 user_id and the project namespace the plugin itself derives (computed with its own
  code), notes are seeded verbatim (infer=False, 20 per call), the harness waits 90 s between sessions for the
  plugin's background flush and cloud extraction, and the run's memories are deleted from Mem0 afterwards.
  Local plugin store: ~/.claude/plugins/data/mem0-inline (keyed by session and repo), to be removed after the runs.
  E2E check on COR-03: passed; session 1's rule was flushed and extracted, and session 2 was injected it at rank 2.
- Iteration 5 Mem0 runs STARTED in parallel with iteration 5 (same cases, same window): out scratchpad/iter5-c4.
- 13:2x session quota hit 95% (guard stopped iter5 at 26/48). Mem0 runs were started by mistake after the cap: stopped within a minute, their 2 half-seeded runs deleted from Mem0 and from disk. Resume both after the 16:50 reset: iter5 (C3,E,M) and iter5-c4 (C4).
  Resume commands (S = the scratchpad dir):
  `cd ~/projects/anchor/bench/v2 && CM_PLUGIN=$S/ext/claude-mem/plugin CM_BUN=$(mise where bun@latest)/bin nohup python3 run.py $S/iter5 --runs 4 --parallel 3 --cases dec04,cor05,stale04,neu01 --arms C3,E,M >> $S/iter5.log 2>&1 &`
  `cd ~/projects/anchor/bench/v2 && M0_PLUGIN=$S/ext/mem0/integrations/claude-code-plugin M0_KEYFILE=$S/secrets/mem0.key nohup python3 run.py $S/iter5-c4 --runs 4 --parallel 2 --cases dec04,cor05,stale04,neu01 --arms C4 >> $S/iter5-c4.log 2>&1 &`
  then the detached quota guard. Check quota first.
- 14:37 waiting for quota (97%). 17:02 RESUMED iter5 (26/48) and iter5-c4 (0/16) after reset (session 24%, week 23%); guard detached.
- Feature: `anchor report [--days N] [--json]`: what Anchor did in a repo (sessions, context shown and its
  token size, most-shown items, guardrail warnings/blocks, stale flags, corrections superseded). Integration test added.
  Added README (install on any machine, usage, limits, results), EVALUATING.md (how to judge Anchor on your
  own work), bench/ab/ab.py (A/B runner for any repo: none vs CLAUDE.md vs Anchor; smoke-tested: none 0/1,
  CLAUDE.md 1/1, Anchor 1/1), and a release workflow (Linux x86_64 + macOS arm64 binaries on v* tags).
- Improvement candidate found with `anchor report` on cor05-E-0: 7 edits to queue.py flagged 37 items stale,
  because every note about a file goes stale on any edit to that file. Noisy in busy files: next, narrow
  file-level staleness (e.g. only when the symbols a note names change, or when the note's words match changed code).
- Improvement (from `anchor report`): precise file-level staleness. A note linked to a whole file now goes stale
  only when what it names changes: the named symbols' AST hashes plus the non-comment lines mentioning other
  identifiers it names (DEFAULT_LIMIT, ...). A note naming nothing in the file goes stale on any logic change only
  if it is a `fact`; rules, decisions, preferences, gotchas and notes keep (they guide future work). A deleted file
  still stales everything. Old anchors upgrade in place. ENG-13 added; ENG-03 (whole-file facts, constants,
  comment edits) still passes. Replaying the COR-05 queue.py edits on the 400-note base: 17 stale flags -> 0
  (none of those notes names the changed code). Built at 17:24, after iteration 5's Anchor runs (last 17:07),
  so iteration 5 used the previous binary throughout.
- v0.2.0 features (tests added, all green):
  - `anchor sync [--enable]`: knowledge shared through git as .anchor/knowledge.jsonl (one item per line, sorted;
    ids are content hashes so they match across machines). Auto-written on remember/review when enabled,
    imported at SessionStart when it changed (e.g. after a pull); later updated_at wins per item; code anchors
    are recomputed per checkout. Two-machine integration test (decision travels A->B, correction travels B->A).
  - `anchor import [FILE...] [--dry-run]`: CLAUDE.md / AGENTS.md / docs -> items (bullets and short paragraphs;
    code blocks and tables skipped), kind guessed from wording, linked to mentioned files, symbols and constants
    (a constant resolves to the single file containing it). Origin user. Idempotent. Integration test added.
  - The benchmark binary (target/release/anchor) is now v0.2.0: iterations from 6 on run with it.
- v0.2.1: fix "database is locked" when two Anchor processes write at once (e.g. the background first index
  started by SessionStart and a CLI command): write transactions are now BEGIN IMMEDIATE, so they wait on the
  busy timeout instead of failing. Found by the two-machine test failing in the pre-push hook. Tag v0.2.0 had
  been pushed before the fix (its commit was not on main); it is left as is, and v0.2.1 is the release to use.
- 17:27 quota guard stopped iter5-c4 at 15/16 (session 92%); the unfinished run's Mem0 memories deleted and its dir removed. Resume with the iter5-c4 command after the 21:50 reset.

## Next (user approved; start after the 21:50 quota reset)
1. Resume iter5-c4 (1 run left, cor05-C4-3) with its resume command above; then analyze iteration 5 in full
   (C3, C4, E, M) into bench/results/2026-11-15-iter5 and log it. Partial so far: Mem0 ties Anchor on success
   (DEC-04 4/4, STALE-04 4/4, COR-05 3/3, NEU-01 4/4), slightly cheaper on 3 tasks; STALE-04 tokens Mem0 40k vs E 69k.
   Note that Mem0's cloud extraction cost is not visible to the harness. claude-mem DEC-04 0/4 (p=0.03 vs E).
2. Memory-aware compaction ("post-compact restore"):
   a. Check Claude Code's PreCompact / SessionStart(source=compact) hook contracts (claude-code-guide agent or docs):
      what input they get, whether output is injected, and whether the summary can be influenced.
   b. Implement: on SessionStart with source=compact, re-inject a small working set verbatim: user rules in force,
      corrections made this session (superseded items), items shown this session that are still fresh, stale
      warnings, and key knowledge for the files edited this session. Budget about 500 tokens. Tests.
   c. New benchmark case COMPACT-01: rule + correction early in a long session, forced compaction, then a task
      that depends on them. Arms M, B, C4, E. Grade with a hidden check like COR-05's.
3. Investigate why E uses ~70% more input tokens than Mem0 on STALE-04 (events.jsonl, transcripts).
- 18:37 waiting for quota (session 99%, resets 21:49; week 31%).
- 22:14 quota reset (session 4%, week 32%). Resumed iter5-c4 (15/16). Starting the compaction work.
- STALE-04 token gap explained (E ~70k vs C4 ~35k input tokens): Anchor's injection is small (249 tokens, 5 items).
  The extra cost is behaviour: E sees the fact flagged STALE, greps the code (as C4 does), then calls `remember`
  to correct the stored fact (+1 turn, +1 more when the MCP tool schema must be loaded via ToolSearch). C4 answers
  and leaves its stale memory wrong for the next session. A one-session case shows only that cost, not the payoff.
  Proposed STALE-05: same stale fact, session 2 asks again without code access hints; measures whether the
  correction pays off (cost and correctness over both sessions). Not changing Anchor's behaviour for this.
- Feature: memory-aware compaction (src/compact.rs). Contract checked against the Claude Code hook docs: PreCompact
  stdout becomes instructions for the compaction summary; SessionStart(source=compact) output is added to the
  compacted context; no hook can replace the summary. Anchor now (1) steers the summary to keep this session's
  rules and corrections in their current form, and (2) re-injects the working set verbatim after compaction:
  items stored this session (corrections first, with "replaces the earlier ...") , the user's rules in force,
  items shown this session that are still fresh, knowledge about the files edited; about 500 tokens.
  Integration test added. Headless `/compact` works with `claude -p "/compact" --resume <id>` (verified).
- NEW case COMPACT-01: one conversation resumed turn by turn: rule (log.info), correction (log.debug + "[fleet] "
  prefix), unrelated billing work with its own rule, an unrelated health task, `/compact`, then the drain task.
  Graded with check_cor05 (strict). The harness records whether the transcript really compacted.
  Smoke run E: success, compacted=true, 8 items steered and restored (correction and Decimal rule first), $0.68.
- Iteration 6 STARTED: compact01 × E,M,B × 4 (scratchpad/iter6) and × C4 × 4 (scratchpad/iter6-c4); Anchor v0.2.1+compaction.
  Resume: `cd ~/projects/anchor/bench/v2 && nohup python3 run.py $S/iter6 --runs 4 --parallel 2 --cases compact01 --arms E,M,B >> $S/iter6.log 2>&1 &`
  and `M0_PLUGIN=$S/ext/mem0/integrations/claude-code-plugin M0_KEYFILE=$S/secrets/mem0.key nohup python3 run.py $S/iter6-c4 --runs 4 --parallel 1 --cases compact01 --arms C4 >> $S/iter6-c4.log 2>&1 &`
  Check `compacted` is true in every graded row; a run that did not compact is a harness error.
- Iteration 5 COMPLETE (C3,E,M 48/48; C4 16/16). Results: bench/results/2026-11-15-iter5 (merged).
  Success: E 16/16, C4 16/16, M 13/16 (COR-05 1/4), C3 12/16 (DEC-04 0/4, p=0.03 vs E).
  Cost E vs C4: DEC-04 $0.097 vs $0.095, COR-05 $0.206 vs $0.209, STALE-04 $0.047 vs $0.036, NEU-01 $0.043 vs $0.041.
  Mem0 ties Anchor on success and cost except STALE-04 (explained above). Mem0's cloud extraction cost is not counted.
- Iteration 6 partial (14/16): COMPACT-01 saturated: E 4/4, M 4/4, B 4/4, C4 2/2, every run compacted=true. Claude's own
  compaction summary kept the correction in all arms, so this case cannot show the restore's value. Cost: E $0.716,
  C4 $0.735, B $0.825, M $1.000 (E -28% vs M). Next: a harder variant (more work between correction and compaction,
  two compactions, correction mentioned once and briefly) before claiming anything about the restore.
- Iteration 6 COMPLETE (16/16, every run compacted): COMPACT-01 saturated, E 4/4 $0.716, C4 4/4 $0.737, B 4/4 $0.825,
  M 4/4 $1.000. Results: bench/results/2026-11-15-iter6. No claim about the restore from this case.
- NEW case COMPACT-02 (harder): correction stated once, in passing, inside a rename request; then billing work, a health
  task, /compact, a turn reading every parsing/notify module, a shard function, /compact again, then the drain task.
  Same strict check (check_cor05). Rows record `compactions` (expect 2). Why: COMPACT-01 showed Claude's own summary
  keeps a correction that was stated prominently and recently; this tests a brief, older correction across two summaries.
  Housekeeping: do not clean ~/.claude/projects while compaction runs are active (the harness reads those transcripts).
- Iteration 7 STARTED: compact02 × E,M,B × 4 (scratchpad/iter7) and × C4 × 4 (scratchpad/iter7-c4).
  Resume: same commands as iteration 6 with iter7 / iter7-c4 and `--cases compact02`.
- 04:37 (Nov 16) quota guard had stopped iter7 at 11/16 (E,M,B) and 2/16 (C4) at 22:49 (session 95%). Session 82% now,
  resets 07:59: benchmarks resume after that.
- v0.3.0 feature: orchestration memory (src/orch.rs, user-level ~/.anchor/orchestration.db, created only on first use).
  Episodes (goal, linked sessions, decisions, delegations, worker reports, outcome), situation matching (shared
  sessions/repos + goal words) that offers resume instead of starting over, briefing at orchestrator start/resume/
  compaction, PreCompact steering, automatic delegation logging from SendMessage, worker reports at Stop, evidence
  from the user's prompts in orchestrated sessions, patterns (INFERRED until the user confirms; superseded/rejected
  never shown), authority rules in every briefing, a one-time nudge when a normal session starts messaging others.
  MCP: orchestration_start / _context / _log / pattern (work outside projects). Commands /orchestrate and /patterns
  installed by `anchor install`. CLI `anchor orch`. Session binding: the PostToolUse hook of orchestration_start
  (MCP calls carry no session id). Tests: tests/orchestration.rs (5) + integration/engine all green.
- Benchmark binary pinned: run.py honours ANCHOR_BIN. Iteration 7 resumes with the pre-orchestration build at
  $S/anchor-iter7/target/release/anchor (commit c8a979b) so the whole iteration uses one binary.
  Resume iter7: `ANCHOR_BIN=$S/anchor-iter7/target/release/anchor nohup python3 run.py $S/iter7 --runs 4 --parallel 2 --cases compact02 --arms E,M,B >> $S/iter7.log 2>&1 &`
  and the C4 command with iter7-c4 (C4 does not use the Anchor binary). Clean unfinished C4 dirs (Mem0 cleanup) first.
- Next benchmark: ORCH-01 (orchestrator delegates, is compacted or replaced, must continue correctly; and replay of
  user decisions to measure agreement with confirmed patterns).
- 05:41 waiting for quota (session 94%, resets 08:00; week 53%).
- 06:37 waiting for quota (session 95%, resets 08:00).
- 08:02 quota reset (session 0%, week 53%). Cleaned unfinished iter7 runs; resumed iter7 (pinned binary) and iter7-c4; guard detached.
- Iteration 7 COMPLETE (E,M,B 12/12; C4 4/4; every run compacted twice): COMPACT-02 saturated too. E $1.798,
  C4 $1.744, B $1.913, M $2.254. Results: bench/results/2026-11-16-iter7. Conclusion: with Claude's built-in memory
  on in every arm (it records the correction mid-session) plus Claude's own summary, compaction cases cannot
  separate the tools. Stopping compaction benchmarks; the restore stays (cheap, verified by tests).
- NEW case ORCH-01: orchestration conversation (decisions incl. an unguessable one: docs MAY push changelog
  straight to main; a relayed api report; unrelated work), then a NEW session must answer WEB: GO|WAIT and
  DOCS: ALLOW|DENY (correct GO + ALLOW). Graded by regex on the final answer (self-tested). Per-run ANCHOR_HOME.
  NEW control arm A0 (built-in memory off) measures guessability.
- Anchor fix found by the ORCH-01 smoke run (E failed: Claude never called orchestration_start, because the tool
  said "only on explicit request" and "you are coordinating three worker sessions for me" did not count):
  (1) orchestration_start's description now counts a request in any words as the permission; (2) a word-based
  intent hint at UserPromptSubmit (once per session); (3) when similar episodes exist, orchestration_start also
  returns the closest one's state. Test added (tests/orchestration.rs, 6 tests). Smoke re-run: E success, $0.19.
- v0.3.1. Iteration 8 STARTED: orch01 × A0,M,B,E × 4 (scratchpad/iter8, ANCHOR_BIN=$S/bin/anchor-iter8) and
  × C4 × 4 (scratchpad/iter8-c4).
- Iteration 8 COMPLETE (A0,M,B,E 16/16; C4 4/4). Results: bench/results/2026-11-16-iter8 (ORCH-01).
  E 4/4 ($0.217), C4 4/4 ($0.159), M 0/4 ($0.215), B 0/4 ($0.192), A0 0/4 ($0.137); E vs M/B/A0 Fisher p=0.029.
  Failures are genuine: M's built-in memory saved no orchestration decision (only the seeded notes); B's CLAUDE.md was
  never updated (the agent said it found no record); all three gave the cautious guess WAIT/DENY. Mem0 passes by
  capturing the conversation automatically; Anchor through orchestration memory (episode bound, decisions and the
  relayed report logged, closest-episode state shown to the new session).
  E costs 37% more than C4 here (66k vs 48k input tokens: the orchestration_start / log calls and their schemas).
- Next: ORCH-02 (harder): a decision is reversed later in the conversation (docs push permission revoked) and a
  second, unrelated orchestration exists in the same project. Tests whether stale decisions are kept apart.
  Known Anchor gap for it: orchestration entries have no supersede; a reversed decision shows both. Fix first.
- v0.3.2: orchestration entries can be superseded (`orchestration_log` with `supersedes: <#n>`); briefings number
  entries and hide superseded ones; old stores migrate (ALTER TABLE). Test added (7 orchestration tests).
- NEW case ORCH-02: ORCH-01 plus a later reversal (docs may NOT push to main any more) and a second, unrelated
  orchestration (billing workers) in the same project; the new session must answer WEB: GO and DOCS: DENY.
- Iteration 9 STARTED: orch02 × A0,M,B,E × 4 (scratchpad/iter9, ANCHOR_BIN=$S/bin/anchor-iter9) and × C4 × 4 (iter9-c4).
- Iteration 9 COMPLETE (20/20, ORCH-02). Results: bench/results/2026-11-16-iter9. E 4/4 ($0.421), C4 4/4 ($0.254),
  M 0/4 ($0.361), B 0/4 ($0.321), A0 0/4 ($0.234); E vs M/B/A0 p=0.029. All failures WAIT/DENY (no memory of the
  api report). Anchor costs 66% more than Mem0: every user message cost an extra orchestration_log turn, and the
  new session needed 4 turns (ToolSearch + context lookups) against Mem0's 1.
- v0.3.3 (cost): in an orchestrator session the user's messages are recorded verbatim (kind "user", oldest first,
  "later messages override earlier ones"), the first request is read from the transcript at binding, and each
  recorded message is acknowledged so Claude does not log it again; a coordination request in a new session gets
  the closest episode's state injected directly (no tool round trip). Matching counts user entries and normalises
  by at most 6 terms. Test added (8 orchestration tests). Smoke ORCH-02: $0.42 -> $0.36, final session 4 turns ->
  1 ($0.038 -> $0.025), orchestration_log calls 6 -> 3.
- Iteration 10 STARTED: orch02 × E × 4 (ANCHOR_BIN=$S/bin/anchor-iter10, scratchpad/iter10) and × C4 × 4
  (scratchpad/iter10-c4), same window. M/B/A0 not re-run: 0/4 on ORCH-01 and ORCH-02, nothing they use changed.
- Iteration 10 COMPLETE (ORCH-02, v0.3.3): E 4/4 $0.405 (final session 18k tokens, below C4's 22k), C4 4/4 $0.280.
  The smoke run's $0.36 was optimistic; the gap is now in the orchestration turns (start call + ToolSearch + logs).
- Change (uncommitted until measured): a plain-words coordination request with no similar episode now starts the
  episode in the UserPromptSubmit hook and binds the session (the request is the permission; ANCHOR_ORCH_AUTOSTART=0
  turns it off); a similar episode still gets an offer to resume. Tests updated (8 orchestration tests green).
- Iteration 11 STARTED: orch02 × E × 4 (ANCHOR_BIN=$S/bin/anchor-iter11, scratchpad/iter11). Compared with iter10's
  C4 (same case, ~20 minutes earlier, very stable: $0.276-0.286) to save quota.
- Iteration 11 COMPLETE (ORCH-02, E only, auto-start): E 4/4, mean $0.360 (runs 0.296-0.406) vs $0.405 in iter10
  (-11%); C4 reference $0.280. Run 0 made no orchestration tool call at all ($0.296); the rest still log some of
  Claude's own decisions. Auto-start kept: v0.3.4. Results: bench/results/2026-11-16-iter11.
  Summary of orchestration so far: on ORCH-01/02 only Anchor and Mem0 carry orchestration state to a new session
  (both 4/4 vs 0/4 for built-in memory, CLAUDE.md, none); Mem0 stays ~22% cheaper, partly because its cloud
  extraction model is not billed to the session.
- Claude Doc report updated (rev 18): 'Results update: iterations 5-11' (project tasks with Mem0/claude-mem, compaction saturated, orchestration results, v0.2.0-v0.3.4 changes).
- 10:37 session 71%, week 60% (5.5 days left: pace runs). NEW case ORCH-03 (pattern learning): in three separate
  sessions a worker reports "done" and the user each time pushes back the same way (run the full test suite and
  paste the summary), never stated as a rule; a new session gets a different worker's "done" and must reply on a
  REPLY: line asking for the full test suite and its summary (grader self-tested). Arms A0, M, E, C4 (B left out:
  CLAUDE.md was never updated in the ORCH cases).
- Iteration 12 STARTED: orch03 × A0,M,E × 4 (ANCHOR_BIN=$S/bin/anchor-iter12 = v0.3.4, scratchpad/iter12) and
  × C4 × 4 (scratchpad/iter12-c4).
- 10:41 quota guard stopped iter12 at 90% (session jumped 71%->90%). Partial ORCH-03: E 1/1 (asked for full test suite + summary), M 0/1, A0 0/2, C4 0/0. Unfinished runs cleaned (Mem0 memories deleted). Resume after the 13:00 reset with the iter12 commands (ANCHOR_BIN=$S/bin/anchor-iter12).
- 14:37 quota reset (session 12%, week 65%). Resumed iter12 (A0,M,E and C4) with guard.
- Iteration 12 COMPLETE (ORCH-03, pattern learning). Results: bench/results/2026-11-16-iter12.
  E 4/4 ($0.312), C4 1/4 ($0.223), M 0/4 ($0.305), A0 0/4 ($0.214). E vs M/A0 p=0.029, E vs C4 p=0.14.
  E applied the unstated habit every time ("not accepting yet; run the full test suite and paste the summary"),
  from the closest episode's recorded user messages injected at the new session's prompt. Mem0 mostly surfaced
  unrelated stored notes (DEFAULT_LIMIT, lowercased keys); one C4 miss (run 0) asked for "unit and e2e test results"
  and a change summary: partial, failed by the strict grader (same rule for every arm). M and A0 just accepted "done".
- Iteration 13 STARTED: more power for E vs C4 on ORCH-03: runs 4-7 for E and C4 in the same out dirs
  (`--runs 8 --arms E` in iter12 with ANCHOR_BIN=$S/bin/anchor-iter12; `--runs 8 --arms C4` in iter12-c4).
- Iteration 13 COMPLETE (ORCH-03 runs 4-7 for E and C4): totals E 8/8 ($0.297), C4 3/8 ($0.226); E vs C4 p=0.026,
  E vs M/A0 p=0.002. First significant win over Mem0: Anchor applies a habit the user showed but never stated.
- Cost analysis ORCH-03 (E $0.297 vs C4 $0.226): E's extra turns are orchestration_log calls restating the
  instruction the user relays (already recorded verbatim), and in session 3 ToolSearch + orchestration_context
  because Anchor only suggested resuming. Matching scores for the follow-up sessions were 0.83 and 1.33; an
  unrelated goal started a new episode.
- v0.3.5 (uncommitted until measured): a coordination request matching an OPEN episode at >= 0.8 continues it
  automatically (session bound, message recorded, briefing injected; `new: true` escape hatch); closed or weaker
  matches are only offered. The acknowledgement says instructions to workers in the user's message count as
  delegations already. Tests updated (8 orchestration tests green).
- Iteration 14 STARTED: orch03 and orch02 × E × 4 (ANCHOR_BIN=$S/bin/anchor-iter14, scratchpad/iter14).
- Iteration 14 COMPLETE (v0.3.5 candidate, E only): ORCH-03 4/4 mean $0.320 (was $0.297), ORCH-02 4/4 mean $0.393
  (was $0.360). Accuracy held, cost went UP: auto-resume injects the full briefing into each follow-up session, more
  tokens than the tool calls it saves. Reverted (not committed); Anchor stays at v0.3.4.
  Remaining cost gap vs Mem0 on ORCH cases (~25-30%) is mostly Claude's own orchestration_log calls; next idea to
  test: a compact briefing (decisions + last user messages only) for follow-up sessions instead of the full one.
- Housekeeping: /tmp (tmpfs) was filled by 12G of benchmark run dirs, causing a 'disk I/O error' in an engine test at pre-push. Deleted analysed run dirs (results.jsonl kept; summaries are in bench/results), removed the iter7 worktree: scratchpad 1.3G, /tmp 14G free. Delete run dirs after each analysed iteration from now on.
