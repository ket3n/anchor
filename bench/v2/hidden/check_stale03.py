"""STALE-03: docs must state the CURRENT values (batches of 200, 2 attempts), not the stale 500 / 4."""
import os, re, sys
t = open(os.path.join(sys.argv[1], "docs", "OPERATIONS.md")).read()
sec = t.split("## Billing export", 1)[-1]
cur = bool(re.search(r"\b200\b", sec)) and bool(re.search(r"\b(2|two)\b", sec, re.I))
old = bool(re.search(r"\b500\b", sec)) or bool(re.search(r"\b(4|four)\b\s*(times|attempts|tries)", sec, re.I))
todo = "TODO" in sec
print(("OK" if cur and not old and not todo else "FAIL"), f"current={cur} stale={old} todo_left={todo}")
