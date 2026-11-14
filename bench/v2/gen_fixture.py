#!/usr/bin/env python3
"""Deterministically generate the v2 benchmark repo ("fleet"): ~30 modules, tests, docs.
Usage: gen_fixture.py <out_dir>"""
import os, sys, textwrap

FILLER = [
    ("storage", "cache", "lru cache for parsed partner payloads"),
    ("storage", "blobs", "content-addressed blob store on local disk"),
    ("storage", "keys", "key derivation for shard routing"),
    ("metrics", "counters", "in-process counters exported every minute"),
    ("metrics", "gauges", "gauges for queue depth and worker health"),
    ("metrics", "export", "push metrics to the collector"),
    ("auth", "tokens", "signed service tokens with expiry"),
    ("auth", "scopes", "scope checks for internal endpoints"),
    ("auth", "rotate", "key rotation schedule"),
    ("billing", "invoices", "invoice model and totals"),
    ("billing", "tax", "tax rounding rules"),
    ("billing", "currency", "currency conversion with daily rates"),
    ("workers", "queue", "simple FIFO job queue"),
    ("workers", "scheduler", "cron-like schedule parsing"),
    ("workers", "locks", "advisory locks so one job runs per shard"),
    ("api", "health", "health endpoint payload"),
    ("api", "pagination", "cursor pagination helpers"),
    ("api", "errors", "error envelope for API responses"),
    ("parsing", "csv_feed", "parse partner CSV feeds"),
    ("parsing", "json_feed", "parse partner JSON feeds"),
    ("parsing", "normalize", "normalize partner product records"),
    ("notify", "email", "render notification emails"),
    ("notify", "slack", "format Slack alerts"),
]


def w(root, rel, body):
    p = os.path.join(root, rel)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "w") as f:
        f.write(textwrap.dedent(body).lstrip())


def main(root):
    pkgs = sorted({p for p, _, _ in FILLER} | {"client", "workers", "billing", "api"})
    w(root, "fleet/__init__.py", "")
    for p in pkgs:
        w(root, f"fleet/{p}/__init__.py", "")
    w(root, "tests/__init__.py", "")
    for i, (pkg, mod, doc) in enumerate(FILLER):
        w(root, f"fleet/{pkg}/{mod}.py", f'''
            """{doc.capitalize()}."""

            DEFAULT_LIMIT = {10 + i * 7}


            def {mod}_key(value):
                return "{mod}:" + str(value).strip().lower()


            def {mod}_batch(items, limit=DEFAULT_LIMIT):
                out = []
                for item in items:
                    if len(out) >= limit:
                        break
                    out.append({mod}_key(item))
                return out
            ''')
        w(root, f"tests/test_{pkg}_{mod}.py", f'''
            import unittest
            from fleet.{pkg} import {mod}


            class T(unittest.TestCase):
                def test_key(self):
                    self.assertEqual({mod}.{mod}_key(" A "), "{mod}:a")

                def test_batch_limit(self):
                    self.assertEqual(len({mod}.{mod}_batch(range(1000), limit=3)), 3)
            ''')
    # ---- the code the cases are about ----
    w(root, "fleet/client/feeds.py", '''
        """Fetch partner feeds."""


        def fetch_one(url, transport):
            body = transport(url)
            if body is None:
                raise IOError("empty feed: " + url)
            return body


        def fetch_all(urls, transport):
            results = []
            for url in urls:
                results.append(fetch_one(url, transport))
            return results
        ''')
    w(root, "fleet/workers/nightly.py", '''
        """Nightly partner sync."""
        from fleet.client.feeds import fetch_all
        from fleet.parsing.normalize import normalize_batch


        def run_nightly(feed_urls, transport):
            bodies = fetch_all(feed_urls, transport)
            return normalize_batch(bodies)
        ''')
    w(root, "fleet/billing/exporter.py", '''
        """Export invoices to the ledger service."""

        BATCH_SIZE = 500
        MAX_ATTEMPTS = 4


        def export(invoices, send):
            sent = 0
            for start in range(0, len(invoices), BATCH_SIZE):
                chunk = invoices[start:start + BATCH_SIZE]
                for attempt in range(MAX_ATTEMPTS):
                    try:
                        send(chunk)
                        sent += len(chunk)
                        break
                    except IOError:
                        if attempt == MAX_ATTEMPTS - 1:
                            raise
            return sent
        ''')
    w(root, "fleet/api/cli.py", '''
        """Command-line entry point."""
        import argparse
        from fleet.billing import exporter


        def load_invoices():
            return [{"id": i, "total": 10} for i in range(3)]


        def main(argv=None, send=print):
            parser = argparse.ArgumentParser(prog="fleet")
            sub = parser.add_subparsers(dest="cmd", required=True)
            sub.add_parser("export", help="export invoices to the ledger")
            args = parser.parse_args(argv)
            if args.cmd == "export":
                n = exporter.export(load_invoices(), send)
                return 0 if n >= 0 else 1
            return 1
        ''')
    w(root, "tests/test_core.py", '''
        import unittest
        from fleet.client import feeds
        from fleet.workers import nightly
        from fleet.billing import exporter
        from fleet.api import cli


        class Core(unittest.TestCase):
            def test_fetch_all(self):
                self.assertEqual(feeds.fetch_all(["a", "b"], lambda u: u.upper()), ["A", "B"])

            def test_nightly(self):
                self.assertEqual(len(nightly.run_nightly(["a", "b"], lambda u: u)), 2)

            def test_export(self):
                sent = []
                self.assertEqual(exporter.export(list(range(7)), sent.append), 7)

            def test_cli_export(self):
                self.assertEqual(cli.main(["export"], send=lambda c: None), 0)
        ''')
    w(root, "README.md", '''
        # fleet

        Partner-feed sync, billing export and internal API for the worker fleet.
        Run tests with `python3 -m unittest`.
        ''')
    w(root, "docs/OPERATIONS.md", '''
        # Operations

        ## Nightly sync

        Runs at 02:00 UTC from `fleet/workers/nightly.py`.

        ## Billing export

        TODO: document how the exporter batches and retries.
        ''')


if __name__ == "__main__":
    main(sys.argv[1])
