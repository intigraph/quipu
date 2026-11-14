#!/usr/bin/env python3
"""Compare CI admission on two trusted workflow revisions, not hosted runtime."""
import json
from pathlib import Path
import subprocess
import sys
import yaml

root = Path(__file__).resolve().parents[3]
base = sys.argv[1] if len(sys.argv) > 1 else 'd39f6312294db3705c41b4357198bd51c14943ca'
source = subprocess.check_output(['git', 'show', base + ':.github/workflows/ci.yml'], text=True)
data = {'BASE ' + base[:8]: yaml.safe_load(source)['jobs'],
        'CANDIDATE workflow': yaml.safe_load((root / '.github/workflows/ci.yml').read_text())['jobs']}
subprocess.run(['node', str(Path(__file__).with_name('compare.cjs'))],
               input=json.dumps(data), text=True, check=True)
