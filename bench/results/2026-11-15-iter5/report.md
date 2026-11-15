# Anchor comparison benchmark

80 agent sessions (64 graded runs), Claude cost $7.95.


## Claude Code, Sonnet


### DEC-04 keep a decision; 400-note knowledge base, target at a random position

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Mean cost |
| --- | --- | --- | --- | --- | --- |
| Claude native memory (baseline) | 4/4 (100%) | 51%–100% | 1.000 | 156k | $0.117 |
| claude-mem | 0/4 (0%) | 0%–49% | 0.029 | 183k | $0.117 |
| Mem0 (platform) | 4/4 (100%) | 51%–100% | 1.000 | 105k | $0.095 |
| Anchor | 4/4 (100%) | 51%–100% | — | 128k | $0.097 |

### STALE-04 quick question answered from memory; stale fact; 400 notes

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Mean cost |
| --- | --- | --- | --- | --- | --- |
| Claude native memory (baseline) | 4/4 (100%) | 51%–100% | 1.000 | 80k | $0.088 |
| claude-mem | 4/4 (100%) | 51%–100% | 1.000 | 65k | $0.063 |
| Mem0 (platform) | 4/4 (100%) | 51%–100% | 1.000 | 40k | $0.036 |
| Anchor | 4/4 (100%) | 51%–100% | — | 69k | $0.047 |

### COR-05 rule set, corrected, then an unrelated session; session 4 must follow the correction; 400 notes

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Rule captured in session 1 | Mean cost |
| --- | --- | --- | --- | --- | --- | --- |
| Claude native memory (baseline) | 1/4 (25%) | 5%–70% | 0.143 | 106k | — | $0.407 |
| claude-mem | 4/4 (100%) | 51%–100% | 1.000 | 77k | — | $0.302 |
| Mem0 (platform) | 4/4 (100%) | 51%–100% | 1.000 | 59k | — | $0.209 |
| Anchor | 4/4 (100%) | 51%–100% | — | 67k | — | $0.206 |

### NEU-01 memory irrelevant (harm check); 40 distractor notes

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Mean cost |
| --- | --- | --- | --- | --- | --- |
| Claude native memory (baseline) | 4/4 (100%) | 51%–100% | 1.000 | 79k | $0.061 |
| claude-mem | 4/4 (100%) | 51%–100% | 1.000 | 68k | $0.060 |
| Mem0 (platform) | 4/4 (100%) | 51%–100% | 1.000 | 53k | $0.041 |
| Anchor | 4/4 (100%) | 51%–100% | — | 54k | $0.043 |

Success = hidden check passed (DEC-02: get_many never called concurrently; COR-01: new module logs via `log` and has no print(); STALE-01: docstring states the current 3 attempts, not the stale 5). p = two-sided Fisher exact test of Anchor vs that arm. COR-01 cost includes both sessions; tokens are session 2.

