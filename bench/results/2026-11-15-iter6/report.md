# Anchor comparison benchmark

32 agent sessions (16 graded runs), Claude cost $13.11.


## Claude Code, Sonnet


### COMPACT-01 rule set, corrected, unrelated work, then /compact in the same conversation; 40 notes

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Rule captured in session 1 | Mean cost |
| --- | --- | --- | --- | --- | --- | --- |
| Claude native memory (baseline) | 4/4 (100%) | 51%–100% | 1.000 | 50k | — | $1.000 |
| CLAUDE.md (hand-written) | 4/4 (100%) | 51%–100% | 1.000 | 46k | — | $0.825 |
| Mem0 (platform) | 4/4 (100%) | 51%–100% | 1.000 | 44k | — | $0.737 |
| Anchor | 4/4 (100%) | 51%–100% | — | 47k | — | $0.716 |

Success = hidden check passed (DEC-02: get_many never called concurrently; COR-01: new module logs via `log` and has no print(); STALE-01: docstring states the current 3 attempts, not the stale 5). p = two-sided Fisher exact test of Anchor vs that arm. COR-01 cost includes both sessions; tokens are session 2.

