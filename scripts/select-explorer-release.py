#!/usr/bin/env python3
"""Select the public release whose explorer asset pair the Pages build stages.

Normally the newest complete release. While PINNED_TAG is set, exactly that
release is used, and it must be complete; there is no silent fallback.
"""
import json
import re
import sys

# UNPINNED 2026-11-26 (aegis-1o7r). The pin to quipu-ai-v0.18.0 (aegis-x5m)
# existed because the v0.18.2 pack (110 MB expanded, 359,874 triples) trapped
# the wasm32 receiver during SHACL validation. The release pack no longer ships
# export.ttl or the per-node owl:sameAs aliases: v0.19.5 is 65.2 MB expanded,
# 327,493 triples. The Pages build validates the selected pack with this
# checkout's engine and runs the browser loading suite before publishing, so a
# pack that no longer loads fails the build. To re-pin, set a complete tag here.
PINNED_TAG = None


def select_release(pages, pinned=None):
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
    if pinned is not None:
        candidates = [r for r in candidates if r['tag'] == pinned]
        if not candidates:
            raise ValueError(f'pinned explorer release {pinned} is missing or incomplete')
    if not candidates:
        raise ValueError('no published release has a complete repository pack and explorer bundle')
    return max(candidates, key=lambda r: r['published'])


if __name__ == '__main__':
    try:
        print(json.dumps(select_release(json.load(sys.stdin), pinned=PINNED_TAG)))
    except (ValueError, TypeError, KeyError) as error:
        sys.exit(str(error))
