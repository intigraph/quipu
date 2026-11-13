"""Native prerequisite and fail-fast recipe controls; no package installs or Cargo."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class DocTools(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.base = Path(self.directory.name)
        self.bin = self.base / "bin"
        self.bin.mkdir()
        self.trace = self.base / "trace"
        # Copies only: stubs must never write through links to installed tools.
        for name in ("bash", "sh", "cat", "just"):
            source = shutil.which(name)
            self.assertIsNotNone(source, f"test prerequisite missing: {name}")
            shutil.copy2(source, self.bin / name)
        self.env = dict(os.environ, PATH=str(self.bin), TRACE=str(self.trace))
        self.tool("pre-commit")

    def tool(self, name, broken=False):
        path = self.bin / name
        path.write_text(
            "#!/bin/bash\n"
            f'printf "{name} %s\\n" "$*" >> "$TRACE"\n'
            + ("exit 126\n" if broken else "exit 0\n")
        )
        path.chmod(0o755)

    def run_command(self, *args):
        return subprocess.run(args, cwd=ROOT, env=self.env, text=True,
                              capture_output=True, timeout=30)

    def calls(self):
        return self.trace.read_text() if self.trace.exists() else ""

    def test_missing_tools_names_both_and_explains_cargo_path(self):
        result = self.run_command(str(ROOT / "scripts/preflight-doc-tools.sh"))
        self.assertNotEqual(0, result.returncode)
        for expected in ("mdbook not found", "mdbook-mermaid not found",
                         "${CARGO_HOME:-$HOME/.cargo}/bin", "just docs preflight"):
            self.assertIn(expected, result.stderr)

    def test_missing_mermaid_does_not_render_or_start_full_hooks(self):
        self.tool("mdbook")
        for args in (("just", "check"), ("just", "setup"),
                     ("just", "docs", "build"), ("just", "docs", "serve"),
                     (str(ROOT / "scripts/precommit-mdbook.sh"),)):
            with self.subTest(command=args):
                result = self.run_command(*args)
                self.assertNotEqual(0, result.returncode)
                self.assertIn("mdbook-mermaid not found", result.stderr)
                self.assertNotIn("pre-commit ", self.calls())
                self.assertNotIn("mdbook build", self.calls())
                self.assertNotIn("mdbook serve", self.calls())

    def test_unusable_executable_is_an_actionable_failure(self):
        self.tool("mdbook")
        self.tool("mdbook-mermaid", broken=True)
        result = self.run_command("just", "docs", "preflight")
        self.assertNotEqual(0, result.returncode)
        self.assertIn("mdbook-mermaid is on PATH but cannot run", result.stderr)
        self.assertIn("--force", result.stderr)

    def test_successful_preflight_still_runs_actual_build_command(self):
        self.tool("mdbook")
        self.tool("mdbook-mermaid")
        result = self.run_command(str(ROOT / "scripts/precommit-mdbook.sh"))
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertIn("mdbook --version", self.calls())
        self.assertIn("mdbook-mermaid --version", self.calls())
        self.assertIn("mdbook build docs/book", self.calls())

    def test_successful_check_and_setup_keep_canonical_hooks(self):
        self.tool("mdbook")
        self.tool("mdbook-mermaid")
        for command, expected in (("check", "pre-commit run --all-files"),
                                  ("setup", "pre-commit install")):
            with self.subTest(command=command):
                result = self.run_command("just", command)
                self.assertEqual(0, result.returncode, result.stderr)
                self.assertIn(expected, self.calls())


if __name__ == "__main__":
    unittest.main()
