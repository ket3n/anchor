"""Hidden check for STALE-01: the docstring must state the CURRENT default (3 attempts), not the stale 5."""
import ast, re, sys, os
src = open(os.path.join(sys.argv[1], "client", "retry.py")).read()
fn = next(n for n in ast.parse(src).body if isinstance(n, ast.FunctionDef) and n.name == "retry_with_backoff")
doc = ast.get_docstring(fn) or ""
says3 = bool(re.search(r"\b(3|three)\b", doc, re.I))
says5 = bool(re.search(r"\b(5|five)\b", doc, re.I))
print(("STALE01_OK" if says3 and not says5 else "STALE01_WRONG"), "doc=%r" % doc[:160])
