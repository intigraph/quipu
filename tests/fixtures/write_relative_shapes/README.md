# Relative shape document controls

Unmodified W3C data-shapes test fixtures, commit `9c863967`, paths:

- `data-shapes-test-suite/tests/core/property/minCount-002.ttl`
- `data-shapes-test-suite/tests/core/misc/deactivated-001.ttl`

Upstream: <https://github.com/w3c/data-shapes/tree/9c863967/data-shapes-test-suite/tests>.
These documents contain relative manifest IRIs. The paired integration control
uses them unchanged as shapes and checks both batch and write admission, with
absolute minimal data for their target nodes. It does not claim to run the
complete W3C suite or change the existing parser/validation protocol.
