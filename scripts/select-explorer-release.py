#!/usr/bin/env python3
"""Select the public release whose explorer asset pair the Pages build stages.

Normally the newest complete release. While PINNED_TAG is set, exactly that
release is used, and it must be complete; there is no silent fallback.
"""
import json
import re
import sys

# PINNED 2026-11-18 (aegis-x5m). The v0.18.2 repository pack (359,444 triples)
# needs about 4.5-4.8 GiB to receive with SHACL validation, measured natively in
# docs/probes/receiver-shacl-memory. The browser engine is wasm32, capped at
# 4 GiB, and traps while loading it. v0.18.0 is the newest pack that validates
# in the explorer. Unpin (set None) only when the receiver validation-memory fix
# (aegis-8y0t) has landed AND a newer pack validates in this same Pages build.
PINNED_TAG = 'quipu-ai-v0.18.0'


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
