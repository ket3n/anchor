# Anchor comparison benchmark

40 agent sessions (20 graded runs), Claude cost $6.36.


## Claude Code, Sonnet


### ORCH-02 like ORCH-01, but the docs permission is revoked later and a second orchestration exists

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Rule captured in session 1 | Mean cost |
| --- | --- | --- | --- | --- | --- | --- |
| No memory (native auto-memory off) | 0/4 (0%) | 0%–49% | 0.029 | 69k | — | $0.234 |
| Claude native memory (baseline) | 0/4 (0%) | 0%–49% | 0.029 | 80k | — | $0.361 |
| CLAUDE.md (hand-written) | 0/4 (0%) | 0%–49% | 0.029 | 63k | — | $0.321 |
| Mem0 (platform) | 4/4 (100%) | 51%–100% | 1.000 | 22k | — | $0.254 |
| Anchor | 4/4 (100%) | 51%–100% | — | 70k | — | $0.421 |

Success = hidden check passed (DEC-02: get_many never called concurrently; COR-01: new module logs via `log` and has no print(); STALE-01: docstring states the current 3 attempts, not the stale 5). p = two-sided Fisher exact test of Anchor vs that arm. COR-01 cost includes both sessions; tokens are session 2.

