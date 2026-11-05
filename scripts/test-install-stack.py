#!/usr/bin/env python3
"""Exercise the real stack wrapper using private executable fixtures.

Compatibility controls retain the former plan filename, while the former
command sentinel proves that current operations require the renamed executable.
# arming: agent-invoked; source acceptance before publication.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(os.environ.get('INSTALL_STACK_SCRIPT', Path(__file__).with_name('install-stack.sh')))


class StackInstaller(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.log = self.root / 'calls.jsonl'
        self.env = dict(os.environ, PATH=str(self.bin), STACK_FIXTURE_LOG=str(self.log))
        self.stub('cargo', "raise SystemExit(90)\n")

    def stub(self, name, body):
        path = self.bin / name
        path.write_text('#!' + sys.executable + '\n' + body)
        path.chmod(0o700)

    def current(self):
        self.stub('tambo', "import os,json,sys\nwith open(os.environ['STACK_FIXTURE_LOG'],'a') as f:f.write(json.dumps(sys.argv[1:])+'\\n')\nif sys.argv[1:] == ['--version']:print('tambo fixture')\n")

    def run_wrapper(self, *args):
        return subprocess.run(['/bin/bash', str(SCRIPT), *args], cwd=self.root,
                              env=self.env, capture_output=True, text=True)

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def test_plan_only_uses_current_command_and_preserves_review_boundary(self):
        self.current()
        result = self.run_wrapper()
        self.assertEqual(result.returncode, 0, result.stderr)
        # Compatibility: existing reviewed plan filenames remain readable.
        self.assertEqual(self.calls(), [['--version'], ['plan', '--profile', 'kg', '--output', 'caboodle-plan.toml']])
        self.assertIn('nothing has been installed', result.stdout)

    def test_explicit_yes_runs_plan_apply_verify_in_order(self):
        self.current()
        result = self.run_wrapper('--yes', '--plan', 'reviewed.toml')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls(), [['--version'], ['plan', '--profile', 'kg', '--output', 'reviewed.toml'],
                                       ['apply', '--plan', 'reviewed.toml'], ['verify', '--plan', 'reviewed.toml']])

    def test_legacy_command_does_not_replace_current_command(self):
        # Negative compatibility sentinel: the old executable must not be invoked.
        self.stub('caboodle', "raise SystemExit('legacy command unexpectedly invoked')\n")
        result = self.run_wrapper()
        self.assertEqual(result.returncode, 90, result.stdout + result.stderr)
        self.assertIn('tambo not found', result.stdout)
        self.assertFalse(self.log.exists())

    def test_successful_installer_without_executable_is_refused(self):
        self.stub('cargo', "raise SystemExit(0)\n")
        result = self.run_wrapper('--yes')
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("'tambo' is not on PATH", result.stderr)
        self.assertFalse(self.log.exists())

    def test_dry_run_never_applies_or_verifies(self):
        self.current()
        result = self.run_wrapper('--dry-run', '--yes', '--plan', 'reviewed.toml')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls(), [['--version']])
        self.assertIn('+ tambo apply --plan reviewed.toml', result.stdout)


if __name__ == '__main__':
    unittest.main()
