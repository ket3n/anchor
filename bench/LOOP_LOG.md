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
