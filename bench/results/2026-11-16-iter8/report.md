# Anchor comparison benchmark

40 agent sessions (20 graded runs), Claude cost $3.68.


## Claude Code, Sonnet


### ORCH-01 orchestration decisions + a worker report, then a NEW session must answer two requests

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Rule captured in session 1 | Mean cost |
| --- | --- | --- | --- | --- | --- | --- |
| No memory (native auto-memory off) | 0/4 (0%) | 0%–49% | 0.029 | 72k | — | $0.137 |
| Claude native memory (baseline) | 0/4 (0%) | 0%–49% | 0.029 | 80k | — | $0.215 |
| CLAUDE.md (hand-written) | 0/4 (0%) | 0%–49% | 0.029 | 64k | — | $0.192 |
| Mem0 (platform) | 4/4 (100%) | 51%–100% | 1.000 | 48k | — | $0.159 |
| Anchor | 4/4 (100%) | 51%–100% | — | 66k | — | $0.217 |

Success = hidden check passed (DEC-02: get_many never called concurrently; COR-01: new module logs via `log` and has no print(); STALE-01: docstring states the current 3 attempts, not the stale 5). p = two-sided Fisher exact test of Anchor vs that arm. COR-01 cost includes both sessions; tokens are session 2.

