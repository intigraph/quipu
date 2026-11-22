"""Installed-binary contract: structural views and owned presentation differ."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
PREFIX = '@prefix a: <http://aegis.gastown.local/ontology/> . @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n'
STRUCTURAL = '''<urn:module> a a:CodeModule ; a:filePath "src/new.rs" ; a:repo "fixture" ; a:language "rust" .
<urn:doc> a a:Document ; a:filePath "guide.md" .
<urn:section> a a:Section ; a:headingDepth 1 ; a:inDocument <urn:doc> .'''


@unittest.skipUnless(os.environ.get('QUIPU_SHAPE_NATIVE') == '1', 'opt-in installed binary; no build or fleet writes')
class StructuralViews(unittest.TestCase):
    def check_data(self, body, conforms):
        binary = shutil.which('quipu')
        self.assertIsNotNone(binary)
        with tempfile.TemporaryDirectory(prefix='structural-shapes-') as temporary:
            root = Path(temporary)
            data = root / 'fixture.ttl'
            data.write_text(PREFIX + body)
            # Explicit files only: validate does not open a database or HTTP.
            result = subprocess.run([binary, 'validate', '--shapes', str(ROOT / 'shapes/code-entities.ttl'),
                                     '--data', str(data)], capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0 if conforms else 1, result.stdout + result.stderr)
            if conforms:
                self.assertEqual(result.stdout.strip(), 'valid (0 warnings)')
            else:
                self.assertIn('invalid:', result.stdout)
                self.assertIn('Violation', result.stdout)

    def test_shared_structural_projection_without_owner_fields(self):
        self.check_data(STRUCTURAL, True)

    def test_owner_metadata_still_accepted_when_present(self):
        self.check_data(STRUCTURAL + '\n<urn:module> rdfs:label "src/new.rs" .\n'
                        '<urn:doc> rdfs:label "guide.md" .\n'
                        '<urn:section> rdfs:label "Guide / Section" ; a:heading "Guide / Section" .', True)

    def test_present_presentation_values_remain_typed(self):
        for body in ('<urn:module> rdfs:label 42 .', '<urn:doc> rdfs:label 42 .',
                     '<urn:section> rdfs:label 42 .', '<urn:section> a:heading 42 .'):
            with self.subTest(body=body):
                self.check_data(STRUCTURAL + '\n' + body, False)

    def test_structural_requirements_remain_required(self):
        for old in ('a:repo "fixture" ; ', 'a:language "rust"', 'a:filePath "guide.md"', 'a:headingDepth 1 ; '):
            with self.subTest(old=old):
                # Preserve valid Turtle when removing a terminal property.
                body = STRUCTURAL.replace('; ' + old, '') if old in ('a:language "rust"', 'a:filePath "guide.md"') else STRUCTURAL.replace(old, '')
                self.check_data(body, False)

    def test_owned_symbol_and_chunk_still_require_labels(self):
        self.check_data('<urn:symbol> a a:CodeSymbol ; a:name "run" ; a:symbolKind "function" ; a:definedIn <urn:module> .', False)
        self.check_data('<urn:chunk> a a:Chunk .', False)
        self.check_data('<urn:chunk> a a:Chunk ; rdfs:label "span" .', True)
        # Dual typing cannot use the structural Section exception to bypass Chunk.
        self.check_data('<urn:section> a a:Section, a:Chunk ; a:headingDepth 1 .', False)


if __name__ == '__main__':
    unittest.main()
