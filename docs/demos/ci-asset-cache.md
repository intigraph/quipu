# Pinned archive cache during an upstream outage

Implementation `0b122eb3000ea2e30fbf85a921dc65eac7657c85` passed the full quality
gate. The recording uses that implementation's fetcher with an unavailable
upstream fixture: a cold acquisition fails with HTTP 504; the same request with
a checksum-verified cache seed succeeds without contacting upstream. A corrupted
cache refuses without fallback. The before state models existing uncached
acquisition; it does not claim an independently executed base binary.

No remote mirror, hosted cache delivery, credential, or production build is
exercised. A cold hosted cache still requires a successful verified acquisition
or a matching archive restored from the existing Cargo cache.

Replay: `scriptreplay --log-timing docs/demos/ci-asset-cache.timing --log-out
docs/demos/ci-asset-cache.output`. Fixture: [ci-asset-cache.py](ci-asset-cache.py).
