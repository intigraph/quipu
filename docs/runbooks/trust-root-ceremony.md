# Trust-root ceremony: enrol the first human key

Once per store, ever (aegis-kzt0ql.9.4). After this, every further key,
edit or revocation of a human-tier registration is an amendment signed by
an enrolled human key. Nobody can run the bootstrap a second time.

## Before you start

- **Schedule it immediately after the gate is deployed.** Until the
  ceremony runs, anyone who can run `quipu` against the store file could
  bootstrap their own key first. Keep that window short.
- Run it on the quipu host, as the service user, against the live store.
  The server can stay up: SQLite serializes the single write.
- Have the device whose key you are enrolling. Ed25519 keys work today;
  hardware schemes (passkey, security key) need aegis-kzt0ql.9.2 deployed.
- Decide which decision policies this key may sign, in addition to the
  trust-root policy it always gets.

## Steps

1. Check that nothing has been enrolled yet:

       quipu trust-root status --db <store>

   It must print `bootstrapped no`. **If it prints `yes` and you did not
   run the ceremony, stop: that is a compromise. Escalate.**

2. Get the challenge for your key:

       quipu trust-root challenge --verifier stiwi --public-key <hex> --db <store>

   Note the `fingerprint` line.

3. Sign the printed challenge bytes with the device, exactly as printed,
   with no trailing newline. Keep the hex signature.

4. Enrol:

       quipu trust-root bootstrap --verifier stiwi --public-key <hex> \
           --pop-signature <hex> --attests <decision policy> --db <store>

5. **Compare fingerprints.** The `FINGERPRINT` line from step 4 must equal
   the fingerprint your device (or `ssh-keygen -lf` on your public key)
   shows, character for character. **Any mismatch means someone else
   bootstrapped first. Stop, do not use the store for human decisions, and
   escalate.** This step is the only check that the enrolled key is yours;
   proof of possession proves the key, not the person.

6. Record the ceremony on the bead: the time, the registration IRI, and
   the fingerprint you compared. Acknowledge the bootstrap alert.

7. Enrol a second device (recovery) as an amendment signed by the first,
   using `trust_root::digest_after` to compute the digest to sign. With
   only one enrolled device, losing it means no key can amend the
   registry, and the bootstrap cannot be re-run.

## Endorsing agent registrations (aegis-c97o)

Verdict verification trusts a verifier key only when its registration
chains to the trust root. A registration written by a plain graph write
(every registration from before aegis-c97o, and anything an agent writes
since) verifies nothing until the root endorses it. There is no grace
period: until endorsed, its verdicts report `trusted: false` with
`untrusted_reason: registration-not-chained-to-trust-root`, and the server
logs a warning naming the registration.

1. List what needs endorsing:

       quipu trust-root status --db <store>

   The `unendorsed` lines are `verifier<TAB>registration`. Check each one
   is a verifier you recognise, with the key you expect (read the
   registration's `aegis:publicKey`). Do not endorse anything you cannot
   account for: endorsing it trusts its key.

2. Get the challenge (the tier defaults to `agent`):

       quipu trust-root endorse-challenge --registration <iri> --db <store>

   It prints the digest, a fresh nonce, and the bytes to sign.

3. Sign those bytes with an enrolled human trust-root key, exactly as
   printed, with no trailing newline.

4. Write the endorsement:

       quipu trust-root endorse --registration <iri> --signer <your verifier name> \
           --nonce <nonce> --signature <hex> --db <store>

   The gate refuses it if the signature, the signer, or the registration's
   content changed since step 2. Re-run step 2 in that case.

After this, any change to that registration (a new key, a rotation, a
revocation) is itself an amendment. To revoke, sign an amendment that
retracts the `aegis:trustTier` marker (or the key).

Endorsement is not retroactive. Verdicts recorded before the endorsement
keep `trusted: false` (their `signature_valid` still reports `true`); only
verdicts recorded after it verify as trusted.

## Shuttle transition registrations (aegis-udr9)

The transition write gate requires the same governed ROOT registration as
verdict verification. A registration in shuttle's identity named graph does not
authenticate transitions, even if it carries its own `trustTier` marker or uses
the same registration IRI as a governed ROOT key. Existing named-graph metadata
is retained; it is not migrated or endorsed by a binary upgrade.

For a separately authorized operator migration:

1. Inventory the actual shuttle identity graph explicitly. `trust-root status`
   reports ROOT registrations; it is not an inventory of every named graph.
2. Compare each agent's public key and registration identity with the expected
   operator records. Do not treat the writer of a graph fact as its approver.
3. Through the normal governed write path, copy only the reviewed registration
   facts (type, verifier, key and intended scope) into ROOT. Resolve conflicting
   ROOT registrations explicitly; do not overwrite a governed key or copy an
   agent-supplied `trustTier` marker. Copying an unmarked key grants no trust.
4. Follow the signed endorsement steps above against that ROOT registration.
   Use the existing human root; this is not authority to bootstrap a replacement
   root, copy private keys or broaden an agent's scope.
5. Retain the exact source/runtime identity, root endorsement receipt and one
   scoped genuine transition receipt. Keep existing flags and rollout holds;
   source publication or an ordinary valid signature is not migration acceptance.

Until this is complete, an enforced transition whose key is unendorsed is
refused atomically. An agent-tier registration cannot endorse another key.
Revocation refuses later writes even when their claimed transition time is old.
Stored events remain historical data and are not revalidated by this write gate.
Do not disable verification as a rollback shortcut; a security rollback needs
its own controller disposition and preserved evidence.

## What this does not protect against

Root on the quipu host. Someone who can edit the database file, replace
the binary, or run `import`/`unpack`/`restore` is past this gate. The gate
stops graph writers (anything holding the write bearer), not hosts.

### Current registration validity

Current human/governed authority checks accept only open-ended ROOT facts:
registration type, verifier, key, policy scope and tier. The trust-root snapshot
seals that same open-ended set. A native assertion with a finite `valid_to` is
still ordinary graph data, even when its expiry is in the future; it cannot
supply current transition or human authority. Do not copy finite tier or key
rows into a migration as evidence of endorsement. Re-enrol the reviewed facts
through the existing human-signed ceremony. Revoke with a signed amendment;
there is no automatic expiry-to-authority conversion.

Historical transaction-bound lookups retain the existing revocation semantics.
This repair prevents new transition authorization by bounded unsigned facts; it
does not retrospectively certify stored events or arbitrary historical tiers.
