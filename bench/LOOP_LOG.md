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
