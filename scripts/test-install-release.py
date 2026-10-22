#!/usr/bin/env python3
"""Install release-shaped fixtures; failures must preserve an existing command."""
from contextlib import redirect_stderr, redirect_stdout
import hashlib
import io
import json
import os
import shutil
import sys
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from urllib.request import urlopen as real_urlopen
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
CONFIG = {'archive': 'quipu-{tag}-{target}.tar.gz', 'checksum': '{archive}.sha256', 'directory': '{archive}', 'commands': ['quipu', 'qp', 'quipu-server'], 'targets': {'Linux:x86_64': 'x86_64-unknown-linux-gnu', 'Linux:aarch64': 'aarch64-unknown-linux-gnu', 'Darwin:x86_64': 'x86_64-apple-darwin', 'Darwin:arm64': 'aarch64-apple-darwin'}, 'repo': 'quipu'}
TAG = 'quipu-ai-v0.15.0'
VERSION = '0.15.0'
TARGET = CONFIG['targets']['Linux:x86_64']


class ReleaseInstall(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='release-install-test-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.release = self.root / 'release'
        self.release.mkdir()
        self.prefix = self.root / 'prefix'
        (self.prefix / 'bin').mkdir(parents=True)
        self.existing = self.prefix / 'bin' / CONFIG['commands'][0]
        self.existing.write_text('previous installed bytes')
        self.env = dict(os.environ, HOME=str(self.root / 'home'), STACK_VERSION=TAG,
                        STACK_INSTALL_ROOT=str(self.prefix), STACK_RELEASE_BASE_URL=self.release.as_uri())
        self.name = CONFIG['archive'].format(tag=TAG, version=VERSION, target=TARGET)
        self.archive = self.release / self.name
        self.fixture()
        self.latest = {"tag_name": TAG, "draft": False, "prerelease": False}

    def fixture(self, version=VERSION, extra=None):
        directory = CONFIG['directory'].format(archive=self.name.removesuffix('.tar.gz'))
        with tarfile.open(self.archive, 'w:gz') as bundle:
            for command in CONFIG['commands']:
                body = f'#!/bin/sh\necho "{command} {version}"\n'.encode()
                member = tarfile.TarInfo(str(Path(directory) / command))
                member.size, member.mode = len(body), 0o755
                bundle.addfile(member, io.BytesIO(body))
            if extra:
                member = tarfile.TarInfo(extra)
                member.size = 1
                bundle.addfile(member, io.BytesIO(b'x'))
        digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        manifest = CONFIG['checksum'].format(archive=self.name)
        (self.release / manifest).write_text(f'{digest}  {self.name}\n')

    def open_release(self, request, **kwargs):
        if request.full_url.startswith('https://api.github.com/repos/scbrown/'):
            return io.BytesIO(json.dumps(self.latest).encode())
        return real_urlopen(request, **kwargs)

    def run_installer(self, system='Linux', machine='x86_64'):
        # Execute this exact entrypoint's embedded Python with only platform
        # detection substituted; fixture requests use real file:// downloads.
        script = (ROOT / 'install.sh').read_text().split("<<'PYINSTALL'\n", 1)[1].rsplit('PYINSTALL', 1)[0]
        stdout, stderr = io.StringIO(), io.StringIO()
        status = 0
        with patch.dict(os.environ, self.env, clear=True), patch('sys.argv', ['install.sh']), patch('urllib.request.urlopen', side_effect=self.open_release), \
             patch('platform.system', return_value=system), patch('platform.machine', return_value=machine), \
             redirect_stdout(stdout), redirect_stderr(stderr):
            try:
                exec(compile(script, str(ROOT / 'install.sh'), 'exec'), {'__name__': '__main__'})
            except SystemExit as error:
                status = error.code
        return status, stdout.getvalue(), stderr.getvalue()

    def assert_preserved(self, result):
        self.assertNotEqual(result[0], 0, result)
        self.assertEqual(self.existing.read_text(), 'previous installed bytes')
        self.assertFalse(self.existing.is_symlink())
        self.assertNotIn('Installed verified', result[1])

    def test_install_verified_release_and_run_linked_command(self):
        result = self.run_installer()
        self.assertEqual(result[0], 0, result)
        self.assertTrue(self.existing.is_symlink())
        output = subprocess.check_output([str(self.existing), '--version'], text=True)
        self.assertIn(VERSION, output)

    def test_latest_release_selection_installs_the_resolved_tag(self):
        self.env.pop('STACK_VERSION')
        result = self.run_installer()
        self.assertEqual(result[0], 0, result)
        self.assertIn(TAG, result[1])

    def test_prerelease_cannot_become_latest_stable_install(self):
        self.env.pop('STACK_VERSION')
        self.latest['prerelease'] = True
        self.assert_preserved(self.run_installer())

    def test_reinstall_of_intact_release_succeeds(self):
        self.assertEqual(self.run_installer()[0], 0)
        result = self.run_installer()
        self.assertEqual(result[0], 0, result)

    def test_tampered_existing_bundle_cannot_claim_verified_success(self):
        self.assertEqual(self.run_installer()[0], 0)
        self.existing.resolve().write_text('tampered installed bytes')
        result = self.run_installer()
        self.assertNotEqual(result[0], 0, result)
        self.assertIn('installed bundle bytes differ', result[2])
        self.assertNotIn('Installed verified', result[1])
        self.assertEqual(self.existing.read_text(), 'tampered installed bytes')

    def test_corrupt_checksum_preserves_previous(self):
        self.archive.write_bytes(self.archive.read_bytes() + b'corruption')
        self.assert_preserved(self.run_installer())

    def test_missing_archive_preserves_previous(self):
        self.archive.unlink()
        self.assert_preserved(self.run_installer())

    def test_unsupported_platform_preserves_previous(self):
        self.assert_preserved(self.run_installer(system='UnsupportedOS'))

    def test_wrong_version_preserves_previous(self):
        self.fixture(version='999.999.999')
        self.assert_preserved(self.run_installer())

    def test_archive_traversal_preserves_previous(self):
        self.fixture(extra='../escaped')
        self.assert_preserved(self.run_installer())
        self.assertFalse((self.root / 'escaped').exists())

    def test_ambiguous_checksum_preserves_previous(self):
        manifest = self.release / CONFIG['checksum'].format(archive=self.name)
        manifest.write_text(manifest.read_text() * 2)
        self.assert_preserved(self.run_installer())



class DocumentedInstallExamples(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='documented-install-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.cwd = self.root / 'work'
        self.cwd.mkdir()
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.downloads = self.root / 'downloads'
        self.downloads.mkdir()
        self.calls = self.root / 'calls'
        self.calls.mkdir()
        self.archive_name = f'quipu-{TAG}-{TARGET}'
        self.fixture_archive = self.root / (self.archive_name + '.tar.gz')
        with tarfile.open(self.fixture_archive, 'w:gz') as bundle:
            for name in ('quipu', 'quipu-server', 'qp'):
                body = (f'#!/bin/sh\nprintf "%s\\n" "{name}" >> "$DOC_EXEC_LOG"\n'
                        f'printf "%s\\n" "{name} {VERSION}"\n').encode()
                info = tarfile.TarInfo(self.archive_name + '/' + name)
                info.size, info.mode = len(body), 0o755
                bundle.addfile(info, io.BytesIO(body))
        self.stale = self.cwd / (self.archive_name + '.tar.gz')
        self.stale.write_bytes(b'previous archive must not be used or removed')
        self.tools = {name: shutil.which(name) for name in ('tar', 'gzip')}
        self.assertTrue(all(self.tools.values()), 'fixture requires real tar and gzip')
        self.stub('curl', '''
url = next(arg for arg in sys.argv if arg.startswith('https://'))
out = Path(sys.argv[sys.argv.index('-o') + 1])
kind = 'checksum' if url.endswith('.sha256') else 'archive'
with open(os.environ['DOC_CURL_LOG'], 'a') as log: log.write(kind + '\\n')
if os.environ['DOC_MODE'] == 'fail-' + kind: raise SystemExit(22)
if kind == 'archive': shutil.copyfile(os.environ['DOC_FIXTURE'], out)
else:
    digest = hashlib.sha256(Path(os.environ['DOC_FIXTURE']).read_bytes()).hexdigest()
    if os.environ['DOC_MODE'] == 'bad-checksum': digest = '0' * 64
    out.write_text(digest + '  ' + Path(os.environ['DOC_FIXTURE']).name + '\\n')
''')
        self.stub('mktemp', '''
print(tempfile.mkdtemp(prefix='download-', dir=os.environ['DOC_TEMP_ROOT']))
''')
        self.stub('rm', '''
root = Path(os.environ['DOC_TEMP_ROOT']).resolve()
for arg in sys.argv[1:]:
    if arg.startswith('-'): continue
    path = Path(arg)
    if not path.resolve().is_relative_to(root): raise SystemExit('cleanup escaped fixture')
    if path.is_dir(): shutil.rmtree(path)
    elif path.exists(): path.unlink()
''')
        self.stub('tar', f'''
with open(os.environ['DOC_TAR_LOG'], 'a') as log: log.write('tar\\n')
raise SystemExit(subprocess.run([{self.tools['tar']!r}, *sys.argv[1:]], check=False).returncode)
''')
        self.stub('gzip', f'os.execv({self.tools["gzip"]!r}, [{self.tools["gzip"]!r}, *sys.argv[1:]])\n')

    def stub(self, name, body):
        path = self.bin / name
        path.write_text('#!' + sys.executable + '\n'
                        'import hashlib,os,shutil,subprocess,sys,tempfile\n'
                        'from pathlib import Path\n' + body)
        path.chmod(0o755)

    def example(self, heading):
        text = (ROOT / 'docs/book/src/getting-started/installation.md').read_text()
        return text.split('## ' + heading + '\n', 1)[1].split('```sh\n', 1)[1].split('\n```', 1)[0] + '\n'

    def run_archive(self, mode, checksum_tool):
        self.stub(checksum_tool, '''
with open(os.environ['DOC_CHECK_LOG'], 'a') as log: log.write('check\\n')
manifest = Path(sys.argv[-1]).read_text().split()
actual = hashlib.sha256(Path(manifest[1]).read_bytes()).hexdigest()
raise SystemExit(0 if manifest[0] == actual else 1)
''')
        env = {'HOME': str(self.root / 'home'), 'PATH': str(self.bin),
               'DOC_MODE': mode, 'DOC_FIXTURE': str(self.fixture_archive),
               'DOC_TEMP_ROOT': str(self.downloads)}
        if 'LD_LIBRARY_PATH' in os.environ:
            env['LD_LIBRARY_PATH'] = os.environ['LD_LIBRARY_PATH']
        for name in ('CURL', 'CHECK', 'TAR', 'EXEC'):
            env['DOC_' + name + '_LOG'] = str(self.calls / name.lower())
        return subprocess.run(['/bin/sh', '-c', self.example('Release archive')],
                              cwd=self.cwd, env=env, capture_output=True, text=True, timeout=20)

    def log(self, name):
        path = self.calls / name
        return path.read_text().splitlines() if path.exists() else []

    def test_documented_archive_bad_checksum_stops_before_extraction(self):
        for tool in ('sha256sum', 'shasum'):
            with self.subTest(tool=tool):
                self.run_case('bad-checksum', tool, False)

    def test_documented_archive_download_failures_stop_before_extraction(self):
        for mode in ('fail-archive', 'fail-checksum'):
            for tool in ('sha256sum', 'shasum'):
                with self.subTest(mode=mode, tool=tool):
                    self.run_case(mode, tool, False)

    def test_documented_archive_intact_checks_all_three_commands(self):
        for tool in ('sha256sum', 'shasum'):
            with self.subTest(tool=tool):
                self.run_case('good', tool, True)

    def run_case(self, mode, tool, success):
        # Each subcase gets independent stale files, logs and a single checksum tool.
        for name in ('sha256sum', 'shasum'):
            path = self.bin / name
            if path.exists(): path.unlink()
        for path in self.calls.iterdir(): path.unlink()
        extracted = self.cwd / self.archive_name
        if extracted.exists(): shutil.rmtree(extracted)
        self.stale.write_bytes(b'previous archive must not be used or removed')
        result = self.run_archive(mode, tool)
        if success:
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(self.log('exec'), ['quipu', 'quipu-server', 'qp'])
            self.assertEqual(self.log('tar'), ['tar'])
            self.assertEqual(self.log('check'), ['check'])
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual(self.log('tar'), [], 'tar invoked after failed download/checksum')
            self.assertEqual(self.log('exec'), [], 'executable invoked after failed download/checksum')
        self.assertEqual(list(self.downloads.iterdir()), [], 'download scratch left behind')
        self.assertEqual(self.stale.read_bytes(), b'previous archive must not be used or removed')


if __name__ == '__main__':
    unittest.main(verbosity=2)
