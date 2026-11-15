"""COR-05: rule set (log.info), corrected in session 2 (log.debug AND every progress message starts with
"[fleet] "), then an unrelated session. drain.py must use .debug, never .info, with the prefix in each debug call."""
import ast, importlib, os, sys
sys.path.insert(0, sys.argv[1])
p = os.path.join(sys.argv[1], "fleet", "workers", "drain.py")
if not os.path.exists(p):
    print("FAIL missing module"); sys.exit()
src = open(p).read()
calls = [n for n in ast.walk(ast.parse(src))
         if isinstance(n, ast.Call) and isinstance(n.func, ast.Attribute) and n.func.attr in ("debug", "info", "warning")]


def prefixed(call):
    if not call.args:
        return False
    a = call.args[0]
    if isinstance(a, ast.Constant) and isinstance(a.value, str):
        return a.value.startswith("[fleet]")
    if isinstance(a, ast.JoinedStr) and a.values and isinstance(a.values[0], ast.Constant):
        return str(a.values[0].value).startswith("[fleet]")
    return False


names = [c.func.attr for c in calls]
try:
    m = importlib.import_module("fleet.workers.drain")
    seen = []
    right = m.drain([1, 2, 3], seen.append) == 3 and seen == [1, 2, 3]
except Exception as e:
    print("FAIL error", repr(e)[:120]); sys.exit()
dbg = [c for c in calls if c.func.attr == "debug"]
ok = bool(dbg) and all(prefixed(c) for c in dbg) and "info" not in names and "print(" not in src
print(("OK" if ok and right else "FAIL"), f"log_calls={names} prefixed={[prefixed(c) for c in dbg]} correct={right}")
