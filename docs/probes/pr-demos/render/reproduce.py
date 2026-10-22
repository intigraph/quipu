"""Observe actual candidate CLI publication in a private temporary store."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

binary = Path(sys.argv[1]).resolve(strict=True)
with tempfile.TemporaryDirectory(prefix="render-demo-") as directory:
    root = Path(directory)
    home = root / "home"
    home.mkdir()
    env = {key:value for key,value in os.environ.items() if not key.startswith("QUIPU_")}
    env.update(HOME=str(home), XDG_CONFIG_HOME=str(home / ".config"))
    dest = root / "owned"
    args = [str(binary), "render", "--query",
            "SELECT ?title WHERE { VALUES ?title { 'Example' } }",
            "--template", "# {{ rows[0].title }}\n", "--dest", str(dest),
            "--db", str(root / "private.db")]
    def run():
        return subprocess.run(args, cwd=root, env=env, text=True, capture_output=True)
    assert not dest.exists()
    first = run()
    assert first.returncode == 0, first.stderr
    data = (dest / "report.md").read_bytes()
    before = {p.name:(p.read_bytes(),p.stat().st_mtime_ns,p.stat().st_ino)
              for p in dest.iterdir()}
    repeat = run()
    assert repeat.returncode == 0, repeat.stderr
    after = {p.name:(p.read_bytes(),p.stat().st_mtime_ns,p.stat().st_ino)
             for p in dest.iterdir()}
    assert before == after and json.loads(repeat.stdout)["diff"]["noop"]
    unknown = dest / "unknown.txt"
    unknown.write_text("preserve me")
    refused = run()
    assert refused.returncode != 0 and unknown.read_text() == "preserve me"
    print(json.dumps({"target":"candidate CLI / private local store",
                      "before":"destination absent", "first_files":json.loads(first.stdout)["files"],
                      "after_utf8":data.decode(), "repeat_noop":True,
                      "repeat_same_bytes_mtime_inode":True, "unknown_file_refused_and_preserved":True,
                      "running_installation":"not invoked"}, indent=2))
