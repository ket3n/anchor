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
]


def distractors(n=40):
    out = []
    i = 0
    while len(out) < n:
        pkg, mod, doc = FILLER[i % len(FILLER)]
        t = _TEMPLATES[(i // len(FILLER)) % len(_TEMPLATES)]
        out.append((f"fleet/{pkg}/{mod}.py", "note", t.format(pkg=pkg, mod=mod, doc=doc)))
        i += 1
    return out


def notes_for(case):
    """Knowledge every memory arm holds for this case (target first, then 40 distractors).
    COR-03 seeds no target: the rule must be captured in session 1."""
    base = distractors()
    t = TARGETS.get(case)
    return ([t] if t else []) + base
