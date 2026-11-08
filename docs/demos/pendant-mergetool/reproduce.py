"""Store-free demo in a disposable Git repository; needs two CLI executables."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

before, after = map(str, map(Path.resolve, map(Path, sys.argv[1:3])))


def run(args, expected=0, input=None):
    result = subprocess.run(args, input=input, text=True, capture_output=True)
    print(result.stdout, end="")
    print(result.stderr, end="")
    print(f"exit {result.returncode}")
    if result.returncode != expected:
        raise SystemExit("unexpected demonstration result")


def digest(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


with tempfile.TemporaryDirectory() as directory:
    import os
    os.chdir(directory)
    git = ["git", "-c", "user.name=Demo", "-c", "user.email=demo@example.org"]
    subprocess.run(git + ["init", "-q", "-b", "base"], check=True)
    subprocess.run(git + ["config", "user.name", "Demo"], check=True)
    subprocess.run(git + ["config", "user.email", "demo@example.org"], check=True)
    pack = Path("pendant")
    pack.mkdir()
    shapes = '''@prefix sh: <http://www.w3.org/ns/shacl#> .
<urn:shape> a sh:NodeShape; sh:targetClass <urn:Ticket>;
    sh:property [sh:path <urn:value>; sh:maxCount 1] .
'''
    (pack / "shapes.ttl").write_text(shapes)
    Path(".gitattributes").write_text(
        "pendant/export.nt merge=quipu\npendant/shapes.ttl merge=quipu\n"
        "pendant/manifest.json merge=quipu\n")

    def save(value):
        graph = ('<urn:ticket> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <urn:Ticket> .\n'
                 f'<urn:ticket> <urn:value> "{value}" .\n')
        (pack / "export.nt").write_text(graph)
        manifest = {
            "canonicalization": "RDFC-1.0", "created_at": "2026-01-01T00:00:00Z",
            "files": {"graph": "export.nt", "shapes": "shapes.ttl", "turtle_view": None},
            "graph_hash": digest(graph.encode()), "parent_share": None,
            "producer": {"name": "quipu", "version": "demo"},
            "schema": "https://github.com/scbrown/quipu/share-manifest/v1",
            "scope": {"kind": "root", "value": None}, "shapes_hash": digest(shapes.encode()),
            "store_id": "urn:uuid:00000000-0000-4000-8000-000000000001", "tx_anchor": 1,
        }
        encoded = lambda: json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode()
        manifest["share_id"] = digest(encoded())
        (pack / "manifest.json").write_bytes(encoded())
        subprocess.run(git + ["add", "."], check=True)
        subprocess.run(git + ["commit", "-qm", value], check=True)

    save("base")
    for branch in ("ours", "theirs"):
        subprocess.run(git + ["checkout", "-qb", branch, "base"], check=True)
        save(branch)
    subprocess.run(git + ["checkout", "-q", "ours"], check=True)
    refs = [subprocess.check_output(git + ["rev-parse", ref], text=True).strip()
            for ref in ("base", "ours", "theirs")]
    print("AFTER: quipu git-merge theirs (a real slot conflict)")
    run([after, "git-merge", "theirs"], expected=2)
    print("BEFORE: quipu git-mergetool pendant/export.nt --by reviewer")
    run([before, "git-mergetool", "pendant/export.nt", "--by", "reviewer"], expected=2)
    print("AFTER: same command, input: conflict:0 theirs")
    run([after, "git-mergetool", "pendant/export.nt", "--by", "reviewer"],
        input="conflict:0 theirs\n")
    assert '"theirs"' in (pack / "export.nt").read_text()
    subprocess.run(git + ["add", "."], check=True)
    subprocess.run(git + ["commit", "-qm", "resolved"], check=True)
    print("CONTROL: immutable pendant-check base ours theirs HEAD")
    run([after, "pendant-check", *refs, "HEAD"])
