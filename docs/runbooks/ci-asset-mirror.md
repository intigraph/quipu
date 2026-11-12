# Pinned CI asset mirror

Set the repository variable `CI_ARTIFACT_MIRROR_URL` to an approved HTTPS base
URL reachable by GitHub-hosted runners. The mirror is optional. CI restores an
`actions/cache` entry keyed by the native asset and its exact SHA256, using the
built-in GitHub Actions cache identity; it requires no new credential. Source
builds outside this CI preparation path retain native acquisition unchanged.

Objects use `<base>/sha256/<lowercase-sha256>/<upstream-filename>`. Populate an
object only from bytes independently verified against the existing upstream
pin. The generic fetcher verifies every response before atomically publishing a
local file. An unavailable or absent mirror object falls back to the restored
cache, then once to the original upstream URL. A checksum mismatch, access refusal, byte-budget excess
or unsafe redirect fails closed. URLs cannot contain credentials or query data.

For ONNX, `scripts/prepare-ci-onnx.py` selects the native target's URL and archive
pin from `build/onnx_catalogue.rs`; it does not maintain a second checksum list.
It sets `QUIPU_ORT_ARCHIVE` only after verification. The native build verifies the
same checksum again. Verified bytes are saved to the immutable cache before the
build, so a later source-test failure does not discard a successful acquisition.
An existing restored Cargo build archive may seed a missing dedicated entry only
after its full SHA256 matches the current catalogue pin.
Neither runtime acquisition nor release pins change. A cold cache still needs
one successful verified acquisition; cache configuration alone is not recovery.

The mirror is a CI availability aid. Configuring a URL does not authorize
credentials, widen package scopes, expose a new service, or waive source review.
Use an existing approved publisher and existing access scopes. Keep the mirror
unset until the host and read path are accepted. Remove the repository variable
to roll back mirror use; retain published immutable objects and their provenance.
Revert the CI action steps to remove the cache layer as well.

Run `python scripts/test-ci-asset-mirror.py` for the offline discrimination
controls. A passing fixture does not prove that a hosted runner can reach a
deployed mirror. Record a full remote GET and matching SHA256 before claiming
that acceptance.
