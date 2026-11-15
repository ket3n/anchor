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

ANCHOR = os.environ.get("ANCHOR_BIN") or os.path.abspath(os.path.join(HERE, "..", "..", "target", "release", "anchor"))  # pin per iteration
MODEL = "sonnet"
TOOLS = "Read,Edit,Write,MultiEdit,Grep,Glob,Bash(python3 -m unittest),Bash(python3 -m unittest:*)"
EXTRA_TOOLS = {"C1": ",mcp__memory", "C2": ",mcp__basic-memory", "C3": ",mcp__mcp-search", "C4": ",mcp__mem0", "E": ",mcp__anchor"}
# C3 = claude-mem (github.com/thedotmack/claude-mem), loaded per session with --plugin-dir; worker, data and port per run.
CM_PLUGIN = os.environ.get("CM_PLUGIN", "")
CM_BUN = os.environ.get("CM_BUN", "")


def cm_env(d):
    e = dict(os.environ, CLAUDE_MEM_DATA_DIR=os.path.join(d, ".cmdata"), CLAUDE_MEM_WORKER_PORT=open(os.path.join(d, ".cm_port")).read().strip(),
             CLAUDE_MEM_AUTO_UPDATE="false", CLAUDE_MEM_TELEMETRY="0", DO_NOT_TRACK="1", MISE_BUN_VERSION="1.4.4",
             CLAUDE_PLUGIN_ROOT=CM_PLUGIN)
    e["PATH"] = CM_BUN + ":" + e["PATH"]
    return e


# C4 = Mem0 (github.com/mem0ai/mem0, official Claude Code plugin 0.3.3, Mem0 Platform). Key from a private file, never logged.
M0_PLUGIN = os.environ.get("M0_PLUGIN", "")
M0_KEYFILE = os.environ.get("M0_KEYFILE", "")
M0_API = "https://api.mem0.ai"


def m0_env(d):
    return dict(os.environ, MEM0_API_KEY=open(M0_KEYFILE).read().strip(), MEM0_TELEMETRY="false",
                MEM0_USER_ID=open(os.path.join(d, ".m0_user")).read().strip())


def m0_call(method, path, body=None):
    import urllib.request
    req = urllib.request.Request(M0_API + path, method=method, data=None if body is None else json.dumps(body).encode(),
                                 headers={"Authorization": "Token " + open(M0_KEYFILE).read().strip(),
                                          "Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.loads(r.read() or b"{}")


def m0_scope(d):
    """The ids the Mem0 plugin itself derives for this repo (computed with its own code)."""
    p = sh([sys.executable, "-I", "-c", "import sys,json; sys.path.insert(0, sys.argv[1]); import memory_core as m; "
            "r = m.resolve_repo(sys.argv[2]); print(json.dumps([r.app_id, r.project_id]))",
            os.path.join(M0_PLUGIN, "core"), d], d)
    return json.loads(p.stdout)


def m0_cleanup(d):
    """Delete this run's memories from the Mem0 cloud (user and project scopes)."""
    try:
        app, proj = m0_scope(d)
        uid = open(os.path.join(d, ".m0_user")).read().strip()
        import urllib.parse
        for q in ({"user_id": uid}, {"agent_id": proj}):
            m0_call("DELETE", "/v1/memories/?" + urllib.parse.urlencode(q))
    except Exception as e:
        return repr(e)[:200]
    return "ok"


def cm_stop(d):
    """Stop this run's claude-mem worker and its chroma process (pids from its supervisor file)."""
    pids = []
    for f, get in (("supervisor.json", lambda j: [p.get("pid") for p in j.get("processes", {}).values()]),
                   ("worker.pid", lambda j: [j.get("pid")])):
        try:
            pids += get(json.load(open(os.path.join(d, ".cmdata", f))))
        except Exception:
            pass
    for pid in {p for p in pids if p}:
        try:
            os.kill(int(pid), 15)
        except Exception:
            pass


def cm_observer_tokens(d):
    tot = {}
    logs = os.path.join(d, ".cmdata", "logs")
    for f in (os.listdir(logs) if os.path.isdir(logs) else []):
        for line in open(os.path.join(logs, f), errors="replace"):
            if "Observer turn usage" in line:
                for k, v in re.findall(r"(inputTokens|cacheCreationInputTokens|cacheReadInputTokens|outputTokens)=(\d+)", line):
                    tot[k] = tot.get(k, 0) + int(v)
    return tot

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
    "cor04_s1": ("Add a function retry_count(results) to fleet/workers/queue.py that returns how many results are None. "
                 "Also, a rule for this project: progress messages in workers must use log.info from the standard logging module."),
    "cor04_s2": ("Rename the helper queue_key in fleet/workers/queue.py to job_key and update its uses. "
                 "Correction to the earlier rule: from now on, progress messages in workers must use log.debug, not log.info."),
    "cor04_s3": ("Create fleet/workers/drain.py with a function drain(jobs, handle) that calls handle(job) for each job, "
                 "emits a progress message after each one, and returns how many were handled."),
    "cor05_s2": ("Rename the helper queue_key in fleet/workers/queue.py to job_key and update its uses. "
                 "Correction to the earlier rule: from now on, progress messages in workers must use log.debug, not log.info, "
                 "and every progress message must start with the prefix \"[fleet] \"."),
    "cor05_s3": ("Add a function invoice_total(lines) to fleet/billing/invoices.py that sums line['amount'] for each line. "
                 "Also, a rule for billing code: money amounts must be summed with decimal.Decimal, never float."),
    "c01_extra": ("Add a function health_payload(ok) to fleet/api/health.py that returns {'status': 'ok'} when ok "
                  "is true and {'status': 'degraded'} otherwise."),
    "c02_brief": ("Rename the helper queue_key in fleet/workers/queue.py to job_key and update its uses. "
                  "Oh, and from now on worker progress messages are log.debug (not log.info), each starting with \"[fleet] \"."),
    "c02_read": "Go through every module in fleet/parsing/ and fleet/notify/ and add a one-line docstring to each public function that lacks one.",
    "c02_shard": ("Add a function shard_for(key, n) to fleet/storage/keys.py that returns a stable shard index from 0 to n-1 "
                  "using hashlib.blake2b of the key."),
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
    if arm == "C3":
        import zlib, urllib.request
        os.makedirs(os.path.join(d, ".cmdata"), exist_ok=True)
        write(os.path.join(d, ".cmdata", "settings.json"), json.dumps(
            {"CLAUDE_MEM_AUTO_UPDATE": "false", "CLAUDE_MEM_TELEMETRY": "0", "CLAUDE_MEM_CLOUD_SYNC_WS": "false"}))
        port = 38200 + zlib.crc32(d.encode()) % 700
        write(os.path.join(d, ".cm_port"), str(port))
        env = cm_env(d)
        sh(["claude-mem", "start"], d, env=env, timeout=120)
        for _ in range(60):
            try:
                urllib.request.urlopen(f"http://127.0.0.1:{port}/api/health", timeout=2)
                break
            except Exception:
                time.sleep(1)
        for about, kind, text in notes:
            req = urllib.request.Request(f"http://127.0.0.1:{port}/api/memory/save", method="POST",
                                         data=json.dumps({"text": text, "title": f"{kind}: {about}", "cwd": d}).encode(),
                                         headers={"Content-Type": "application/json"})
            urllib.request.urlopen(req, timeout=30).read()
        m = json.load(open(os.path.join(CM_PLUGIN, ".mcp.json")))["mcpServers"]["mcp-search"]
        m["env"] = {k: env[k] for k in ("CLAUDE_MEM_DATA_DIR", "CLAUDE_MEM_WORKER_PORT", "CLAUDE_MEM_TELEMETRY", "DO_NOT_TRACK", "CLAUDE_PLUGIN_ROOT")}
        mcp["mcpServers"]["mcp-search"] = m
    if arm == "C4":
        import zlib
        write(os.path.join(d, ".m0_user"), f"bench-{zlib.crc32(d.encode()):08x}")
        app, proj = m0_scope(d)
        uid = open(os.path.join(d, ".m0_user")).read().strip()
        for i in range(0, len(notes), 20):  # verbatim (infer=False), like every other arm's seeded notes
            m0_call("POST", "/v3/memories/add/", {
                "agent_id": proj, "user_id": uid, "app_id": app, "infer": False,
                "messages": [{"role": "user", "content": t} for _, _, t in notes[i:i + 20]],
                "metadata": {"source": "bench-seed"}})
        mm = json.load(open(os.path.join(M0_PLUGIN, ".mcp.json")))["mcpServers"]["mem0"]
        mm["args"] = [a.replace("${CLAUDE_PLUGIN_ROOT}", M0_PLUGIN) for a in mm["args"]]
        mm["env"] = {"MEM0_CODE_DATA_DIR": os.path.join(d, ".m0data"), "MEM0_TELEMETRY": "false", "MEM0_USER_ID": uid}
        mcp["mcpServers"]["mem0"] = mm
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


def claude(d, mcp, prompt, arm, sess=None, first=False):
    env = cm_env(d) if arm == "C3" else m0_env(d) if arm == "C4" else dict(os.environ)
    env.pop("CLAUDE_CODE_DISABLE_AUTO_MEMORY", None)
    t = time.time()
    plug = ["--plugin-dir", CM_PLUGIN] if arm == "C3" else ["--plugin-dir", M0_PLUGIN] if arm == "C4" else []
    p = sh(["claude", "-p", prompt, "--model", MODEL, "--setting-sources", "project,local", "--strict-mcp-config",
            "--mcp-config", mcp, "--permission-mode", "default", "--allowedTools", TOOLS + EXTRA_TOOLS.get(arm, ""),
            "--output-format", "json"] + (["--no-session-persistence"] if sess is None else
                                         ["--session-id", sess] if first else ["--resume", sess]) + plug, d, env=env)
    try:
        j = json.loads(p.stdout)
    except Exception:
        j = {"result": (p.stdout + p.stderr)[-1500:], "is_error": True}
    u = j.get("usage") or {}
    return {"wall_s": round(time.time() - t, 1), "cost_usd": j.get("total_cost_usd"), "turns": j.get("num_turns"),
            "input_tokens": (u.get("input_tokens") or 0) + (u.get("cache_read_input_tokens") or 0)
                            + (u.get("cache_creation_input_tokens") or 0),
            "output_tokens": u.get("output_tokens"), "result": (j.get("result") or "")[:1500], "is_error": j.get("is_error")}


def compacted(sess):
    """Did the transcript record a compaction? (guards against a silent no-op /compact)"""
    import glob
    for f in glob.glob(os.path.expanduser(f"~/.claude/projects/*/{sess}.jsonl")):
        txt = open(f, errors="replace").read()
        if "compact_boundary" in txt or '"isCompactSummary":true' in txt:
            return True
    return False


def compactions(sess):
    import glob
    n = 0
    for f in glob.glob(os.path.expanduser(f"~/.claude/projects/*/{sess}.jsonl")):
        n = max(n, open(f, errors="replace").read().count("compact_boundary"))
    return n


def tests_pass(d):
    return sh([sys.executable, "-m", "unittest", "-q"], d).returncode == 0


def changed(d, paths):
    return bool(sh(["git", "diff", "--name-only", "HEAD", "--"] + paths, d).stdout.strip()
                or sh(["git", "ls-files", "--others", "--exclude-standard", "--"] + paths, d).stdout.strip())


def grade_answer_stale(final):
    """STALE-04 is graded on the answer: current values (200, 2), not the stale ones (500, 4)."""
    cur = bool(re.search(r"\b200\b", final)) and bool(re.search(r"\b(2|two)\b\s*(times|attempts|tries)|(attempts?|tries|times)\D{0,25}\b(2|two)\b", final, re.I))
    contrast = re.search(r"\b(but|however|now|actually|outdated|out of date|out-of-date|stale|no longer|changed|previously|used to|was|wrong|incorrect)\b", final, re.I)
    old = bool(re.search(r"\b500\b", final)) and not contrast
    return cur and not old, f"current={cur} stale_asserted={old}"


def grade(case, d, final):
    if case == "dec04":
        case = "dec03"  # same task and check, larger knowledge base
    if case in ("compact01", "compact02"):
        case = "cor05"  # same final task and the same strict check (log.debug + "[fleet] " prefix, no log.info)
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
    elif case in ("cor04", "cor05"):
        did = os.path.exists(os.path.join(d, "fleet", "workers", "drain.py"))
    else:
        did = changed(d, ["fleet/api/cli.py"])
    return {"check": chk[:200], "hidden_ok": ok, "tests_pass": tp, "did_task": did, "success": ok and tp and did}


def run_case(case, arm, i, out):
    d = os.path.join(out, f"{case}-{arm}-{i}")
    mcp = setup(d, arm, case, i)
    r = {"case": case, "arm": arm, "run": i}
    if case == "compact02":
        # harder: correction stated once in passing, more work, two compactions before the task
        import uuid
        sess = str(uuid.uuid4())
        r["sessions"] = []
        steps = ["cor04_s1", "c02_brief", "cor05_s3", "c01_extra", "/compact", "c02_read", "c02_shard", "/compact"]
        for n, k in enumerate(steps):
            if k == "/compact" and arm == "C4":
                time.sleep(90)
            sx = claude(d, mcp, PROMPTS.get(k, k), arm, sess=sess, first=(n == 0))
            r["sessions"].append({x: sx.get(x) for x in ("cost_usd", "input_tokens", "is_error", "result")})
            if k != "/compact":
                commit(d, k)
        r["s1"] = {"cost_usd": sum((x["cost_usd"] or 0) for x in r["sessions"]),
                   "input_tokens": sum((x["input_tokens"] or 0) for x in r["sessions"])}
        r.update(claude(d, mcp, PROMPTS["cor04_s3"], arm, sess=sess))
        r["compacted"] = compacted(sess)
        r["compactions"] = compactions(sess)
    elif case == "compact01":
        # one conversation (resumed turn by turn): rule, correction, unrelated work, /compact, then the task
        import uuid
        sess = str(uuid.uuid4())
        r["sessions"] = []
        for n, k in enumerate(("cor04_s1", "cor05_s2", "cor05_s3", "c01_extra")):
            sx = claude(d, mcp, PROMPTS[k], arm, sess=sess, first=(n == 0))
            r["sessions"].append({x: sx.get(x) for x in ("cost_usd", "input_tokens", "is_error", "result")})
            commit(d, k)
        if arm == "C4":
            time.sleep(90)  # Mem0's background flush, as in the multi-session cases
        cx = claude(d, mcp, "/compact", arm, sess=sess)
        r["sessions"].append({x: cx.get(x) for x in ("cost_usd", "input_tokens", "is_error", "result")})
        r["s1"] = {"cost_usd": sum((x["cost_usd"] or 0) for x in r["sessions"]),
                   "input_tokens": sum((x["input_tokens"] or 0) for x in r["sessions"])}
        r.update(claude(d, mcp, PROMPTS["cor04_s3"], arm, sess=sess))
        r["compacted"] = compacted(sess)
    elif case in ("cor04", "cor05"):
        r["sessions"] = []
        pre = ("cor04_s1", "cor04_s2") if case == "cor04" else ("cor04_s1", "cor05_s2", "cor05_s3")
        for k in pre:
            sx = claude(d, mcp, PROMPTS[k], arm)
            r["sessions"].append({x: sx.get(x) for x in ("cost_usd", "input_tokens", "is_error", "result")})
            commit(d, k)
            if arm == "C4":
                time.sleep(90)  # Mem0 flushes after the session ends, then extracts asynchronously in its cloud
        r["s1"] = {"cost_usd": sum((x["cost_usd"] or 0) for x in r["sessions"]),
                   "input_tokens": sum((x["input_tokens"] or 0) for x in r["sessions"])}
        r.update(claude(d, mcp, PROMPTS["cor04_s3"], arm))
    elif case == "cor03":
        s1 = claude(d, mcp, PROMPTS["cor03_s1"], arm)
        r["s1"] = {k: s1.get(k) for k in ("cost_usd", "input_tokens", "result", "is_error")}
        commit(d, "session 1")
        if arm == "C4":
            time.sleep(90)
        r.update(claude(d, mcp, PROMPTS["cor03_s2"], arm))
    else:
        r.update(claude(d, mcp, PROMPTS[case], arm))
    if arm == "C3":
        time.sleep(45)  # let the background observer finish compressing the last session
        t = r["c3_observer_tokens"] = cm_observer_tokens(d)
        # claude-mem's background observer runs on Haiku (CLAUDE_MEM_MODEL=haiku): $0.10 in, $0.50 out per MTok;
        # cache write 1.25x and cache read 0.1x of input. Added to the arm's cost so arms stay comparable.
        r["c3_observer_cost_usd"] = (t.get("inputTokens", 0) * 0.10 + t.get("cacheCreationInputTokens", 0) * 0.125
                                     + t.get("cacheReadInputTokens", 0) * 0.01 + t.get("outputTokens", 0) * 0.50) / 1e6
        cm_stop(d)
    if arm == "C4":
        r["c4_cleanup"] = m0_cleanup(d)
    r.update(grade(case, d, r.get("result") or ""))
    return r


def limit_hit(r):
    txt = (r.get("result") or "") + json.dumps(r.get("s1") or {}) + json.dumps(r.get("sessions") or [])
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
