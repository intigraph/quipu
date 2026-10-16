# Native writer-lock metric control

Run `bash docs/probes/writer-lock-metrics/run.sh` from the repository root.
The private in-memory router executes the actual knot, stage and asynchronous
promotion handlers, verifies a terminal successful promotion and a populated
query control, and reports the completed writer hold. No production connection
or load replay is used.

Retained output: [output.log](output.log). The measured hold is a fixture
observation, not a throughput promise. A control mutation removing only detached
promotion context compiles but fails the same native test after its bounded
three-second wait: no endpoint-attributed writer hold arrives. The corrected
source passes. Other controls verify multiple holds, errors and unwinding,
excluded acquisition wait and deferred work, and real pooled readers.

Caption: detached promotion previously lacked endpoint-attributed held time;
its actual writer acquisition now contributes a positive completed hold while
pooled reads contribute none. Production installation and a served promotion
positive remain separate acceptance work.
