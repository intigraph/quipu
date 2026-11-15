# Deployment skip prefix proof

`deploy-skip-prefixes.json` is the single prefix list for deployment consumers.
It currently names only `benchmark/census/agent/`. A passing proof does not
activate skipping: the deployment consumer must separately adopt the merged list
and retain its immutable base/target input census and fail-to-deploy behavior.

Run `just deploy-skip test` and `just deploy-skip check`. CI runs both on every
pull request and main push, including commits that change only source. Release
correctness requires this job to succeed. This prevents an earlier source commit
from introducing a file dependency that a later data-only commit would hide.

The checker inventories tracked `src/`, `build/`, `build.rs` and `Cargo.toml`
inputs. It conservatively rejects even comments mentioning the listed directory's
root and requires a real include macro as a positive control. Missing inventory,
nonregular source, malformed paths and missing prefix contents refuse proof.
This is a literal-reference proof, not a general analysis of computed filesystem
access; deployment must continue refusing unknown inputs.

`docs/` is deliberately absent: current build/source inputs reference it.
Runtime source, shapes and Cargo/build metadata cannot be prefix entries.
New prefixes require the same standing proof before consumer adoption; there is
no second deployment-maintained prefix list to drift from this one.
