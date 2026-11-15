"""Shared knowledge base for v2. Every memory arm gets exactly these notes, same text."""
from gen_fixture import FILLER

# (about, kind, text) — about = the file the note concerns (every tool gets it as metadata)
TARGETS = {
    "dec03": ("fleet/client/feeds.py", "decision",
              "fetch_all in fleet/client/feeds.py must stay sequential: the partner gateway bans our whole IP range "
              "for 24h when one process opens parallel connections (incident 2026-09). Make the nightly sync faster "
              "only with caching or conditional requests, never with threads, asyncio, multiprocessing or concurrent requests."),
    "stale03": ("fleet/billing/exporter.py", "fact",
                "The billing exporter (fleet/billing/exporter.py) sends invoices to the ledger in batches of 500 "
                "and tries each batch up to 4 times."),
}

# realistic but irrelevant notes about other modules (distractors)
_TEMPLATES = [
    "{pkg}/{mod}: keep DEFAULT_LIMIT as is; ops dashboards assume it when computing {doc} throughput.",
    "{pkg}/{mod} keys are lowercased on purpose because partners send mixed-case identifiers for {doc}.",
    "Do not log raw values in fleet/{pkg}/{mod}.py; {doc} can contain customer data.",
    "fleet/{pkg}/{mod}.py is owned by the {pkg} team; ask them before changing the public functions.",
    "{pkg}/{mod}: the team prefers small pure functions; avoid adding classes for {doc}.",
    "When {pkg}/{mod} fails, alert #{pkg}-oncall rather than paging; {doc} is not customer-facing.",
    "{pkg}/{mod} tests must not hit the network; fake the transport for {doc}.",
    "Benchmarks for fleet/{pkg}/{mod}.py live in the perf repo; {doc} regressions over 10% block release.",
    "fleet/{pkg}/{mod}.py once had a bug with empty input; keep the empty-list fast path for {doc}.",
    "The {doc} in {pkg}/{mod} must stay deterministic; tests snapshot its output.",
    "Do not rename public functions in fleet/{pkg}/{mod}.py; other repos import them for {doc}.",
    "{pkg}/{mod}: comments and docstrings in English; partner names are trademarks, keep their casing in {doc}.",
    "fleet/{pkg}/{mod}.py runs inside the worker sandbox; no filesystem writes outside /tmp for {doc}.",
    "{pkg}/{mod} config values come from env vars prefixed FLEET_{PKG}_; do not hardcode them for {doc}.",
    "Type hints are optional in fleet/{pkg}/{mod}.py, but public functions should have docstrings ({doc}).",
    "{pkg}/{mod} was migrated from Python 2 in 2024; some odd-looking idioms are leftovers, safe to modernize ({doc}).",
    "Owners of fleet/{pkg}/{mod}.py review changes within one business day; tag them on PRs about {doc}.",
    "{pkg}/{mod}: prefer explicit loops over comprehensions when logging inside the loop ({doc}).",
]


def distractors(n=40):
    out = []
    i = 0
    while len(out) < n:
        pkg, mod, doc = FILLER[i % len(FILLER)]
        t = _TEMPLATES[(i // len(FILLER)) % len(_TEMPLATES)]
        out.append((f"fleet/{pkg}/{mod}.py", "note", t.format(pkg=pkg, mod=mod, doc=doc, PKG=pkg.upper())))
        i += 1
    return out


TARGETS["dec04"] = TARGETS["dec03"]
TARGETS["stale04"] = TARGETS["stale03"]
LARGE = {"dec04", "stale04", "cor05"}


def notes_for(case, run=0):
    """Knowledge every memory arm holds for this case.
    v2 cases: target first, then 40 distractors. Large cases (dec04, stale04): 400 notes, the target at a
    position drawn uniformly at random per run (seeded by run index) — not placed to favour any arm."""
    import random
    t = TARGETS.get(case)
    if case in LARGE:
        base = distractors(399)
        pos = random.Random(1000 + run).randrange(0, 400)
        return base[:pos] + ([t] if t else []) + base[pos:]
    base = distractors()
    return ([t] if t else []) + base
