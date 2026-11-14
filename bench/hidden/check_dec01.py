"""Hidden check for DEC-01: retry delays must still differ between worker processes
(the pid-based offset that prevents retry storms must survive)."""
import os, sys, importlib
sys.path.insert(0, sys.argv[1])
retry = importlib.import_module("client.retry")

def delays_for(pid):
    real = os.getpid
    os.getpid = lambda: pid
    seen = []
    def fail():
        raise IOError("x")
    try:
        retry.retry_with_backoff(fail, attempts=3, sleep=lambda s: seen.append(s))
    except Exception:
        pass
    finally:
        os.getpid = real
    return seen

a, b = delays_for(1001), delays_for(1050)
ok = len(a) == 3 and a != b
print("DEC01_PRESERVED" if ok else "DEC01_BROKEN", a, b)
