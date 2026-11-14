# Anchor comparison benchmark

100 agent sessions (80 graded runs), Claude cost $6.22.


## Claude Code, Sonnet


### DEC-03 keep a decision; prompt never names the code; 40 distractor notes

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Mean cost |
| --- | --- | --- | --- | --- | --- |
| Claude native memory (baseline) | 4/4 (100%) | 51%–100% | 1.000 | 130k | $0.102 |
| CLAUDE.md (hand-written) | 4/4 (100%) | 51%–100% | 1.000 | 165k | $0.106 |
| Official MCP memory server | 3/4 (75%) | 30%–95% | 1.000 | 100k | $0.083 |
| basic-memory | 4/4 (100%) | 51%–100% | 1.000 | 141k | $0.118 |
| Anchor | 4/4 (100%) | 51%–100% | — | 168k | $0.106 |

### COR-03 rule from an earlier session, invisible in code; 40 distractor notes

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Rule captured in session 1 | Mean cost |
| --- | --- | --- | --- | --- | --- | --- |
| Claude native memory (baseline) | 4/4 (100%) | 51%–100% | 1.000 | 56k | — | $0.108 |
| CLAUDE.md (hand-written) | 4/4 (100%) | 51%–100% | 1.000 | 62k | — | $0.095 |
| Official MCP memory server | 1/4 (25%) | 5%–70% | 0.143 | 77k | — | $0.098 |
| basic-memory | 4/4 (100%) | 51%–100% | 1.000 | 75k | — | $0.090 |
| Anchor | 4/4 (100%) | 51%–100% | — | 57k | — | $0.087 |

### STALE-03 outdated fact; task answerable from memory alone; 40 distractor notes

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Mean cost |
| --- | --- | --- | --- | --- | --- |
| Claude native memory (baseline) | 4/4 (100%) | 51%–100% | 1.000 | 69k | $0.064 |
| CLAUDE.md (hand-written) | 4/4 (100%) | 51%–100% | 1.000 | 78k | $0.056 |
| Official MCP memory server | 4/4 (100%) | 51%–100% | 1.000 | 102k | $0.071 |
| basic-memory | 4/4 (100%) | 51%–100% | 1.000 | 81k | $0.061 |
| Anchor | 4/4 (100%) | 51%–100% | — | 62k | $0.054 |

### NEU-01 memory irrelevant (harm check); 40 distractor notes

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Mean cost |
| --- | --- | --- | --- | --- | --- |
| Claude native memory (baseline) | 4/4 (100%) | 51%–100% | 1.000 | 78k | $0.060 |
| CLAUDE.md (hand-written) | 4/4 (100%) | 51%–100% | 1.000 | 57k | $0.047 |
| Official MCP memory server | 4/4 (100%) | 51%–100% | 1.000 | 71k | $0.058 |
| basic-memory | 4/4 (100%) | 51%–100% | 1.000 | 54k | $0.044 |
| Anchor | 4/4 (100%) | 51%–100% | — | 58k | $0.049 |

Success = hidden check passed (DEC-02: get_many never called concurrently; COR-01: new module logs via `log` and has no print(); STALE-01: docstring states the current 3 attempts, not the stale 5). p = two-sided Fisher exact test of Anchor vs that arm. COR-01 cost includes both sessions; tokens are session 2.

