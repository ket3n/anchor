#!/usr/bin/env python3
"""Benchmark v2 — harder, bias-controlled. See ../PROTOCOL.md (pre-registered before the run).

Arms (Claude Code, Sonnet; native auto-memory ON in every arm, each run in its own folder):
  M   baseline: Claude Code native auto-memory, seeded with the knowledge base
  B   CLAUDE.md containing the knowledge base
  C1  official MCP memory server, knowledge base as entities + its README instructions
  C2  basic-memory (MCP + its lifecycle hooks), knowledge base as notes
  E   Anchor (hooks + MCP), knowledge base as items linked to their files
Every memory arm holds the SAME 41 notes (1 relevant + 40 distractors; COR-03: 40 distractors only).

Cases: dec03 (decision, prompt never names the function), cor03 (rule from session 1, invisible in code),
       stale03 (outdated fact, answerable without reading code), neu01 (memory irrelevant: harm check).
Strict success = hidden check AND full visible test suite passes AND task not skipped.

Usage: run.py <out> [--runs 4] [--cases dec03,cor03,stale03,neu01] [--arms M,B,C1,C2,E] [--parallel 3]
"""
import argparse, json, os, re, shutil, subprocess, sys, threading, time
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import gen_fixture, knowledge  # noqa: E402

ANCHOR = os.path.abspath(os.path.join(HERE, "..", "..", "target", "release", "anchor"))
MODEL = "sonnet"
TOOLS = "Read,Edit,Write,MultiEdit,Grep,Glob,Bash(python3 -m unittest),Bash(python3 -m unittest:*)"
EXTRA_TOOLS = {"C1": ",mcp__memory", "C2": ",mcp__basic-memory", "E": ",mcp__anchor"}

PROMPTS = {
    "dec03": "The nightly partner sync takes hours now that we have about 500 partner feeds. Make it faster.",
    "cor03_s1": ("Fix the docstring of fleet/storage/cache.py: it should say 'LRU cache', not 'lru cache'. "
                 "Also, a rule for this project from now on: every new Python module must define `__all__` "
                 "listing its public names."),
    "cor03_s2": ("Create fleet/metrics/latency.py with a function p95(values) that returns the 95th percentile "
                 "of a list of numbers (nearest-rank method)."),
    "stale03": ("Fill in the 'Billing export' section of docs/OPERATIONS.md: explain in two or three sentences "
                "how the exporter batches invoices and how many times it tries each batch."),
    "dec04": "The nightly partner sync takes hours now that we have about 500 partner feeds. Make it faster.",
    "stale04": ("Quick question, no code changes needed: what batch size does our billing exporter use when it "
                "sends invoices to the ledger, and how many times does it try each batch?"),
    "neu01": ("Add a --dry-run flag to the `export` command in fleet/api/cli.py. With --dry-run it must not send "
              "anything; it should print how many invoices would be exported and return 0."),
}
C1_INSTRUCTIONS = """# Memory

You have a persistent knowledge-graph memory (the `memory` MCP tools).
1. At the start of a task, search your memory (search_nodes / read_graph) for anything relevant to the task.
2. While working, note new information worth keeping: user preferences and rules, decisions and their reasons, project facts.
3. When you learn such information, store it: create entities for the things involved, and add observations to them.
"""


def sh(cmd, cwd, timeout=900, env=None):
    return subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=timeout, env=env)


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(text)


def commit(d, msg):
    sh(["git", "add", "-A"], d)
    sh(["git", "-c", "user.email=b@b", "-c", "user.name=bench", "commit", "-qm", msg], d)


def slug(s):
    return re.sub(r"[^a-z0-9]+", "-", s.lower()).strip("-")[:60]


def setup(d, arm, case, run=0):
    gen_fixture.main(d)
    sh(["git", "init", "-q"], d)
    commit(d, "init")
    notes = knowledge.notes_for(case, run)
    mcp = {"mcpServers": {}}
    local_settings = {}
    if arm == "M":
        mem = os.path.join(d, ".automem")
        os.makedirs(mem, exist_ok=True)
        index = []
        for about, kind, text in notes:
            name = slug(f"{about}-{text[:30]}")
            write(os.path.join(mem, name + ".md"),
                  f"---\nname: {name}\ndescription: {json.dumps(text[:120])}\nmetadata:\n  type: project\n---\n\n{text}\n")
            index.append(f"- [{about}]({name}.md) — {text[:100]}")
        write(os.path.join(mem, "MEMORY.md"), "\n".join(index) + "\n")
        local_settings["autoMemoryDirectory"] = mem
    if arm == "B":
        write(os.path.join(d, "CLAUDE.md"), "# Project notes\n\n" + "".join(f"- {t}\n" for _, _, t in notes))
    if arm == "C1":
        f = os.path.join(d, ".mem", "memory.jsonl")
        write(f, "".join(json.dumps({"type": "entity", "name": f"{about} #{i}", "entityType": kind,
                                       "observations": [text]}) + "\n" for i, (about, kind, text) in enumerate(notes)))
        write(os.path.join(d, "CLAUDE.md"), C1_INSTRUCTIONS)
        mcp["mcpServers"]["memory"] = {"command": "npx", "args": ["-y", "@modelcontextprotocol/server-memory"],
                                       "env": {"MEMORY_FILE_PATH": f}}
    if arm == "C2":
        cfg, ndir = os.path.join(d, ".bm", "cfg"), os.path.join(d, ".bm", "notes")
        for i, (about, kind, text) in enumerate(notes):
            write(os.path.join(ndir, kind + "s", f"{i:02d}-{slug(about)}.md"),
                  f"---\ntitle: {about} {kind} {i}\ntype: {kind}\n---\n\n{text}\n")
        env = dict(os.environ, BASIC_MEMORY_CONFIG_DIR=cfg)
        sh(["uvx", "basic-memory", "project", "add", "bench", ndir, "--default"], d, env=env, timeout=600)
        cf = os.path.join(cfg, "config.json")
        conf = json.load(open(cf))
        if "main" in conf.get("projects", {}):
            conf["projects"]["main"]["path"] = os.path.join(d, ".bm", "main")
            os.makedirs(conf["projects"]["main"]["path"], exist_ok=True)
            write(cf, json.dumps(conf, indent=2))
        stray = os.path.expanduser("~/basic-memory")  # created by `project add` before we can redirect it
        if os.path.isdir(stray) and not os.listdir(stray):
            os.rmdir(stray)
        mcp["mcpServers"]["basic-memory"] = {"command": "uvx", "args": ["basic-memory", "mcp"],
                                             "env": {"BASIC_MEMORY_CONFIG_DIR": cfg}}
        pre = f"BASIC_MEMORY_CONFIG_DIR='{cfg}' uvx basic-memory hook"
        write(os.path.join(d, ".claude", "settings.json"), json.dumps({"hooks": {
            "SessionStart": [{"hooks": [{"type": "command", "command": f"{pre} session-start", "timeout": 60}]}],
            "PreCompact": [{"hooks": [{"type": "command", "command": f"{pre} pre-compact", "timeout": 60}]}],
            "Stop": [{"hooks": [{"type": "command", "command": f"{pre} stop", "timeout": 60}]}]}}, indent=2))
    if arm == "E":
        sh([ANCHOR, "--root", d, "install", "--project", d], d)
        mcp = json.load(open(os.path.join(d, ".mcp.json")))
        for about, kind, text in notes:
            sh([ANCHOR, "--root", d, "remember", text, "--kind", kind, "--about", about, "--origin", "user"], d)
    # native auto-memory is ON in every arm (it is what every Claude Code user has); each run keeps it
    # in its own folder. Only arm M has it pre-seeded with the knowledge base.
    local_settings.setdefault("autoMemoryDirectory", os.path.join(d, ".automem"))
    os.makedirs(local_settings["autoMemoryDirectory"], exist_ok=True)
    write(os.path.join(d, ".claude", "settings.local.json"), json.dumps(local_settings, indent=2))
    if case in ("stale03", "stale04"):  # a later teammate commit changes the facts the stored note describes
        p = os.path.join(d, "fleet", "billing", "exporter.py")
        write(p, open(p).read().replace("BATCH_SIZE = 500", "BATCH_SIZE = 200").replace("MAX_ATTEMPTS = 4", "MAX_ATTEMPTS = 2"))
    commit(d, "arm setup")
    path = os.path.join(d, ".bench-mcp.json")
    write(path, json.dumps(mcp, indent=2))
    return path


def claude(d, mcp, prompt, arm):
    env = dict(os.environ)
    env.pop("CLAUDE_CODE_DISABLE_AUTO_MEMORY", None)
    t = time.time()
    p = sh(["claude", "-p", prompt, "--model", MODEL, "--setting-sources", "project,local", "--strict-mcp-config",
            "--mcp-config", mcp, "--permission-mode", "default", "--allowedTools", TOOLS + EXTRA_TOOLS.get(arm, ""),
            "--no-session-persistence", "--output-format", "json"], d, env=env)
    try:
        j = json.loads(p.stdout)
    except Exception:
        j = {"result": (p.stdout + p.stderr)[-1500:], "is_error": True}
    u = j.get("usage") or {}
    return {"wall_s": round(time.time() - t, 1), "cost_usd": j.get("total_cost_usd"), "turns": j.get("num_turns"),
            "input_tokens": (u.get("input_tokens") or 0) + (u.get("cache_read_input_tokens") or 0)
                            + (u.get("cache_creation_input_tokens") or 0),
            "output_tokens": u.get("output_tokens"), "result": (j.get("result") or "")[:1500], "is_error": j.get("is_error")}


def tests_pass(d):
    return sh([sys.executable, "-m", "unittest", "-q"], d).returncode == 0


def changed(d, paths):
    return bool(sh(["git", "diff", "--name-only", "HEAD", "--"] + paths, d).stdout.strip()
                or sh(["git", "ls-files", "--others", "--exclude-standard", "--"] + paths, d).stdout.strip())


def grade_answer_stale(final):
    """STALE-04 is graded on the answer: current values (200, 2), not the stale ones (500, 4)."""
    cur = bool(re.search(r"\b200\b", final)) and bool(re.search(r"\b(2|two)\b\s*(times|attempts|tries)|(attempts?|tries|times)\D{0,25}\b(2|two)\b", final, re.I))
    contrast = re.search(r"\b(but|however|now|actually|outdated|stale|no longer|changed|previously|used to|was)\b", final, re.I)
    old = bool(re.search(r"\b500\b", final)) and not contrast
    return cur and not old, f"current={cur} stale_asserted={old}"


def grade(case, d, final):
    if case == "dec04":
        case = "dec03"  # same task and check, larger knowledge base
    if case == "stale04":
        ok, info = grade_answer_stale(final)
        return {"check": ("OK " if ok else "FAIL ") + info, "hidden_ok": ok, "tests_pass": tests_pass(d),
                "did_task": bool(final.strip()), "success": ok and tests_pass(d) and bool(final.strip())}
    chk = sh([sys.executable, os.path.join(HERE, "hidden", f"check_{case}.py"), d], d).stdout.strip()
    ok = chk.startswith("OK")
    tp = tests_pass(d)
    if case == "dec03":  # not skipped: some code change, or an explicit explanation of the constraint
        did = changed(d, ["fleet"]) or bool(re.search(r"sequential|parallel|concurren|ban", final, re.I))
    elif case == "stale03":
        did = changed(d, ["docs/OPERATIONS.md"])
    elif case == "cor03":
        did = os.path.exists(os.path.join(d, "fleet", "metrics", "latency.py"))
    else:
        did = changed(d, ["fleet/api/cli.py"])
    return {"check": chk[:200], "hidden_ok": ok, "tests_pass": tp, "did_task": did, "success": ok and tp and did}


def run_case(case, arm, i, out):
    d = os.path.join(out, f"{case}-{arm}-{i}")
    mcp = setup(d, arm, case, i)
    r = {"case": case, "arm": arm, "run": i}
    if case == "cor03":
        s1 = claude(d, mcp, PROMPTS["cor03_s1"], arm)
        r["s1"] = {k: s1.get(k) for k in ("cost_usd", "input_tokens", "result", "is_error")}
        commit(d, "session 1")
        r.update(claude(d, mcp, PROMPTS["cor03_s2"], arm))
    else:
        r.update(claude(d, mcp, PROMPTS[case], arm))
    r.update(grade(case, d, r.get("result") or ""))
    return r


def limit_hit(r):
    txt = (r.get("result") or "") + json.dumps(r.get("s1") or {})
    return "hit your" in txt and "limit" in txt


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--runs", type=int, default=4)
    ap.add_argument("--cases", default="dec03,cor03,stale03,neu01")
    ap.add_argument("--arms", default="M,B,C1,C2,E")
    ap.add_argument("--parallel", type=int, default=3)
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    res = os.path.join(a.out, "results.jsonl")
    done = set()
    if os.path.exists(res):
        for l in open(res):
            r = json.loads(l)
            done.add((r["case"], r["arm"], r["run"]))
    # interleave arms within each run index so a quota stop leaves balanced arms
    jobs = [(c, arm, i) for i in range(a.runs) for c in a.cases.split(",") for arm in a.arms.split(",")
            if (c, arm, i) not in done]
    lock = threading.Lock()

    def work(job):
        try:
            r = run_case(*job, a.out)
        except Exception as e:
            r = {"case": job[0], "arm": job[1], "run": job[2], "error": repr(e)[:400]}
        if limit_hit(r):
            print(json.dumps({"skipped_limit": job}), flush=True)
            return
        with lock, open(res, "a") as f:
            f.write(json.dumps(r) + "\n")
        print(json.dumps({k: r.get(k) for k in ("case", "arm", "run", "success", "hidden_ok", "tests_pass", "did_task", "error")}), flush=True)

    with ThreadPoolExecutor(a.parallel) as ex:
        list(ex.map(work, jobs))


if __name__ == "__main__":
    main()
