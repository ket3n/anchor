# Anchor comparison benchmark

40 agent sessions (20 graded runs), Claude cost $6.57.


## Claude Code, Sonnet


### COR-05 rule set, corrected, then an unrelated session; session 4 must follow the correction; 400 notes

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Rule captured in session 1 | Mean cost |
| --- | --- | --- | --- | --- | --- | --- |
| Claude native memory (baseline) | 1/4 (25%) | 5%–70% | 0.143 | 120k | — | $0.447 |
| CLAUDE.md (hand-written) | 2/4 (50%) | 15%–85% | 0.429 | 121k | — | $0.485 |
| Official MCP memory server | 4/4 (100%) | 51%–100% | 1.000 | 99k | — | $0.269 |
| basic-memory | 2/4 (50%) | 15%–85% | 0.429 | 66k | — | $0.232 |
| Anchor | 4/4 (100%) | 51%–100% | — | 63k | — | $0.209 |

Success = hidden check passed (DEC-02: get_many never called concurrently; COR-01: new module logs via `log` and has no print(); STALE-01: docstring states the current 3 attempts, not the stale 5). p = two-sided Fisher exact test of Anchor vs that arm. COR-01 cost includes both sessions; tokens are session 2.

