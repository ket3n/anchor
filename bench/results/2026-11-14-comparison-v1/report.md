# Anchor comparison benchmark

180 agent sessions (134 graded runs), Claude cost $7.26.


## Claude Code, Sonnet


### DEC-02 keep a past decision the user's request conflicts with

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Mean cost |
| --- | --- | --- | --- | --- | --- |
| No memory | 0/5 (0%) | 0%–43% | 0.008 | 87k | $0.052 |
| CLAUDE.md (hand-written) | 5/5 (100%) | 57%–100% | 1.000 | 52k | $0.042 |
| Official MCP memory server | 4/5 (80%) | 38%–96% | 1.000 | 74k | $0.055 |
| basic-memory | 0/5 (0%) | 0%–43% | 0.008 | 85k | $0.056 |
| Anchor | 5/5 (100%) | 57%–100% | — | 77k | $0.053 |

### COR-01 apply a rule the user gave in an earlier session

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Rule captured in session 1 | Mean cost |
| --- | --- | --- | --- | --- | --- | --- |
| No memory | 5/5 (100%) | 57%–100% | 1.000 | 65k | 0/5 | $0.083 |
| Official MCP memory server | 4/5 (80%) | 38%–96% | 1.000 | 74k | 5/5 | $0.096 |
| basic-memory | 5/5 (100%) | 57%–100% | 1.000 | 74k | 0/5 | $0.094 |
| Anchor | 5/5 (100%) | 57%–100% | — | 56k | 5/5 | $0.084 |

### COR-02 apply a user rule that the code cannot reveal

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Rule captured in session 1 | Mean cost |
| --- | --- | --- | --- | --- | --- | --- |
| No memory | 5/5 (100%) | 57%–100% | 1.000 | 37k | 0/5 | $0.076 |
| Official MCP memory server | 1/5 (20%) | 4%–62% | 0.048 | 74k | 5/5 | $0.094 |
| basic-memory | 5/5 (100%) | 57%–100% | 1.000 | 40k | 0/5 | $0.087 |
| Anchor | 5/5 (100%) | 57%–100% | — | 52k | 5/5 | $0.076 |

### STALE-01 don't repeat a fact that the code has since changed

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Mean cost |
| --- | --- | --- | --- | --- | --- |
| No memory | 5/5 (100%) | 57%–100% | 1.000 | 51k | $0.038 |
| CLAUDE.md (not updated) | 5/5 (100%) | 57%–100% | 1.000 | 51k | $0.039 |
| Official MCP memory server | 5/5 (100%) | 57%–100% | 1.000 | 81k | $0.055 |
| basic-memory | 5/5 (100%) | 57%–100% | 1.000 | 54k | $0.044 |
| Anchor | 5/5 (100%) | 57%–100% | — | 65k | $0.048 |

### STALE-02 outdated fact, task answerable without reading the code

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Mean cost |
| --- | --- | --- | --- | --- | --- |
| No memory | 5/5 (100%) | 57%–100% | 1.000 | 84k | $0.053 |
| CLAUDE.md (not updated) | 5/5 (100%) | 57%–100% | 1.000 | 74k | $0.049 |
| Official MCP memory server | 5/5 (100%) | 57%–100% | 1.000 | 87k | $0.062 |
| basic-memory | 5/5 (100%) | 57%–100% | 1.000 | 98k | $0.065 |
| Anchor | 4/5 (80%) | 38%–96% | — | 63k | $0.053 |

## Antigravity, Gemini 3.8 Flash


### DEC-02 keep a past decision the user's request conflicts with

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens |
| --- | --- | --- | --- | --- |
| No memory | 2/3 (67%) | 21%–94% | 1.000 | 94k |
| AGENTS.md (hand-written) | 3/3 (100%) | 44%–100% | 1.000 | 62k |
| Anchor (MCP only) | 3/3 (100%) | 44%–100% | — | 99k |

### COR-01 apply a rule the user gave in an earlier session

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens | Rule captured in session 1 |
| --- | --- | --- | --- | --- | --- |
| No memory | 3/3 (100%) | 44%–100% | 1.000 | 53k | 0/3 |
| Anchor (MCP only) | 3/3 (100%) | 44%–100% | — | 76k | 3/3 |

### STALE-01 don't repeat a fact that the code has since changed

| Arm | Success | 95% CI | p vs Anchor | Mean input tokens |
| --- | --- | --- | --- | --- |
| No memory | 2/2 (100%) | 34%–100% | 1.000 | 40k |
| AGENTS.md (not updated) | 1/1 (100%) | 21%–100% | 1.000 | 28k |
| Anchor (MCP only) | 1/1 (100%) | 21%–100% | — | 48k |

Success = hidden check passed (DEC-02: get_many never called concurrently; COR-01: new module logs via `log` and has no print(); STALE-01: docstring states the current 3 attempts, not the stale 5). p = two-sided Fisher exact test of Anchor vs that arm. COR-01 cost includes both sessions; tokens are session 2.


## Caveats found after this run (fixed in v2, see ../../PROTOCOL.md)
- The "No memory" Claude arm actually had Claude Code's native auto-memory enabled. It captured the user's
  rules in session 1, which explains its COR-01/COR-02 successes. v2 adds a true no-memory arm (A0) and a
  seeded native-memory arm (M).
- Anchor's seeded items carried extra tags other arms did not get; prompts named the exact function. Removed in v2.
- STALE-01 forced reading the code, so it could not separate arms. Antigravity run stopped at 18/24 by the quota guard.
