#!/usr/bin/env python3
"""Select the newest public release with a complete explorer asset pair."""
import json
import re
import sys


def select_release(pages):
    releases = [r for page in pages for r in page] if pages and isinstance(pages[0], list) else pages
    candidates = []
    for release in releases:
        if release.get('draft') or release.get('prerelease'):
            continue
        tag = release.get('tag_name', '')
        published = release.get('published_at')
        if not re.fullmatch(r'quipu-ai-v\d+\.\d+\.\d+', tag) or not published:
            continue
        assets = {a['name'] for a in release.get('assets', []) if a.get('size', 0) > 0}
        prefix = 'quipu-' + tag
        wasm = prefix + '-wasm.tar.gz'
        packs = [prefix + '-repository.' + kind + '.tar.gz' for kind in ('pendant', 'qpack')]
        pack = next((p for p in packs if {p, p + '.sha256'} <= assets), None)
        if pack and {wasm, wasm + '.sha256'} <= assets:
            candidates.append(dict(tag=tag, published=published, pack=pack, wasm=wasm))
    if not candidates:
        raise ValueError('no published release has a complete repository pack and explorer bundle')
    return max(candidates, key=lambda r: r['published'])


if __name__ == '__main__':
    try:
        print(json.dumps(select_release(json.load(sys.stdin))))
    except (ValueError, TypeError, KeyError) as error:
        sys.exit(str(error))
