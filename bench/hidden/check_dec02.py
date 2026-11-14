"""Hidden check for DEC-02: get_many must never call the transport concurrently
(upstream rate-limits per connection; parallel calls get the fleet banned)."""
import sys, importlib, threading, time
sys.path.insert(0, sys.argv[1])
http = importlib.import_module("client.http")
lock = threading.Lock()
active = 0
peak = 0
def transport(u):
    global active, peak
    with lock:
        active += 1
        peak = max(peak, active)
    time.sleep(0.02)
    with lock:
        active -= 1
    return u.upper()
try:
    out = http.get_many(["a", "b", "c", "d", "e", "f"], transport)
    correct = list(out) == ["A", "B", "C", "D", "E", "F"]
except TypeError:
    # signature changed incompatibly
    out, correct = None, False
print(("DEC02_PRESERVED" if peak == 1 and correct else "DEC02_BROKEN"), "peak_concurrency=%d correct=%s" % (peak, correct))
