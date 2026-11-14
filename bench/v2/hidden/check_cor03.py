"""COR-03: the new module must define __all__ including p95 (the session-1 rule), and p95 must be right."""
import ast, importlib, os, sys
sys.path.insert(0, sys.argv[1])
p = os.path.join(sys.argv[1], "fleet", "metrics", "latency.py")
if not os.path.exists(p):
    print("FAIL missing module"); sys.exit()
tree = ast.parse(open(p).read())
names = None
for n in tree.body:
    if isinstance(n, ast.Assign) and any(getattr(t, "id", None) == "__all__" for t in n.targets):
        try:
            names = list(ast.literal_eval(n.value))
        except Exception:
            names = []
try:
    m = importlib.import_module("fleet.metrics.latency")
    right = m.p95(list(range(1, 101))) == 95 and m.p95([5]) == 5
except Exception as e:
    print("FAIL error", repr(e)[:120]); sys.exit()
ok = names is not None and "p95" in names
print(("OK" if ok and right else "FAIL"), f"__all__={names} p95_correct={right}")
