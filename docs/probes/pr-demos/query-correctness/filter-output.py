"""Keep the observable fixture output; shell pipefail preserves test failures."""
import sys
for line in sys.stdin:
    if line.startswith(("ORDER BY", "application/", "text/", "running ", "test ", "error:")):
        print(line, end="")
