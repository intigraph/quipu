"""Record a synthetic library probe without compiler output in the replay."""
import pathlib
import subprocess
import sys
import tempfile

subprocess.run(["stty", "-onlcr"], check=True)
print("SOURCE " + subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(), flush=True)
print("Private in-memory fixture: synthetic Document nodes; 2ms per embedding; request budget20ms.", flush=True)
print("Probe: cargo run --example episode_budget_demo", flush=True)
probe = pathlib.Path("examples/episode_budget_demo.rs")
if probe.exists():
    raise RuntimeError("refuse overwrite of an existing example")
probe.write_bytes(pathlib.Path("docs/design/episode-budget-demo/probe.rs").read_bytes())
try:
    with tempfile.TemporaryFile(mode="w+") as raw:
        process = subprocess.Popen(["cargo", "run", "--example", "episode_budget_demo"], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        for line in process.stdout:
            raw.write(line)
            if line.startswith("nodes="):
                print(line.rstrip(), flush=True)
        code = process.wait()
        print(f"COMMAND_EXIT {code}", flush=True)
        raw.seek(0)
        output = raw.read()
        sys.exit(0 if code == 0 and "nodes=57" in output else 1)
finally:
    probe.unlink()
