# Is Anchor actually useful for you?

Don't take the benchmark's word for it: it uses a synthetic repo. Here are three ways to check Anchor on
your own work, from cheapest to most rigorous. Decide what counts as success **before** you start, and
write it down, or you will see what you hope to see.

## 1. Ten-minute sanity check

In one of your repositories:

```sh
anchor install --project .
anchor remember "<a real rule of yours, e.g. 'API handlers never call the DB directly; go through repo/'>" \
  --kind rule --about <a file or folder it applies to> --origin user
anchor recall "<a task that should trigger it>"     # is the rule in the output?
anchor recall "<an unrelated task>"                 # it should NOT be
```

Then open Claude Code and give it the task. Did it follow the rule without being told? Then edit the file
the rule is about and run `anchor list`: the rule should show as stale until it is reviewed.

## 2. A/B on your real work (one to two weeks)

What matters is whether you repeat yourself less and the agent breaks fewer of your rules, at the same or
lower cost. Built-in memory stays on throughout; the only thing that changes is Anchor.

1. **Before you start, write down your measures:**
   - How often you had to repeat a rule or decision you had already stated.
   - How often the agent broke a stored rule or decision.
   - Cost per task (`/cost` in Claude Code, or the usage page).
2. **Randomize per task, not per day.** Flip a coin for each task: heads, run Claude Code normally;
   tails, run it as `ANCHOR_DISABLE=1 claude`. Days differ too much to compare.
3. **Log each task in one line** (`tasks.csv`):

   ```csv
   date,task,anchor(on/off),repeated_myself(count),rule_broken(count),cost_usd,notes
   ```

4. **Check what Anchor did:** `anchor report --days 14`. It counts sessions, how often context was
   shown and how large it was, guardrail warnings and blocks, stale flags and superseded corrections.
   Look at "Most shown". If those items are irrelevant to your work, Anchor is adding noise, not value.
5. **Decide** after at least 20 tasks per arm. Compare rates, not anecdotes. With fewer tasks, only a
   large difference means anything.

Credibility checks while you do this:

- **Is it showing the right things?** `anchor recall "<task>"` before a task shows exactly what it will
  inject.
- **Is it silent when it should be?** On unrelated tasks, prompts in `anchor report` should mostly show
  no injection.
- **Did corrections stick?** After you correct something, `anchor list --all` should show the old item
  superseded.

## 3. Reproducible A/B on your repository (headless)

`bench/ab/ab.py` replays tasks on fresh clones of your repo with three setups: no notes, the same notes
in CLAUDE.md, and the same notes in Anchor. It uses headless Claude Code, so each run costs usage
(about $0.05–0.20 with Sonnet).

1. Pick 3–5 **real** rules or decisions from your project's history: ones an agent has broken before, or
   one a newcomer would plausibly break.
2. For each, write a prompt that tempts breaking it, without mentioning it, and a shell check that passes
   only when the rule held (and your tests still pass). Add 5–20 unrelated true notes as distractors:
   real memory is never just the one relevant note.
3. Run it:

   ```sh
   python3 bench/ab/ab.py tasks.json --repo /path/to/repo --runs 3
   ```

4. Read every failure (`check_output`, `final`, and `git diff` in the clone) before counting it; a broken
   check is not an agent failure.

Example `tasks.json`:

```json
[
  {
    "id": "no-direct-db",
    "notes": [
      {"text": "API handlers never query the DB directly; they call functions in app/repo/", "kind": "rule", "about": "app/api"},
      {"text": "Tests use the fake clock in tests/clock.py", "kind": "note", "about": "tests"}
    ],
    "prompt": "Add an endpoint GET /users/{id}/orders that returns the user's last 10 orders.",
    "check": "! grep -rqE 'session\\.query|execute\\(' app/api && python3 -m pytest -q"
  }
]
```

Anchor is better *for you* if it passes more tasks than CLAUDE.md, at the same or lower cost, across more
than one run. If it ties, it is still useful only when it is cheaper or needs less upkeep from you.

## Comparing with other memory tools

The same setup works for any tool. Give each one the same notes, keep Claude's built-in memory on in all
arms, and use identical prompts and checks. `bench/v2/run.py` shows how this benchmark does it for
claude-mem, Mem0, basic-memory and the official MCP memory server, including the isolation each needs.
Be careful with tools that call a model of their own (claude-mem, Mem0): add that cost to theirs.
