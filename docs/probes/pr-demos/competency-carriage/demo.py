"""Record real, isolated base/head CLI calls as an asciinema v2 cast."""
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time

base, head, destination = map(pathlib.Path, sys.argv[1:])
started = time.monotonic()
events = []


def emit(text):
    print(text, end="", flush=True)
    events.append([round(time.monotonic() - started, 6), "o", text.replace("\n", "\r\n")])


with tempfile.TemporaryDirectory() as directory:
    root = pathlib.Path(directory)
    contract = '''schema = "quipu-competency/1"
[[question]]
id = "present"
ask = "Is someone present?"
query = "ASK { ?p a <urn:Person> }"
justifies = ["urn:PersonShape"]
[[command]]
id = "record"
does = "Record a person"
inputs = { person = "iri" }
template = '<{person}> a <urn:Person> .'
satisfies = ["urn:PersonShape"]
write_class = "record-person"
key = ["person"]
example = { person = "urn:alice" }
'''
    (root / "contract.toml").write_text(contract)

    def run(binary, label, args, success=True):
        emit(f"$ {label} {' '.join(args)}\n")
        result = subprocess.run(
            [str(binary), *args], cwd=root,
            env={"HOME": directory, "PATH": os.defpath},
            capture_output=True, text=True, check=False,
        )
        emit(result.stdout + result.stderr)
        emit(f"exit={result.returncode}\n")
        assert (result.returncode == 0) == success
        return result

    for binary, label in [(base, "base"), (head, "head")]:
        run(binary, label, ["--version"])
    common = ["share", "--db", "source.db", "--no-shapes", "--destination", "internal"]
    run(base, "base", common + ["--output", "base-contract", "--competency", "contract.toml"], False)
    run(head, "head", common + ["--output", "head-contract", "--competency", "contract.toml"])
    pendant = root / "head-contract"
    emit("carried files: " + ", ".join(sorted(p.name for p in pendant.iterdir())) + "\n")
    assert (pendant / "competency.toml").read_text() == contract
    run(head, "head", ["import", "head-contract", "--db", "receiver.db"])
    commands = pendant / "commands.ttl"
    commands.write_text(commands.read_text() + "\n# altered after sealing\n")
    emit("Changed commands.ttl after sealing.\n")
    run(head, "head", ["import", "head-contract", "--db", "refused.db"], False)
    for binary, label in [(base, "base"), (head, "head")]:
        run(binary, label, common + ["--output", label + "-legacy"])
    before = {p.name: p.read_bytes() for p in (root / "base-legacy").iterdir()}
    after = {p.name: p.read_bytes() for p in (root / "head-legacy").iterdir()}
    assert before == after
    emit(f"No-contract control: {len(before)} files byte-identical.\n")
header = {"version": 2, "width": 110, "height": 32, "timestamp": int(time.time()), "env": {"TERM": "xterm-256color"}}
destination.write_text("\n".join(json.dumps(x) for x in [header, *events]) + "\n")
