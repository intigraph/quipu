# Page-bound signed actions

Status: proposed integration pattern. The embedded artifact assets are
read-only; this design does not enable signing, enroll a device, grant a new
writer, or turn a signature into approval to execute an action.

## Scope

A page can hold a signing key in its browser storage and submit actions that
Quipu verifies against an explicitly enrolled identity. Bind that identity to
an approved page deployment, a human, a device label and a narrow action scope.
The existing [signing plane](signing-plane.md) governs enrollment and registry
changes. Browser algorithms and the native verifier must actually interoperate;
do not infer that support from a successful explorer import.

Current mechanisms have separate contracts. Session request authentication
uses the protected session registry and the native Ed25519 attestation envelope;
P-256 is not an automatic fallback for that format. Governance human approval
uses its protected verifier scope and decision seal. The seal's source API is
not evidence of a shipped browser-facing decision route. WebAuthn verdict
schemes have separate activation and enrollment requirements. Preserve these
distinctions when choosing the action adapter; session write permission is not
human decision authority.

## Browser and enrollment

First probe WebAuthn in the actual artifact frame when origin and human
presence are required. Its client data and authenticator checks provide a
different assurance contract from a plain signature. Check the frame's
permissions policy, relying-party scope, user gesture and native verifier
support before enrollment; a failed or cancelled probe does not authorize an
unannounced downgrade. Origin binding still does not attest an exact URL path
or document revision.

For the explicitly accepted narrower device-key fallback, use a secure
context. Generate a non-extractable private signing key with only
the required usages, retaining it in that origin's IndexedDB. Select an
algorithm supported by both the browser and the native verification contract;
Ed25519 or ECDSA P-256 are candidates, not an implemented fallback agreement.
Display the public key and fingerprint. An authorized enrollment operation,
confirmed by the human, registers the public key, page/origin binding, device,
algorithm, scope and lifetime. Loss of browser storage requires a fresh key
and enrollment; it never silently replaces the old registry binding.

The guarantee needs precise wording. Non-extractable prevents ordinary key
export; it does not attest a physical device, a human gesture or the top-level
page URL. CryptoKey objects are serializable and page code can pass them through
structured cloning. Same-origin script also shares the origin's authority.
The [WebCrypto specification](https://www.w3.org/TR/webcrypto/#cryptokey-interface)
defines these properties. Accordingly, a server verifies the enrolled key and
its approved binding; a self-asserted `page_url` is not independent browser
origin attestation. Stronger page-only claims need a separately demonstrated
mechanism and threat model.

## Signed envelope and verification

Agree on a versioned canonical byte format before implementation. Include the
action/item identity, choice, note, intended verifier/audience, enrolled key
identifier, page binding, timestamp/expiry and a fresh single-use nonce. Display
the exact action before signing. Use the existing page's owner-only submission
rule; the account rule and signature are separate checks.

This proposed action envelope is not the existing signed-request wire format.
Reuse the native canonicalization and published test vectors when using native
request authentication, and bind its exact method, route, body and audience.
Do not substitute a JSON signature for its domain-separated canonical message.

The consumer checks the algorithm, exact canonical bytes, signature, enrolled
identity, active binding, scope, audience, expiry and nonce before accepting
the action. Record nonce consumption and accepted action atomically. An exact
retry must return its prior durable receipt; a conflicting replay is refused.
An invalid, revoked, expired or unknown key is refused, not downgraded to an
unsigned decision. A bridge accepting a request is not a verified decision.

Retain the canonical envelope, signature, key identity, binding revision,
verification result, receipt and provenance for independent re-verification.
Provide an audit query that separates accepted actions from refused attempts.
Keep revocation and reenrollment under the same protected registry authority.

## Acceptance before activation

Prove persistent key recovery on the actual published origin, supported native
verification, approved enrollment, one human-confirmed real action, durable
read-back and the audit query. Show refusal controls for altered bytes,
wrong-page binding, wrong scope, replay, expiry and revocation. Test origin
changes across reload and republish; do not assume an artifact frame origin is
permanent. Preserve the prior read-only page as rollback while this remains
unaccepted.
