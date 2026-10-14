"""Capture the actual first-success history-admission regression."""
import subprocess
import sys

subprocess.run(["stty", "-onlcr"], check=True)
print("SOURCE " + subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(), flush=True)
print("Probe: cargo test --lib failed_first_acquisition_cannot_admit_an_earlier_history_floor -- --nocapture", flush=True)
process = subprocess.Popen(["cargo", "test", "--lib", "failed_first_acquisition_cannot_admit_an_earlier_history_floor", "--", "--nocapture"], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
for line in process.stdout:
    if line.startswith("ANCHORDEMO") or line.startswith("test result:"):
        print(line.rstrip(), flush=True)
code = process.wait()
print(f"COMMAND_EXIT {code}", flush=True)
sys.exit(code)
