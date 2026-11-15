# Anchor comparison benchmark

80 agent sessions (60 graded runs), Claude cost $7.72.


## Claude Code, Sonnet


### DEC-03 keep a decision; prompt never names the code; 40 distractor notes

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Mean cost |
| --- | --- | --- | --- | --- | --- |
| Claude native memory (baseline) | 4/4 (100%) | 51%–100% | 1.000 | 141k | $0.098 |
| CLAUDE.md (hand-written) | 4/4 (100%) | 51%–100% | 1.000 | 125k | $0.092 |
| Official MCP memory server | 2/4 (50%) | 15%–85% | 0.429 | 135k | $0.097 |
| basic-memory | 4/4 (100%) | 51%–100% | 1.000 | 135k | $0.108 |
| Anchor | 4/4 (100%) | 51%–100% | — | 111k | $0.087 |

### DEC-04 keep a decision; 400-note knowledge base, target at a random position

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Mean cost |
| --- | --- | --- | --- | --- | --- |
| Claude native memory (baseline) | 4/4 (100%) | 51%–100% | 1.000 | 168k | $0.135 |
| CLAUDE.md (hand-written) | 4/4 (100%) | 51%–100% | 1.000 | 207k | $0.163 |
| Official MCP memory server | 3/4 (75%) | 30%–95% | 1.000 | 168k | $0.108 |
| basic-memory | 4/4 (100%) | 51%–100% | 1.000 | 181k | $0.131 |
| Anchor | 4/4 (100%) | 51%–100% | — | 126k | $0.091 |

### COR-04 user changes a rule in session 2; session 3 must follow the new rule; 40 notes

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Rule captured in session 1 | Mean cost |
| --- | --- | --- | --- | --- | --- | --- |
| Claude native memory (baseline) | 3/4 (75%) | 30%–95% | 1.000 | 83k | — | $0.195 |
| CLAUDE.md (hand-written) | 3/4 (75%) | 30%–95% | 1.000 | 57k | — | $0.154 |
| Official MCP memory server | 4/4 (100%) | 51%–100% | 1.000 | 86k | — | $0.166 |
| basic-memory | 4/4 (100%) | 51%–100% | 1.000 | 68k | — | $0.153 |
| Anchor | 4/4 (100%) | 51%–100% | — | 67k | — | $0.153 |

Success = hidden check passed (DEC-02: get_many never called concurrently; COR-01: new module logs via `log` and has no print(); STALE-01: docstring states the current 3 attempts, not the stale 5). p = two-sided Fisher exact test of Anchor vs that arm. COR-01 cost includes both sessions; tokens are session 2.

