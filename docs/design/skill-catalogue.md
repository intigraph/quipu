# Pinned skill catalogue contract

The opt-in shape set `shapes/skill-catalogue.ttl` describes immutable releases,
profile pins and profiles in `urn:quipu:skills:`. It does not upgrade older Skill
observations into installable releases. This is source for review, not evidence
that a server has loaded this vocabulary or that a publisher is running.

A release identifies its skill, destination name, trigger description, owning
repository and source path, version, native `sha256:` share identity, render
specification, public/internal scope and supported harnesses. A pin repeats the
exact skill, version and share identity. A profile selects one or more pins,
requests Claude and/or Codex, and explicitly declares its public-only policy.
An empty profile cannot imply retirement of every installed skill.

The consumer must select an explicit graph and transaction, establish complete
query results and validate the selected profile, pins and releases together.
It must compare all three pin fields, reject duplicate skill identities or
names, enforce public-only policy and ensure every requested harness is
supported. SHACL validates structure; it does not establish the authority of a
repository, the confidentiality of a graph or the trustworthiness of a package.

Native pendant import and composition remain responsible for byte verification,
quarantine and authority admission. The selected render specification remains
subject to the renderer's path, hash, file ownership and atomic-publication
checks. A source path is provenance, never a path to open or a command to run.
The selector must not fetch arbitrary repository or render-specification IRIs.
Internal skills require an explicitly selected private dataset and must never
be published into a public catalogue or pendant.

Source publication, server shape activation, catalogue registration, trusted
materialization, scheduled propagation and loaded-version receipts are separate
steps. This change performs none of those runtime actions. The existing tooling
manifest and its owner remain unchanged. No new live harness entries are added.

The native `skill_catalogue_shapes` tests accept a three-release, two-harness
profile and reject missing required metadata, malformed native identities,
unknown scopes or harnesses, missing pins and empty profiles. Cross-record
public-only admission remains an explicit consumer test; a conforming internal
release does not become public merely because a profile refers to it.
