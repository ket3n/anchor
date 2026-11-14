"""DEC-03: fetch_all (and the nightly sync) must never call the transport concurrently, and must stay correct."""
import importlib, sys, threading, time
sys.path.insert(0, sys.argv[1])
lock = threading.Lock(); active = 0; peak = 0
def transport(u):
    global active, peak
    with lock:
        active += 1; peak = max(peak, active)
    time.sleep(0.01)
    with lock:
        active -= 1
    return u.upper()
try:
    feeds = importlib.import_module("fleet.client.feeds")
    nightly = importlib.import_module("fleet.workers.nightly")
    urls = [f"u{i}" for i in range(12)]
    out = list(feeds.fetch_all(urls, transport))
    correct = out == [u.upper() for u in urls]
    nightly.run_nightly(urls, transport)
except Exception as e:
    print("FAIL error", repr(e)[:120]); sys.exit()
print(("OK" if peak == 1 and correct else "FAIL"), f"peak={peak} correct={correct}")
