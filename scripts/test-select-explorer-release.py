import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('selector', Path(__file__).with_name('select-explorer-release.py'))
selector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(selector)


def release(version, date, pack=True, kind='pendant'):
    tag = 'quipu-ai-v' + version
    names = ['quipu-' + tag + '-wasm.tar.gz']
    if pack:
        names += ['quipu-' + tag + '-repository.' + kind + '.tar.gz']
    return dict(tag_name=tag, published_at=date, assets=[dict(name=n+s, size=10) for n in names for s in ('', '.sha256')])


class Selection(unittest.TestCase):
    def test_latest_complete(self):
        self.assertEqual(selector.select_release([release('0.15.0', '2026-10-20'), release('0.14.1', '2026-10-19')])['tag'], 'quipu-ai-v0.15.0')

    def test_latest_missing_uses_previous_complete(self):
        self.assertEqual(selector.select_release([[release('0.15.0', '2026-10-20', False)], [release('0.14.1', '2026-10-19')]])['tag'], 'quipu-ai-v0.14.1')

    def test_all_missing_fails(self):
        with self.assertRaises(ValueError):
            selector.select_release([release('0.15.0', '2026-10-20', False)])

    def test_empty_fails(self):
        with self.assertRaises(ValueError):
            selector.select_release([])

    def test_legacy_complete_pack(self):
        self.assertIn('.qpack.', selector.select_release([release('0.11.0', '2026-10-01', kind='qpack')])['pack'])

    def test_missing_checksum_and_draft_rejected(self):
        r = release('0.15.0', '2026-10-20'); r['assets'].pop()
        d = release('0.16.0', '2026-10-21'); d['draft'] = True
        with self.assertRaises(ValueError):
            selector.select_release([r, d])

    def test_metadata_only_no_private_content(self):
        r = release('0.15.0', '2026-10-20'); r['body'] = 'PRIVATE-CONTROL'
        result = selector.select_release([r])
        self.assertEqual(set(result), {'tag', 'published', 'pack', 'wasm'})
        self.assertNotIn('PRIVATE-CONTROL', str(result))

    def test_pin_selects_the_older_release_even_when_newer_is_complete(self):
        pages = [release('0.18.2', '2026-11-18'), release('0.18.0', '2026-11-13')]
        self.assertEqual(selector.select_release(pages, pinned='quipu-ai-v0.18.0')['tag'], 'quipu-ai-v0.18.0')

    def test_pin_never_falls_back(self):
        for pages in ([release('0.18.2', '2026-11-18')],
                      [release('0.18.2', '2026-11-18'), release('0.18.0', '2026-11-13', False)]):
            with self.subTest(pages=len(pages)):
                with self.assertRaises(ValueError):
                    selector.select_release(pages, pinned='quipu-ai-v0.18.0')

    def test_pages_build_selects_the_latest_complete_release(self):
        pages = [release('0.19.5', '2026-11-25'), release('0.18.0', '2026-11-13')]
        self.assertEqual(selector.select_release(pages, pinned=selector.PINNED_TAG)['tag'], 'quipu-ai-v0.19.5')


if __name__ == '__main__':
    unittest.main()
