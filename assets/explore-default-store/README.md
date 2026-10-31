# Default-store startup demonstration

Open the same released 198,938-triple repository pendant in a fresh Mac Chrome
156 profile. [Before](before.webm) shows the first six seconds of full receiving;
it is still loading when the excerpt ends. The complete measured run reached
ready at **37.373 s**. [After](after.webm) restores the build-validated default
store and reaches ready at **1.260 s**, then answers a real native query.
Download the WebMs if the repository viewer does not play them inline.

These are source-checkout fixtures served over local HTTP, not installed Pages
acceptance. Base UI: `b6cf8acdcc2ed47386937f7a5c724887099a13d0`, with released
engine `ad8de357521acdbd9f7a0175a37b7c20d3a21f5c` (0.16.6).
Recorded candidate UI: `567630d03511e3f4f23d288682e4762407c51398`, with a locally
built, stripped candidate engine. Its build metadata says `git_sha: unknown`;
[source and runtime hashes](source-sha256.txt) identify the actual recorded files.
The following media/caption commit leaves the implementation unchanged.
[Recorded results](recorded-results.json) retain both real query controls.

A separate DevTools run on this candidate observed the actual ready-state marker
at **1.265 s cold / 0.880 s warm**, with no main-thread long tasks and maximum
heartbeat gaps of **50.8 / 51.2 ms**. Earlier polling included up to 200 ms of
observation delay; these values come from the marker's mutation timestamp.
Both runs restored a genuine native store and completed native queries.
The normal live page baseline measured **41.366 / 39.303 s**; the candidate's
live CDN/download acceptance remains open. The generated store adds **39,920,017
bytes compressed** (205,504,512 bytes raw); candidate WASM is **9,329,937 bytes**.

Pages staging uses the full native receiver, preserving both advisory warnings
and zero blocking violations. The generated proof records `conforms: false`;
it does not turn advisory findings into a clean report. Startup checks the exact
published pack, engine and store digests. The immutable store cache excludes
edits. Selected packs still use full receiving, and “Validate pack” reruns the
full checks in the worker. Export hashes are computed on demand.

The shared WASM smoke action stages a native-minted pack and the candidate
engine, then runs the producer and browser controls. With those artifacts staged:

```sh
just --command node scripts/prepare-explorer-default.mjs docs/book/src/explore
just --command env SITE_DIR=docs/book/src/explore node scripts/test-explore-default.cjs
```

The 23 browser controls cover positive native queries, independent agreement of
published graph views with live queries, real edits and reload isolation,
cache corruption recovery, explicit validation, selected-pack handling despite
a default-like file name, and damaged pack/store/engine/proof refusal. They ran
under the HOME-escape guard; all 157 watched paths and two UID resources were
unchanged. A first report-identity control failed and exposed a missing
cross-check; the final candidate binds import, promotion and manifest identities
and passes that control. This recording supports source review, not a claim of
release, deployment, network acceptance or completion of those later gates.

The two small clips and caption are retained here for review; generated stores,
engine binaries, private profiles and the uncut baseline recording stay outside
Git.
