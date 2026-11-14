"""COR-04: the user changed the rule in session 2 — drain.py must log progress with .debug, never .info."""
import ast, importlib, os, sys
sys.path.insert(0, sys.argv[1])
p = os.path.join(sys.argv[1], "fleet", "workers", "drain.py")
if not os.path.exists(p):
    print("FAIL missing module"); sys.exit()
calls = [n.func.attr for n in ast.walk(ast.parse(open(p).read()))
         if isinstance(n, ast.Call) and isinstance(n.func, ast.Attribute) and n.func.attr in ("debug", "info", "warning")]
try:
    m = importlib.import_module("fleet.workers.drain")
    seen = []
    right = m.drain([1, 2, 3], seen.append) == 3 and seen == [1, 2, 3]
except Exception as e:
    print("FAIL error", repr(e)[:120]); sys.exit()
ok = "debug" in calls and "info" not in calls and "print" not in open(p).read()
print(("OK" if ok and right else "FAIL"), f"log_calls={calls} correct={right}")
