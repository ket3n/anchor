#!/usr/bin/env python3
"""Analyze compare.py results: success rates with Wilson 95% CIs, Fisher exact tests (Anchor vs
each arm), memory-capture rates, tokens and cost. Writes report.md, summary.json and chart.svg.

Usage: analyze.py <results.jsonl> <out_dir>
"""
import json, math, os, sys
from collections import defaultdict

ARM_NAME = {
    "A0": "No memory (native auto-memory off)", "M": "Claude native auto-memory (seeded)",
    "A": "No memory", "B": "CLAUDE.md (hand-written)", "N": "CLAUDE.md (not updated)",
    "C1": "Official MCP memory server", "C2": "basic-memory", "E": "Anchor",
    "XA": "No memory", "XB": "AGENTS.md (hand-written)", "XN": "AGENTS.md (not updated)", "XE": "Anchor (MCP only)",
}
CASE_NAME = {
    "dec02": "DEC-02 keep a past decision the user's request conflicts with",
    "cor01": "COR-01 apply a rule the user gave in an earlier session",
    "stale01": "STALE-01 don't repeat a fact that the code has since changed",
    "cor02": "COR-02 apply a user rule that the code cannot reveal",
    "dec03": "DEC-03 keep a decision; prompt never names the code; 40 distractor notes",
    "cor03": "COR-03 rule from an earlier session, invisible in code; 40 distractor notes",
    "stale03": "STALE-03 outdated fact; task answerable from memory alone; 40 distractor notes",
    "neu01": "NEU-01 memory irrelevant (harm check); 40 distractor notes",
    "stale02": "STALE-02 outdated fact, task answerable without reading the code",
}
ARM_ORDER = ["A0", "M", "A", "B", "N", "C1", "C2", "E", "XA", "XB", "XN", "XE"]


def wilson(k, n, z=1.96):
    if n == 0:
        return (0.0, 0.0)
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (max(0.0, c - h), min(1.0, c + h))


def fisher_two_sided(a, b, c, d):
    """2x2 table [[a, b], [c, d]]; returns two-sided p (sum of tables no more likely than observed)."""
    n1, n2, m1 = a + b, c + d, a + c
    n = n1 + n2

    def prob(x):
        return math.comb(n1, x) * math.comb(n2, m1 - x) / math.comb(n, m1)

    p0 = prob(a)
    lo, hi = max(0, m1 - n2), min(n1, m1)
    return min(1.0, sum(prob(x) for x in range(lo, hi + 1) if prob(x) <= p0 * (1 + 1e-9)))


def mean(xs):
    xs = [x for x in xs if isinstance(x, (int, float))]
    return sum(xs) / len(xs) if xs else None


def main():
    src, out = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    rows = [dict({"agent": "claude"}, **json.loads(l)) for l in open(src) if l.strip()]
    errors = [r for r in rows if r.get("error")]
    rows = [r for r in rows if not r.get("error")]
    g = defaultdict(list)
    for r in rows:
        g[(r["agent"], r["case"], r["arm"])].append(r)

    summary = []
    for (agent, case, arm), rs in g.items():
        k = sum(1 for r in rs if r.get("success"))
        n = len(rs)
        lo, hi = wilson(k, n)
        cap = [r.get("captured") for r in rs if r.get("captured") is not None]
        tok = mean([r.get("input_tokens") for r in rs])
        s1tok = mean([(r.get("s1") or {}).get("input_tokens") for r in rs])
        cost = mean([r.get("cost_usd") for r in rs])
        s1cost = mean([(r.get("s1") or {}).get("cost_usd") for r in rs])
        summary.append({"agent": agent, "case": case, "arm": arm, "k": k, "n": n, "rate": k / n if n else 0,
                        "ci": [lo, hi], "captured": (sum(cap), len(cap)) if cap else None,
                        "input_tokens": tok, "s1_input_tokens": s1tok, "cost_usd": cost, "s1_cost_usd": s1cost,
                        "tests_pass": sum(1 for r in rs if r.get("tests_pass"))})

    def get(agent, case, arm):
        return next((s for s in summary if (s["agent"], s["case"], s["arm"]) == (agent, case, arm)), None)

    for s in summary:  # Fisher: Anchor vs this arm
        e = get(s["agent"], s["case"], "E" if s["agent"] == "claude" else "XE")
        if e and e is not s:
            s["p_vs_anchor"] = fisher_two_sided(e["k"], e["n"] - e["k"], s["k"], s["n"] - s["k"])

    json.dump(summary, open(os.path.join(out, "summary.json"), "w"), indent=2)

    # ---------- report ----------
    L = []
    total_cost = sum((r.get("cost_usd") or 0) + ((r.get("s1") or {}).get("cost_usd") or 0) for r in rows)
    n_sessions = len(rows) + sum(1 for r in rows if r.get("s1"))
    L.append("# Anchor comparison benchmark\n")
    L.append(f"{n_sessions} agent sessions ({len(rows)} graded runs), Claude cost ${total_cost:.2f}"
             f"{f', {len(errors)} harness errors excluded' if errors else ''}.\n")
    for agent, model in (("claude", "Claude Code, Sonnet"), ("agy", "Antigravity, Gemini 3.8 Flash")):
        if not any(s["agent"] == agent for s in summary):
            continue
        L.append(f"\n## {model}\n")
        for case in ("dec02", "dec03", "cor01", "cor02", "cor03", "stale01", "stale02", "stale03", "neu01"):
            ss = sorted([s for s in summary if s["agent"] == agent and s["case"] == case], key=lambda s: ARM_ORDER.index(s["arm"]))
            if not ss:
                continue
            L.append(f"\n### {CASE_NAME[case]}\n")
            hdr = "| Arm | Success | 95% CI | p vs Anchor | Mean input tokens |"
            sep = "| --- | --- | --- | --- | --- |"
            if case in ("cor01", "cor02", "cor03"):
                hdr += " Rule captured in session 1 |"
                sep += " --- |"
            if agent == "claude":
                hdr += " Mean cost |"
                sep += " --- |"
            L.append(hdr)
            L.append(sep)
            for s in ss:
                p = s.get("p_vs_anchor")
                tok = s["input_tokens"]
                line = (f"| {ARM_NAME[s['arm']]} | {s['k']}/{s['n']} ({s['rate']:.0%}) | {s['ci'][0]:.0%}–{s['ci'][1]:.0%} | "
                        f"{'—' if p is None else f'{p:.3f}'} | {'—' if tok is None else f'{tok/1000:.0f}k'} |")
                if case in ("cor01", "cor02", "cor03"):
                    c = s["captured"]
                    line += f" {'—' if not c else f'{c[0]}/{c[1]}'} |"
                if agent == "claude":
                    cst = s["cost_usd"]
                    if case in ("cor01", "cor02", "cor03") and s["s1_cost_usd"]:
                        cst = (cst or 0) + s["s1_cost_usd"]
                    line += f" {'—' if cst is None else f'${cst:.3f}'} |"
                L.append(line)
    L.append("\nSuccess = hidden check passed (DEC-02: get_many never called concurrently; COR-01: new module logs via "
             "`log` and has no print(); STALE-01: docstring states the current 3 attempts, not the stale 5). "
             "p = two-sided Fisher exact test of Anchor vs that arm. COR-01 cost includes both sessions; tokens are session 2.\n")
    open(os.path.join(out, "report.md"), "w").write("\n".join(L) + "\n")

    # ---------- chart (SVG, hand-drawn) ----------
    groups = []
    for agent in ("claude", "agy"):
        for case in ("dec02", "dec03", "cor01", "cor02", "cor03", "stale01", "stale02", "stale03", "neu01"):
            ss = sorted([s for s in summary if s["agent"] == agent and s["case"] == case], key=lambda s: ARM_ORDER.index(s["arm"]))
            if ss:
                groups.append((agent, case, ss))
    W, row_h, left = 760, 22, 230
    H = 60 + sum(30 + row_h * len(ss) for _, _, ss in groups) + 30
    sx = lambda v: left + v * (W - left - 70)
    out_svg = [f"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 {W} {H}' font-family='system-ui, sans-serif' font-size='12'>",
               "<rect width='100%' height='100%' fill='#ffffff'/>",
               f"<text x='16' y='28' font-size='15' font-weight='600' fill='#1f2328'>Task success by memory setup (dot = rate, whisker = 95% CI)</text>"]
    y = 52
    for agent, case, ss in groups:
        title = ("Claude · " if agent == "claude" else "Antigravity · ") + CASE_NAME[case].split(" ")[0]
        out_svg.append(f"<text x='16' y='{y+14}' font-weight='600' fill='#1f2328'>{title}</text>")
        y += 24
        for s in ss:
            cy = y + row_h / 2
            anchor = s["arm"] in ("E", "XE")
            col = "#d1242f" if not anchor else "#0969da"
            out_svg.append(f"<text x='{left-10}' y='{cy+4}' text-anchor='end' fill='#1f2328'{' font-weight=\"600\"' if anchor else ''}>{ARM_NAME[s['arm']]}</text>")
            out_svg.append(f"<line x1='{sx(0)}' x2='{sx(1)}' y1='{cy}' y2='{cy}' stroke='#eaeef2'/>")
            out_svg.append(f"<line x1='{sx(s['ci'][0])}' x2='{sx(s['ci'][1])}' y1='{cy}' y2='{cy}' stroke='{col}' stroke-width='2' opacity='0.5'/>")
            out_svg.append(f"<circle cx='{sx(s['rate'])}' cy='{cy}' r='5' fill='{col}'/>")
            out_svg.append(f"<text x='{sx(1)+8}' y='{cy+4}' fill='#57606a'>{s['k']}/{s['n']}</text>")
            y += row_h
        y += 6
    for v in (0, 0.5, 1):
        out_svg.append(f"<text x='{sx(v)}' y='{H-12}' text-anchor='middle' fill='#57606a'>{int(v*100)}%</text>")
    out_svg.append("</svg>")
    open(os.path.join(out, "chart.svg"), "w").write("\n".join(out_svg))
    print("\n".join(L))


if __name__ == "__main__":
    main()
