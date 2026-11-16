# Anchor comparison benchmark

24 agent sessions (12 graded runs), Claude cost $1.17.


## Claude Code, Sonnet


### STALE-05 STALE-04, then a new session asks again from memory only (does the correction pay off?)

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Rule captured in session 1 | Mean cost |
| --- | --- | --- | --- | --- | --- | --- |
| Claude native memory (baseline) | 0/4 (0%) | 0%–49% | 0.029 | 28k | — | $0.160 |
| Mem0 (platform) | 4/4 (100%) | 51%–100% | 1.000 | 17k | — | $0.058 |
| Anchor | 4/4 (100%) | 51%–100% | — | 17k | — | $0.074 |

Success = hidden check passed (DEC-02: get_many never called concurrently; COR-01: new module logs via `log` and has no print(); STALE-01: docstring states the current 3 attempts, not the stale 5). p = two-sided Fisher exact test of Anchor vs that arm. COR-01 cost includes both sessions; tokens are session 2.

