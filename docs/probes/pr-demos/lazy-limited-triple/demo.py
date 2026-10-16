"""Display retained private measurements; performs no query or network call."""

import sys

if sys.stdout.isatty():
    import termios

    attributes = termios.tcgetattr(sys.stdout.fileno())
    attributes[1] &= ~termios.ONLCR
    termios.tcsetattr(sys.stdout.fileno(), termios.TCSANOW, attributes)

print("Private native evaluator: 10,000 synthetic ROOT facts")
print("SELECT ?s WHERE { ?s ?p ?o } LIMIT 1")
print("BEFORE: one subject; 140,078 SQLite VM instructions; ceiling FAIL")
print("AFTER:  one subject;      30 SQLite VM instructions; ceiling PASS")
print("Positive instruction counter; production latency and cause unproven.")
