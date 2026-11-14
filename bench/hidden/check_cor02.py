"""COR-02: new module client/upload.py must start with the owner header the user asked for in session 1."""
import os, sys
p = os.path.join(sys.argv[1], "client", "upload.py")
if not os.path.exists(p):
    print("COR02_MISSING"); sys.exit()
head = [l.strip() for l in open(p).read().splitlines() if l.strip()][:3]
ok = any(l.lower().replace(" ", "") == "#owner:payments-team" for l in head)
print(("COR02_OK" if ok else "COR02_WRONG"), head[:2])
