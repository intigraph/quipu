# Signed reads through internal reader processes

Status: proposed security boundary; reader-process activation remains off.

The public listener verifies a signed read and durably consumes its nonce before
routing it. Forwarding the original attestation into a second verifier sharing
that nonce ledger causes a fresh public request to be rejected as a replay.
Binary rollback does not repair a configuration that still enables the pool.

## Proposed handoff

Keep signature verification, binding checks, strict-key policy and atomic nonce
admission exclusively on the public primary. Keep signed write handling there
unchanged. Do not remove the original attestation or make an invalid signature
fall back to another authentication method.

For an already-admitted signed read, the proxy transports the primary-derived
principal and credential identity over the existing authenticated private worker
hop. It may create this evidence only from the request-local authenticated
identity with a pending attestation whose state is already Spent. It must fail
closed if a signed read reaches the proxy without that proof.

The proxy always discards caller-supplied values of the reserved handoff header
and supplies its own bounded serialization. The existing random worker key,
private listener, fixed destination, redirect refusal and expected-PID response
check remain mandatory. The primary's replay ledger stays authoritative.

Only worker_guard, after validating that private key, may construct an opaque
trusted-read extension. Accept it only for the existing routed read allowlist,
with the original attestation present and a well-formed attested-session identity.
No public header can itself construct the extension. The worker auth layer
restores the authenticated principal/request-local identity from this extension;
it does not verify or spend the original attestation again. The deserialized
identity carries no mutable PendingAttestation and grants no write authority.
Read-only and external-replica write refusals remain ahead of this branch.

Preserve scoped-read authorization and key-derived attribution on the child.
Ordinary bearer requests, external replicas, protected reads, strict session-key
policy and invalid signatures retain their existing paths. Never infer an
identity from declared agent/session headers. Never add a primary fallback,
retry, public bypass flag, unsigned-read exception, or nonce-ledger mutation.

The existing worker key is the trust boundary, not a new bearer for clients.
A process that can read the primary/worker secret already controls this private
hop; this proposal does not expand the trust boundary to arbitrary local clients.
The hop is loopback-only and endpoints are selected by the owned worker roster.

## Required falsifiable proof

1. Real server, two real child processes, registered synthetic v2 signing key:
   signed write lands on primary; signed read-back succeeds through both child
   PIDs and the heavy lane. Do not accept a primary-served read as coverage.
2. Replay each accepted signed request through the public listener: refuse it.
   Wrong-key, changed-body, changed-path and revoked binding must still refuse.
3. Caller-supplied handoff evidence on the public listener cannot authenticate an
   unsigned request or rescue an invalid attestation. Direct access to each child
   without the private key is refused, even with a valid client attestation.
4. Malformed/missing handoff and unspent source evidence fail closed. Forwarded
   identities never authorize child writes. External replicas keep their policy.
5. Scoped profiles preserve the primary-derived principal; no declared-header
   substitution or shared-bearer identity is substituted for the signed identity.
6. The new real-server regression must fail on the old implementation, pass on
   the reviewed fix, and fail when the handoff is sabotaged. Restore and require
   PASS again. Run all retained process-pool/auth tests under the HOME guard.

Source review must approve this boundary before implementation. Production pool
re-enable remains a separate reviewed configuration and runtime admission after
regression proof; test success alone does not activate it.
