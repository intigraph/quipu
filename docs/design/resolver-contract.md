# Public resolver contract v1

`quipu::resolver_contract` supplies prospective resolver types, a deterministic
encoder and content-bound validation evidence. It does not plan a merge, execute
SHACL, reserve a writer or authorize admission. Existing BaseJournal bytes and
hashes remain unchanged. No private `plan_pair` integration is required.

## Source and time binding

`ProspectiveCandidate` binds store, epoch, base transaction, graph, base-slice
digest, explicit scope and exactly four named/versioned inputs: `base`, `ours`,
`theirs`, `decisions`. Scope `None` means the entire bound graph; `Some([])` means
an empty scope. Row, input, decision and operation order contribute to identity.
Canonical skolemized N-Triples terms omit the terminal dot. UTC timestamps use
canonical seconds. Data intervals must be nonempty. `Received` is separate
zero-duration history and never a BaseJournal data row.

`RetainedUtc` preserves a supplied timestamp. `AdmissionAssigned` remains
unresolved until the writer supplies its actual acceptance time.
`assign_admission_time` creates `AdmissionResolved` values, preserving that
origin and rechecking intervals. It never reads a clock or commits anything.

## Content-bound validator profile

`ValidatorManifest` has no default profile. Bind the implementation artifact,
dependency lock, compiled features and actual engine/parser/base-IRI/mode
options. Loaded shape and ontology snapshots carry exact names, versions and
bytes in strict name order. Combined shapes use two newline bytes between
entries; combined ontologies use one. Both source snapshots and combined bytes
are hashed. Bind the full staged graph, including same-transaction inference.

Incremental selection, selected loaded shapes, optional write-shape bytes,
context facts, budgets, coverage and fallback contract are separate fields.
An absent write-shape value differs from empty bytes. Bind the actual OWL cache
source digest; absence is unknown rather than proof of an empty ontology.
Constraint, verdict-recording and postcommit materialization flags remain
independent. Postcommit activity is never precommit validation evidence.

`bind_validation` only binds a caller's observed result. Missing compiled SHACL,
incomplete context, exceeded fact budget or unproved OWL cache provenance
downgrades apparent conformance to `Unknown`. Pending admission time produces
`RequiresAdmissionTime`. Neither result authorizes admission. This structural
contract cannot establish that the caller actually ran a validator.

After time assignment the writer must validate the actual staged graph again.
`bind_admission_validation` requires fresh conformance and matching resolved
times, and binds actual acceptance time and a transaction newer than the base.
Its output is unsigned evidence to embed in the existing authenticated master
receipt. Existing ownership, scope, workflow, signature and trust gates apply.

## Canonical encoding

SHA-256 covers a NUL-terminated domain followed by one recursively framed value.
Domains are `quipu-resolver-candidate-v1`,
`quipu-resolver-validator-manifest-v1` and
`quipu-resolver-admission-validation-v1`. DTO field and variant names use their
Serde JSON representation; unknown fields are refused. JSON whitespace and
object insertion order have no effect.

All lengths and unsigned integers are eight-byte big-endian values. Tags are:

| Value | Encoding |
| --- | --- |
| Null | `n` |
| Boolean | `b0` or `b1` |
| Unsigned integer | `u`, integer |
| UTF-8 string | `s`, byte length, bytes |
| Nonempty array entirely of integers 0 through 255 | `v`, count, octets |
| Other array, including empty | `a`, count, framed elements |
| Object | `o`, count, sorted key/value entries |

Object keys sort lexically by UTF-8 bytes. Each key is encoded as its byte
length and bytes, without a string tag, followed by the framed value. Arrays
retain order. Negative and floating-point numbers are refused. Digests are
32-octet arrays. Empty byte arrays use the same canonical empty-array form.

## Joint vectors

`test-fixtures/resolver-contract-v1.json` contains 25 synthetic, independently
encoded vectors with byte lengths, SHA-256 values, base golden bytes and an
admission binding. They distinguish source changes, scope, order, temporal
origin, registry bytes/versions, cache provenance, context, flags and builds.
These public fixture profiles describe synthetic data, not a production
validator acceptance claim. Source adapters must supply the actual snapshot
described above; joint vector agreement does not establish live admission.
