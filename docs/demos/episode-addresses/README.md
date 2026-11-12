# Explicit episode addresses

The synthetic in-memory fixture contrasts an omitted `iri` with an explicit
`urn:example:worker`. Watch the actual ASK results: the omitted-address control
uses the fixture's legacy base, while the explicit request creates exactly the
requested subject. A malformed relative address refuses without a transaction.
This records candidate source behavior, not an installed server or a namespace
allocation. The omitted-address control is not a run of an older binary.

Implementation revision: `3837e04eb33d43d3226bfd40555638681d15c5d7`.
The retained example and recorder accompany this caption; subsequent changes
in this demo commit add only the example, recording and caption.

Run `python3 docs/demos/episode-addresses/reproduce.py` from the repository.
It runs the actual `episode_addresses` Rust example with no default features.
All writes stay in fresh in-memory stores, and the configured legacy base is
also a synthetic `example.org` fixture. No registry policy is applied.

[Recording/transcript](output.log) and [timing](timing.log) are retained with
the source. Download both and replay with
`scriptreplay --log-out output.log --log-timing timing.log`.
The recording's command exited zero. Playback requires a terminal replay tool;
no browser embed is claimed. These small text artifacts follow the repository's
existing `docs/demos` retention convention.
