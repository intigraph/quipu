"""Compare actual CI entrypoints using one private CLI and equivalent scratch history."""
import json
import pathlib
import subprocess
import sys
import tempfile

root = pathlib.Path(__file__).resolve().parents[3]
binary = pathlib.Path(sys.argv[1]).resolve()
probe = pathlib.Path(__file__).with_name('probe.py')
with tempfile.TemporaryDirectory() as d:
    old = pathlib.Path(d) / 'baseline.py'
    old.write_bytes(subprocess.check_output([
        'git', '-C', str(root), 'show',
        '65db3e3c49f4e16b188b7c6b1e895f1428187cac:scripts/ci/pendant-merge-check.py',
    ]))
    for name, script, expected in [
        ('BEFORE: 65db3e3c strict historical replay', old, 1),
        ('AFTER: deb2e38e historical non-pack distinction', root / 'scripts/ci/pendant-merge-check.py', 0),
    ]:
        print(name, flush=True)
        result = subprocess.run([sys.executable, str(probe), str(script), str(binary)],
                                capture_output=True, text=True)
        output = json.loads(result.stdout)
        print(json.dumps(output, indent=2), flush=True)
        assert result.returncode == expected, result.stderr
print('Expected failure before and successful repaired-history verification after: PASS')
