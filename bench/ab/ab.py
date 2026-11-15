#!/usr/bin/env python3
"""A/B-test Anchor on YOUR repository with headless Claude Code.

Each task: the notes a memory tool would hold, a prompt that tempts the agent to break one of them,
and a check command that exits 0 when the result is right. Every run works in a fresh clone, so your
repository is never modified.

Arms (Claude's built-in memory stays on in all of them, in a per-run folder):
  none      the notes are not given            -> what happens without project memory
  claudemd  the notes are written to CLAUDE.md -> the usual manual approach
  anchor    the notes are stored in Anchor     -> anchor install --project + anchor remember

Usage:
  ab.py tasks.json --repo /path/to/repo [--runs 3] [--arms none,claudemd,anchor] [--out DIR] [--model sonnet]

tasks.json:
  [{"id": "keep-sequential",
    "notes": [{"text": "fetch_all must stay sequential: the gateway bans parallel clients", "kind": "decision",
               "about": "src/feeds.py"}],
    "prompt": "The nightly sync is slow. Make it faster.",
    "check": "! grep -rqE 'ThreadPool|asyncio.gather' src/feeds.py && python3 -m pytest -q"}]

Runs cost real usage on your Claude plan (about $0.05-0.20 each with Sonnet). Results: <out>/results.jsonl
and a table on stdout. Read the failures before trusting the totals.
"""
import argparse, json, os, shutil, subprocess, sys, tempfile, time
from collections import defaultdict

TOOLS = "Read,Edit,Write,MultiEdit,Grep,Glob,Bash(git diff:*),Bash(git status:*)"


def sh(cmd, cwd, timeout=1800, env=None, shell=False):
    return subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=timeout, env=env, shell=shell)


def setup(repo, d, arm, task, anchor):
    sh(["git", "clone", "-q", "--local", "--no-hardlinks", repo, d], "/")
    automem = os.path.join(d, ".ab-automem")
    os.makedirs(automem, exist_ok=True)
    os.makedirs(os.path.join(d, ".claude"), exist_ok=True)
    with open(os.path.join(d, ".claude", "settings.local.json"), "w") as f:
        json.dump({"autoMemoryDirectory": automem}, f)
    mcp = {"mcpServers": {}}
    notes = task.get("notes", [])
    if arm == "claudemd":
        with open(os.path.join(d, "CLAUDE.md"), "a") as f:
            f.write("\n# Project notes\n\n" + "".join(f"- {n['text']}\n" for n in notes))
    if arm == "anchor":
        sh([anchor, "--root", d, "install", "--project", d], d)
        mcp = json.load(open(os.path.join(d, ".mcp.json")))
        for n in notes:
            cmd = [anchor, "--root", d, "remember", n["text"], "--kind", n.get("kind", "note"), "--origin", "user"]
            if n.get("about"):
                cmd += ["--about", n["about"]]
            sh(cmd, d)
    sh(["git", "add", "-A"], d)
    sh(["git", "-c", "user.email=ab@local", "-c", "user.name=ab", "commit", "-qm", "ab setup", "--allow-empty"], d)
    path = os.path.join(d, ".ab-mcp.json")
    json.dump(mcp, open(path, "w"))
    return path


def run_one(repo, task, arm, i, out, anchor, model):
    d = os.path.join(out, f"{task['id']}-{arm}-{i}")
    shutil.rmtree(d, ignore_errors=True)
    mcp = setup(repo, d, arm, task, anchor)
    env = dict(os.environ)
    env.pop("ANCHOR_DISABLE", None)
    if arm != "anchor":
        env["ANCHOR_DISABLE"] = "1"  # a global Anchor install must not leak into the other arms
    t = time.time()
    p = sh(["claude", "-p", task["prompt"], "--model", model, "--setting-sources", "project,local",
            "--strict-mcp-config", "--mcp-config", mcp, "--permission-mode", "default",
            "--allowedTools", TOOLS + (",mcp__anchor" if arm == "anchor" else ""),
            "--no-session-persistence", "--output-format", "json"], d, env=env)
    try:
        j = json.loads(p.stdout)
    except Exception:
        j = {"result": (p.stdout + p.stderr)[-800:], "is_error": True}
    chk = sh(task["check"], d, shell=True, timeout=900)
    return {"task": task["id"], "arm": arm, "run": i, "ok": chk.returncode == 0,
            "cost_usd": j.get("total_cost_usd"), "turns": j.get("num_turns"), "wall_s": round(time.time() - t),
            "check_output": (chk.stdout + chk.stderr)[-400:], "final": (j.get("result") or "")[:600],
            "error": j.get("is_error")}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("tasks")
    ap.add_argument("--repo", required=True)
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--arms", default="none,claudemd,anchor")
    ap.add_argument("--out", default=None)
    ap.add_argument("--model", default="sonnet")
    ap.add_argument("--anchor", default=shutil.which("anchor") or "anchor")
    a = ap.parse_args()
    tasks = json.load(open(a.tasks))
    out = a.out or tempfile.mkdtemp(prefix="anchor-ab-")
    os.makedirs(out, exist_ok=True)
    res = os.path.join(out, "results.jsonl")
    done = set()
    if os.path.exists(res):
        done = {(r["task"], r["arm"], r["run"]) for r in map(json.loads, open(res))}
    for i in range(a.runs):  # interleave arms so a stop leaves them balanced
        for t in tasks:
            for arm in a.arms.split(","):
                if (t["id"], arm, i) in done:
                    continue
                r = run_one(os.path.abspath(a.repo), t, arm, i, out, a.anchor, a.model)
                with open(res, "a") as f:
                    f.write(json.dumps(r) + "\n")
                print(f"{t['id']:<24} {arm:<9} run {i}: {'PASS' if r['ok'] else 'FAIL'}  ${r['cost_usd'] or 0:.3f}", flush=True)
    rows = [json.loads(l) for l in open(res)]
    g = defaultdict(list)
    for r in rows:
        g[(r["task"], r["arm"])].append(r)
    print(f"\nResults in {out}\n\n| task | arm | pass | mean cost |\n| --- | --- | --- | --- |")
    for (task, arm), rs in sorted(g.items()):
        costs = [r["cost_usd"] for r in rs if r["cost_usd"] is not None]
        print(f"| {task} | {arm} | {sum(r['ok'] for r in rs)}/{len(rs)} | ${(sum(costs) / len(costs)) if costs else 0:.3f} |")
    print("\nOpen the FAIL runs (check_output, final, and the clone's git diff) before drawing conclusions.")


if __name__ == "__main__":
    sys.exit(main())
