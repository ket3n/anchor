#!/usr/bin/env python3
"""Pilot benchmark: DEC-01 (decision preservation) and COR-01 (correction retention).

Arms: A = plain agent, B = hand-written CLAUDE.md, E = Anchor (hooks + MCP).
Each run uses a fresh copy of the fixture, a fresh headless Claude Code session,
user settings excluded (--setting-sources project,local), only the run's MCP config,
and a scoped tool allowlist (no permission bypass).

Usage: run_bench.py <out_dir> [--runs N] [--model sonnet] [--cases dec01,cor01] [--parallel 3]
"""
import argparse, json, os, shutil, subprocess, sys, time
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
FIXTURE = os.path.join(HERE, "fixture", "svc")
ANCHOR = os.path.abspath(os.path.join(HERE, "..", "target", "release", "anchor"))

DECISION = ("The pid-based offset in retry_with_backoff (os.getpid() % 97) spreads retries across the 64 worker "
            "processes. Without it all workers retry in lockstep and overload the upstream API (incident 2026-10). "
            "Do not remove or simplify it away.")

DECISION2 = ("get_many must stay sequential: the upstream API rate-limits per client and bans the whole worker "
             "fleet for an hour when it sees parallel requests from one process (incident 2026-09). Speed it up "
             "only with caching or batching, never with threads, asyncio or concurrent requests.")
DEC02_PROMPT = "get_many in client/http.py is slow for long URL lists. Make it faster."

DEC01_PROMPT = ("The code in client/ has grown messy. Clean it up and simplify it: remove dead code, needless "
                "branches, and anything that looks like a leftover hack. Keep the public functions get, get_many and "
                "retry_with_backoff, and keep the tests passing (run: python3 -m unittest).")

COR01_S1 = ("Add a function head(url, transport) to client/http.py that returns the length of the response. "
            "Also, a rule for this project from now on: never use print() for diagnostics or progress output; "
            "always use the `log` object from client/log.py.")
COR01_S2 = ("Create a new module client/upload.py with a function upload(chunks, transport) that sends each chunk "
            "with transport(chunk) and returns how many were sent. Print a short progress message for each chunk sent.")

TOOLS = ("Read,Edit,Write,MultiEdit,Grep,Glob,Bash(python3 -m unittest),Bash(python3 -m unittest:*),"
         "mcp__anchor__recall,mcp__anchor__remember,mcp__anchor__review")


def sh(cmd, cwd, timeout=600, stdin=None, env=None):
    return subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=timeout, input=stdin, env=env)


def prepare(run_dir, arm):
    shutil.copytree(FIXTURE, run_dir, ignore=shutil.ignore_patterns("__pycache__"))
    sh(["git", "init", "-q"], run_dir)
    sh(["git", "add", "-A"], run_dir)
    sh(["git", "-c", "user.email=b@b", "-c", "user.name=bench", "commit", "-qm", "init"], run_dir)
    mcp = os.path.join(run_dir, ".bench-mcp.json")
    if arm in ("E", "E2"):
        sh([ANCHOR, "--root", run_dir, "install", "--project", run_dir], run_dir)
        shutil.copy(os.path.join(run_dir, ".mcp.json"), mcp)
    else:
        with open(mcp, "w") as f:
            json.dump({"mcpServers": {}}, f)
    return mcp


ARM_ENV = {"E2": {"ANCHOR_NO_CHECKPOINT": "1", "ANCHOR_PROMPT_SCOPE": "knowledge"}}


def claude(run_dir, mcp, prompt, model, arm=None):
    t = time.time()
    env = dict(os.environ, **ARM_ENV.get(arm, {}))
    p = sh(["claude", "-p", prompt, "--model", model, "--setting-sources", "project,local",
            "--strict-mcp-config", "--mcp-config", mcp, "--permission-mode", "default",
            "--allowedTools", TOOLS, "--no-session-persistence", "--output-format", "json"], run_dir, timeout=900, env=env)
    wall = time.time() - t
    try:
        d = json.loads(p.stdout)
    except Exception:
        d = {"result": p.stdout[-2000:], "error": p.stderr[-2000:]}
    u = d.get("usage") or {}
    return {
        "wall_s": round(wall, 1),
        "cost_usd": d.get("total_cost_usd"),
        "turns": d.get("num_turns"),
        "input_tokens": (u.get("input_tokens") or 0) + (u.get("cache_read_input_tokens") or 0)
                        + (u.get("cache_creation_input_tokens") or 0),
        "output_tokens": u.get("output_tokens"),
        "result": (d.get("result") or "")[:1500],
        "is_error": d.get("is_error"),
    }


def anchor_events(run_dir):
    p = os.path.join(run_dir, ".anchor", "events.jsonl")
    if not os.path.exists(p):
        return []
    return [json.loads(l) for l in open(p) if l.strip()]


def tool_counts(events):
    c = {}
    for e in events:
        if e["event"] == "PreToolUse" and e.get("tool"):
            c[e["tool"]] = c.get(e["tool"], 0) + 1
    return c


def unit_ok(run_dir):
    p = sh([sys.executable, "-m", "unittest", "-q"], run_dir)
    return p.returncode == 0


def case_dec01(base, arm, i, model):
    d = os.path.join(base, f"dec01-{arm}-{i}")
    mcp = prepare(d, arm)
    if arm == "B":
        with open(os.path.join(d, "CLAUDE.md"), "w") as f:
            f.write("# Project notes\n\n- " + DECISION + "\n")
        sh(["git", "add", "-A"], d); sh(["git", "-c", "user.email=b@b", "-c", "user.name=bench", "commit", "-qm", "notes"], d)
    if arm in ("E", "E2"):
        sh([ANCHOR, "--root", d, "remember", DECISION, "--kind", "decision", "--about", "retry_with_backoff",
            "--tag", "jitter", "--tag", "retry storm", "--tag", "hack", "--origin", "user"], d)
    r = claude(d, mcp, DEC01_PROMPT, model, arm)
    chk = sh([sys.executable, os.path.join(HERE, "hidden", "check_dec01.py"), d], d)
    diff = sh(["git", "diff", "--stat"], d).stdout.strip().splitlines()
    ev = anchor_events(d)
    r.update({
        "case": "DEC-01", "arm": arm, "run": i,
        "decision_preserved": "DEC01_PRESERVED" in chk.stdout,
        "tests_pass": unit_ok(d),
        "changed": diff[-1] if diff else "no changes",
        "tools": tool_counts(ev) if arm in ("E", "E2") else None,
        "anchor_injected": [e["extra"] for e in ev if e["event"] in ("UserPromptSubmit", "PreToolUse") and e.get("extra") and (e["extra"].get("injected") or e["extra"].get("warnings"))][:4],
    })
    return r


def case_dec02(base, arm, i, model):
    d = os.path.join(base, f"dec02-{arm}-{i}")
    mcp = prepare(d, arm)
    if arm == "B":
        with open(os.path.join(d, "CLAUDE.md"), "w") as f:
            f.write("# Project notes\n\n- " + DECISION2 + "\n")
        sh(["git", "add", "-A"], d); sh(["git", "-c", "user.email=b@b", "-c", "user.name=bench", "commit", "-qm", "notes"], d)
    if arm in ("E", "E2"):
        sh([ANCHOR, "--root", d, "remember", DECISION2, "--kind", "decision", "--about", "get_many",
            "--tag", "parallel", "--tag", "concurrency", "--tag", "performance", "--tag", "rate limit", "--origin", "user"], d)
    r = claude(d, mcp, DEC02_PROMPT, model, arm)
    chk = sh([sys.executable, os.path.join(HERE, "hidden", "check_dec02.py"), d], d)
    diff = sh(["git", "diff", "--stat"], d).stdout.strip().splitlines()
    ev = anchor_events(d)
    r.update({
        "case": "DEC-02", "arm": arm, "run": i,
        "decision_preserved": "DEC02_PRESERVED" in chk.stdout,
        "check": chk.stdout.strip()[:200],
        "tests_pass": unit_ok(d),
        "changed": diff[-1] if diff else "no changes",
        "tools": tool_counts(ev) if arm in ("E", "E2") else None,
    })
    return r


STALE_FACT = "retry_with_backoff makes 5 attempts by default before giving up."
STALE01_PROMPT = ("Add a docstring to retry_with_backoff in client/retry.py that states how many attempts it makes "
                  "by default and what happens after the last one.")


def case_stale01(base, arm, i, model):
    d = os.path.join(base, f"stale01-{arm}-{i}")
    mcp = prepare(d, "E" if arm == "E" else "A")
    if arm == "N":  # naive memory: a notes file that is never invalidated
        with open(os.path.join(d, "CLAUDE.md"), "w") as f:
            f.write("# Project notes\n\n- " + STALE_FACT + "\n")
    if arm == "E":
        sh([ANCHOR, "--root", d, "remember", STALE_FACT, "--kind", "fact", "--about", "retry_with_backoff"], d)
    # a later commit changes the default; the stored fact is now outdated
    p = os.path.join(d, "client", "retry.py")
    src = open(p).read().replace("attempts=5", "attempts=3")
    open(p, "w").write(src)
    sh(["git", "add", "-A"], d); sh(["git", "-c", "user.email=b@b", "-c", "user.name=bench", "commit", "-qm", "fewer retries"], d)
    r = claude(d, mcp, STALE01_PROMPT, model, arm)
    chk = sh([sys.executable, os.path.join(HERE, "hidden", "check_stale01.py"), d], d)
    r.update({"case": "STALE-01", "arm": arm, "run": i, "correct": "STALE01_OK" in chk.stdout,
              "check": chk.stdout.strip()[:200], "tests_pass": unit_ok(d)})
    return r


def cor01_check(d):
    p = os.path.join(d, "client", "upload.py")
    if not os.path.exists(p):
        return {"upload_exists": False, "uses_log": False, "uses_print": False, "retained": False}
    src = open(p).read()
    uses_log = "log." in src
    uses_print = "print(" in src
    return {"upload_exists": True, "uses_log": uses_log, "uses_print": uses_print, "retained": uses_log and not uses_print}


def case_cor01(base, arm, i, model):
    d = os.path.join(base, f"cor01-{arm}-{i}")
    mcp = prepare(d, arm)
    s1 = None
    recorded = None
    if arm in ("E", "E2"):
        s1 = claude(d, mcp, COR01_S1, model, arm)
        lst = sh([ANCHOR, "--root", d, "list"], d).stdout
        recorded = [json.loads(l) for l in lst.splitlines() if l.strip()]
        sh(["git", "add", "-A"], d); sh(["git", "-c", "user.email=b@b", "-c", "user.name=bench", "commit", "-qm", "s1"], d)
    r = claude(d, mcp, COR01_S2, model, arm)
    r.update(cor01_check(d))
    r.update({
        "case": "COR-01", "arm": arm, "run": i,
        "tests_pass": unit_ok(d),
        "session1": {k: s1[k] for k in ("cost_usd", "turns", "result")} if s1 else None,
        "recorded_in_s1": [{"kind": x["kind"], "trust": x["trust"], "content": x["content"][:200]} for x in (recorded or [])],
    })
    return r


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--model", default="sonnet")
    ap.add_argument("--cases", default="dec01,cor01")
    ap.add_argument("--parallel", type=int, default=3)
    ap.add_argument("--arms", default="")
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    jobs = []
    if "dec01" in a.cases:
        jobs += [(case_dec01, arm, i) for i in range(a.runs) for arm in ("A", "B", "E")]
    if "dec02" in a.cases:
        jobs += [(case_dec02, arm, i) for i in range(a.runs) for arm in (a.arms.split(",") if a.arms else ("A", "B", "E"))]
    if "stale01" in a.cases:
        jobs += [(case_stale01, arm, i) for i in range(a.runs) for arm in (a.arms.split(",") if a.arms else ("A", "N", "E"))]
    if "cor01" in a.cases:
        jobs += [(case_cor01, arm, i) for i in range(a.runs) for arm in (a.arms.split(",") if a.arms else ("A", "E"))]
    results_path = os.path.join(a.out, "results.jsonl")

    def run(job):
        fn, arm, i = job
        try:
            r = fn(a.out, arm, i, a.model)
        except Exception as e:
            r = {"case": fn.__name__, "arm": arm, "run": i, "error": repr(e)}
        with open(results_path, "a") as f:
            f.write(json.dumps(r) + "\n")
        print(json.dumps({k: r.get(k) for k in ("case", "arm", "run", "decision_preserved", "retained", "correct", "tests_pass", "cost_usd", "error")}), flush=True)
        return r

    with ThreadPoolExecutor(a.parallel) as ex:
        list(ex.map(run, jobs))


if __name__ == "__main__":
    main()
