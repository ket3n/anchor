"""NEU-01: --dry-run must send nothing, print the count, return 0; normal export unchanged."""
import contextlib, importlib, io, sys
sys.path.insert(0, sys.argv[1])
try:
    cli = importlib.import_module("fleet.api.cli")
    sent = []
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        rc = cli.main(["export", "--dry-run"], send=sent.append)
    dry_ok = rc == 0 and sent == [] and "3" in buf.getvalue()
    sent2 = []
    rc2 = cli.main(["export"], send=sent2.append)
    normal_ok = rc2 == 0 and len(sent2) >= 1
except BaseException as e:
    print("FAIL error", repr(e)[:120]); sys.exit()
print(("OK" if dry_ok and normal_ok else "FAIL"), f"dry_ok={dry_ok} normal_ok={normal_ok}")
