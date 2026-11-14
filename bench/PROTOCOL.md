# Benchmark v2 protocol (pre-registered 2026-11-14, before any v2 run)

## Question
Does Anchor make a coding agent behave better than (a) no memory, (b) Claude Code's native auto-memory,
(c) a hand-written CLAUDE.md, (d) other memory tools — on tasks where project memory should matter —
without hurting tasks where it should not?

## Arms (Claude Code, `sonnet`, one fresh session per run, user settings excluded, scoped tool allowlist)
| Arm | Setup |
| --- | --- |
| A0 | No memory. Native auto-memory disabled (`CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`). |
| M  | Native auto-memory, seeded with the knowledge base (`autoMemoryDirectory` inside the run). |
| B  | CLAUDE.md containing the knowledge base. Native auto-memory disabled. |
| C1 | Official MCP memory server; knowledge base as entities; its README's memory instructions in CLAUDE.md. |
| C2 | basic-memory 1.0: MCP + its own lifecycle hooks; knowledge base as notes. |
| E  | Anchor: hooks + MCP; knowledge base as items linked to the file each note is about. |

## Fairness rules
1. Every memory arm holds the same 41 notes, same text: 1 relevant + 40 realistic distractors
   (COR-03: 40 distractors; the rule must be captured in session 1). No arm gets extra tags or synonyms.
2. Metadata parity: every arm gets the file each note is about (entity name / note title / `about`).
3. Prompts never name the function or file the relevant note is about (DEC-03, STALE-03).
4. Generated repo: 37 Python files, 4 packages of filler, tests for every module.
5. Grading is automatic and identical for all arms; graders never see the arm.
6. A run that hits a usage limit is discarded and re-queued, never scored.

## Cases and strict success (all three must hold)
hidden check passes AND the full visible test suite passes AND the task was actually attempted.
| Case | Situation | Hidden check |
| --- | --- | --- |
| DEC-03 | "Nightly sync is slow, make it faster." A stored decision forbids parallel fetching. | fetch_all + nightly never call the transport concurrently; output correct. Attempted = code changed or the constraint explained. |
| COR-03 | Session 1: user states "every new module defines `__all__`" during an unrelated fix. Session 2: create a new module. | `__all__` contains `p95`; p95 correct. |
| STALE-03 | Stored fact says batches of 500 / 4 attempts; a later commit changed the code to 200 / 2. Task: document it. | Docs state 200 and 2, not 500 / 4; TODO removed. |
| NEU-01 | Add `--dry-run` to the CLI. Memory is irrelevant. | Dry run sends nothing, prints the count, returns 0; normal export unchanged. |

## Pre-registered expectations (to be confirmed or refuted)
- DEC-03: A0 fails most runs; B, M, E succeed most runs; E ≥ C1, C2.
- COR-03: A0 and B fail (no capture); M and E capture and apply; C1/C2 depend on whether the agent calls them.
- STALE-03: B and M (no invalidation) are at risk of writing 500/4; E should flag the item stale and win.
- NEU-01: no arm drops more than one run versus A0 (no harm); token overhead reported.
- Report every arm, every case, with Wilson 95% CIs and two-sided Fisher exact p (E vs each arm),
  whatever the outcome. n = 4 runs per arm per case (quota-bound), so only large effects can reach p < 0.05.
