"""Merge a disposable DataBook; no store, service or installed upgrade."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile

before, after = (str(Path(value).resolve()) for value in sys.argv[1:3])


def run(args, expected=0, input=None, diagnostic=None):
    result = subprocess.run(args, input=input, text=True, capture_output=True)
    print(result.stdout, end="")
    print(result.stderr, end="")
    print(f"exit {result.returncode}")
    assert result.returncode == expected, "unexpected demonstration result"
    if diagnostic is not None:
        assert diagnostic in result.stderr, "unexpected refusal diagnostic"


def document(value):
    return f'''---
id: urn:book
title: Review example
type: databook
version: 1.5.0
unknown: preserve-this-field
---

# Human prose survives the RDF merge.

<!-- databook:id: people -->
<!-- databook:graph: urn:people -->
```turtle
<urn:person> a <urn:Person>; <urn:name> "{value}" .
```

<!-- databook:id: constraints -->
```shacl
@prefix sh: <http://www.w3.org/ns/shacl#> .
<urn:shape> a sh:NodeShape; sh:targetClass <urn:Person>;
    sh:property [sh:path <urn:name>; sh:maxCount 1] .
```
'''


with tempfile.TemporaryDirectory() as directory:
    os.chdir(directory)
    git = ["git", "-c", "user.name=Demo", "-c", "user.email=demo@example.org"]
    subprocess.run(git + ["init", "-q", "-b", "base"], check=True)
    subprocess.run(git + ["config", "user.name", "Demo"], check=True)
    subprocess.run(git + ["config", "user.email", "demo@example.org"], check=True)
    Path(".gitattributes").write_text("people.databook.md merge=quipu-databook\n")

    def save(value):
        Path("people.databook.md").write_text(document(value))
        subprocess.run(git + ["add", "."], check=True)
        subprocess.run(git + ["commit", "-qm", value], check=True)

    save("base")
    for branch in ("ours", "theirs"):
        subprocess.run(git + ["checkout", "-qb", branch, "base"], check=True)
        save(branch)
    subprocess.run(git + ["checkout", "-q", "ours"], check=True)
    refs = [subprocess.check_output(git + ["rev-parse", ref], text=True).strip()
            for ref in ("base", "ours", "theirs")]
    print("AFTER: quipu git-merge theirs (declared DataBook, functional conflict)")
    run([after, "git-merge", "theirs"], expected=2)
    print("BEFORE: installed older CLI, same DataBook mergetool command")
    # A pendant-aware baseline recognizes the verb and refuses this path (exit 1);
    # the earlier unknown-command baseline returned 2. Require its exact reason.
    refusal = "pendant mergetool refused: git-mergetool takes a pendant's repository-relative export.nt"
    print(f"BEFORE expected refusal (exit 1): {refusal}")
    run([before, "git-mergetool", "people.databook.md", "--by", "reviewer"],
        expected=1, diagnostic=refusal)
    print("AFTER: quipu git-mergetool people.databook.md --by reviewer")
    print("input: graph:0/conflict:0 theirs")
    run([after, "git-mergetool", "people.databook.md", "--by", "reviewer"],
        input="graph:0/conflict:0 theirs\n")
    assert Path("people.databook.md").read_text() == document("theirs")
    print("PRESERVED: unknown YAML field, human prose and unchanged shape bytes")
    subprocess.run(git + ["add", "."], check=True)
    subprocess.run(git + ["commit", "-qm", "reviewed merge"], check=True)
    print("CONTROL: immutable databook-check base ours theirs HEAD people.databook.md")
    run([after, "databook-check", *refs, "HEAD", "people.databook.md"])
