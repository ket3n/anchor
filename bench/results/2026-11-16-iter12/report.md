# Anchor comparison benchmark

32 agent sessions (16 graded runs), Claude cost $4.22.


## Claude Code, Sonnet


### ORCH-03 the user's habit shown 3 times in 3 sessions, never stated; a new session must apply it

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Rule captured in session 1 | Mean cost |
| --- | --- | --- | --- | --- | --- | --- |
| No memory (native auto-memory off) | 0/4 (0%) | 0%–49% | 0.029 | 16k | — | $0.214 |
| Claude native memory (baseline) | 0/4 (0%) | 0%–49% | 0.029 | 20k | — | $0.305 |
| Mem0 (platform) | 1/4 (25%) | 5%–70% | 0.143 | 17k | — | $0.223 |
| Anchor | 4/4 (100%) | 51%–100% | — | 18k | — | $0.312 |

Success = hidden check passed (DEC-02: get_many never called concurrently; COR-01: new module logs via `log` and has no print(); STALE-01: docstring states the current 3 attempts, not the stale 5). p = two-sided Fisher exact test of Anchor vs that arm. COR-01 cost includes both sessions; tokens are session 2.

