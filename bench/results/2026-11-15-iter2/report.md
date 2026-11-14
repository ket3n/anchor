# Anchor comparison benchmark

80 agent sessions (80 graded runs), Claude cost $7.09.


## Claude Code, Sonnet


### DEC-03 keep a decision; prompt never names the code; 40 distractor notes

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Mean cost |
| --- | --- | --- | --- | --- | --- |
| Claude native memory (baseline) | 4/4 (100%) | 51%–100% | 1.000 | 139k | $0.099 |
| CLAUDE.md (hand-written) | 4/4 (100%) | 51%–100% | 1.000 | 125k | $0.098 |
| Official MCP memory server | 3/4 (75%) | 30%–95% | 1.000 | 147k | $0.111 |
| basic-memory | 4/4 (100%) | 51%–100% | 1.000 | 146k | $0.126 |
| Anchor | 4/4 (100%) | 51%–100% | — | 166k | $0.110 |

### DEC-04 keep a decision; 400-note knowledge base, target at a random position

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Mean cost |
| --- | --- | --- | --- | --- | --- |
| Claude native memory (baseline) | 4/4 (100%) | 51%–100% | 1.000 | 156k | $0.120 |
| CLAUDE.md (hand-written) | 4/4 (100%) | 51%–100% | 1.000 | 247k | $0.168 |
| Official MCP memory server | 3/4 (75%) | 30%–95% | 1.000 | 147k | $0.108 |
| basic-memory | 4/4 (100%) | 51%–100% | 1.000 | 166k | $0.133 |
| Anchor | 4/4 (100%) | 51%–100% | — | 169k | $0.109 |

### STALE-04 quick question answered from memory; stale fact; 400 notes

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Mean cost |
| --- | --- | --- | --- | --- | --- |
| Claude native memory (baseline) | 4/4 (100%) | 51%–100% | 1.000 | 73k | $0.087 |
| CLAUDE.md (hand-written) | 4/4 (100%) | 51%–100% | 1.000 | 77k | $0.106 |
| Official MCP memory server | 4/4 (100%) | 51%–100% | 1.000 | 69k | $0.057 |
| basic-memory | 4/4 (100%) | 51%–100% | 1.000 | 59k | $0.054 |
| Anchor | 4/4 (100%) | 51%–100% | — | 69k | $0.047 |

### NEU-01 memory irrelevant (harm check); 40 distractor notes

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Mean cost |
| --- | --- | --- | --- | --- | --- |
| Claude native memory (baseline) | 4/4 (100%) | 51%–100% | 1.000 | 72k | $0.056 |
| CLAUDE.md (hand-written) | 4/4 (100%) | 51%–100% | 1.000 | 57k | $0.047 |
| Official MCP memory server | 4/4 (100%) | 51%–100% | 1.000 | 74k | $0.052 |
| basic-memory | 4/4 (100%) | 51%–100% | 1.000 | 59k | $0.044 |
| Anchor | 4/4 (100%) | 51%–100% | — | 54k | $0.042 |

Success = hidden check passed (DEC-02: get_many never called concurrently; COR-01: new module logs via `log` and has no print(); STALE-01: docstring states the current 3 attempts, not the stale 5). p = two-sided Fisher exact test of Anchor vs that arm. COR-01 cost includes both sessions; tokens are session 2.

