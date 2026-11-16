# Anchor comparison benchmark

8 agent sessions (4 graded runs), Claude cost $1.44.


## Claude Code, Sonnet


### ORCH-02 like ORCH-01, but the docs permission is revoked later and a second orchestration exists

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Rule captured in session 1 | Mean cost |
| --- | --- | --- | --- | --- | --- | --- |
| Anchor | 4/4 (100%) | 51%–100% | — | 18k | — | $0.360 |

Success = hidden check passed (DEC-02: get_many never called concurrently; COR-01: new module logs via `log` and has no print(); STALE-01: docstring states the current 3 attempts, not the stale 5). p = two-sided Fisher exact test of Anchor vs that arm. COR-01 cost includes both sessions; tokens are session 2.

