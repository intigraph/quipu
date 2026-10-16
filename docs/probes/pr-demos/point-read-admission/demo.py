"""Display retained private measurements; no network or workload replay."""
import json
import sys

if sys.stdout.isatty():
    import termios
    settings = termios.tcgetattr(sys.stdout.fileno())
    settings[1] &= ~termios.ONLCR
    termios.tcsetattr(sys.stdout.fileno(), termios.TCSANOW, settings)
from pathlib import Path

m = json.loads(Path(__file__).with_name("measurements.json").read_text())
print("Retained private 500-triple fixture, 90 COUNT requests; no workload replay.")
print("Same CPU 25%, pool 4, 4 GiB RAM, no swap, 10-second admission budget.")
for label, x in m.items():
    assert x["all_200_counts_correct"] and x["all_health_200"]
    print(f"{label}: {x['git_sha'][:8]}")
    for p in x["probes"]:
        assert p["true"] is True and p["false"] is False
        print(f"  in flight={p['inflight']}; true={p['true_seconds']:.6f}s false={p['false_seconds']:.6f}s; truth correct")
    print(f"  COUNT: {x['expensive_200']} correct 62500 results, {x['expensive_408']} HTTP 408; drain {x['drain_seconds']:.3f}s")
    print(f"  health all 200; peak {x['peak_bytes'] / 1048576:.1f} MiB")
assert all(p["inflight"] > 0 and max(p["true_seconds"], p["false_seconds"]) <= 1 for p in m["candidate"]["probes"])
print("PASS point-ASK target <=1s. Off by default; production LIMIT-1 incident unresolved.")
