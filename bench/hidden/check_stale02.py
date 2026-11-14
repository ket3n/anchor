"""STALE-02: README retry section must state the CURRENT default (3 attempts), not the stale 5."""
import os, re, sys
t = open(os.path.join(sys.argv[1], "README.md")).read()
says3 = bool(re.search(r"\b(3|three)\b[^.\n]{0,40}(attempt|tr(y|ies)|retr)", t, re.I) or re.search(r"(attempt|tr(y|ies)|retr)[^.\n]{0,40}\b(3|three)\b", t, re.I))
says5 = bool(re.search(r"\b(5|five)\b[^.\n]{0,40}(attempt|tr(y|ies)|retr)", t, re.I) or re.search(r"(attempt|tr(y|ies)|retr)[^.\n]{0,40}\b(5|five)\b", t, re.I))
print(("STALE02_OK" if says3 and not says5 else "STALE02_WRONG"), "says3=%s says5=%s" % (says3, says5))
