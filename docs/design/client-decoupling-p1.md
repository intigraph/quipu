# Client decoupling, first wave: hooks, trust verifier, work-item queries

Status: design, 2026-11-20. Author: malcolm. Items: aegis-kao.1, aegis-kao.2,
aegis-kao.3 (parent aegis-kao). No code changes in this document.

## Rule

Quipu is a generic RDF/SPARQL knowledge-graph product. The other components of
the stack it ships with (shantytown/`st`, seeds, camayoc, chaski, shuttle,
yupana, bobbin, tambo, ...) depend on Quipu; Quipu depends on none of them.
Quipu source names no stack component: no component executable it runs, no
component file format it emits, no component vocabulary in its built-in
queries. Component specifics live in the component, or in data and
configuration that Quipu treats generically. (Operator ruling, 2026-11-20:
"Quipu should not be coupled to the stack, but the opposite".)

Scope of the rule as applied here:

- **Source and shipped behaviour.** The book and design docs may name a
  component *as an example* of something that consumes a generic contract
  (for example, "a hook manager such as `st` can register this manifest").
  Examples do not create a dependency; code paths do.
- **Agent harnesses are not stack components.** Claude Code and Codex are
  third-party runtimes that Quipu integrates with, in the same way a CLI
  writes shell completions for bash or zsh. Writing their settings is a
  harness integration, not stack coupling, and it stays in this wave.
  Whether harness integrations should leave the Quipu binary too (for example
  into the installer) is a separate question with its own item, aegis-kao.13.
  This wave does not decide it.
- **The shared Quechua vocabulary** is a stack component's vocabulary. Today's
  work-item queries inline its prefix; that inlining leaves with kao.3, so no
  separate item is needed.

This wave covers the three places where Quipu reaches *out* to a specific
client. The work is sized so each item lands, deploys and rolls back on its
own.

| Item | Today | Coupling |
|---|---|---|
| kao.1 | `quipu hooks install/uninstall/status` runs `st ops hooks register/unregister/check`; the bundle uses schema `st.hook-bundle/1` | Quipu runs a client executable and emits that client's format |
| kao.2 | Pack-pointer and endpoint trust run `tambo packs trust check[-endpoint]` | Quipu runs a client executable for a security decision |
| kao.3 | Built-in `brief_ground`, `brief_related`, `entity_work`, `cochanged_with` and the `quipu_cooccurrence` tool hard-code one tracker's provenance chain under one deployment's namespace | Quipu's built-in catalog carries a client's data model |

## kao.1: hooks without `st`

### Today

`src/hooks_install.rs` treats `hooks/quipu.bundle.json` (schema
`st.hook-bundle/1`) as the one source of truth. When `st ops hooks list`
succeeds (`st_available()`), install/uninstall call `st ops hooks register`
and `st ops hooks unregister quipu`, and status parses `st ops hooks check
--json` (`src/cli_hooks.rs` `via_st`). Otherwise Quipu writes the harness
settings itself (`settings.json` / `config.toml`) with a merge shared with
sibling tools.

### Target

- Quipu owns a **generic hook manifest**, `quipu.hooks/1`: the same fields it
  ships today (name, version, owner, roles, hooks[event, command, harnesses,
  evidence]). The per-harness fields name harnesses (third-party runtimes;
  see Scope), never a stack component. It is documented in the book as Quipu's contract. Any manager
  may consume it.
- `quipu hooks bundle` prints that manifest (it already exists as a verb).
  `quipu hooks install/uninstall/status` only ever write and read harness
  settings directly. There is no `st` branch and no `Command::new("st")`.
- A host that manages hooks centrally registers Quipu's manifest from its
  own side, for example `st ops hooks register <(quipu hooks bundle)`. Quipu
  is not involved, and that host's renderer stays its own business.
- `status` reports what is actually in the harness settings. A manager's
  own view of what it renders is the manager's check, not Quipu's.

### Migration

1. **Client first (shantytown):** accept `quipu.hooks/1` as an alias of its
   bundle schema, and register Quipu's manifest in its own provisioning (the
   tooling manifest already lists Quipu). Land and install.
2. **Quipu:** emit `schema: quipu.hooks/1`, delete `st_available`,
   `st_register` and `via_st`, and update the hook docs. Tests: install,
   uninstall and status with no `st` on `PATH` (the property under test is
   `PATH`; the fake HOME also holds no copy), and a negative control showing
   that nothing executes `st`.
3. Verify on a managed host: the rendered settings still carry Quipu's Stop
   hook after a settings re-emit (the reason the `st` branch existed).

### Rollback

Revert the Quipu change. The client keeps accepting both schema names, so the
order of rollback does not matter. No settings file is rewritten by the
revert itself.

### Risk

The `st` branch existed because a manager's re-emit dropped hooks that Quipu
had written directly. Step 1 must land, and step 3 must be observed, before
step 2 deploys on a managed host. Otherwise Quipu's Stop hook can disappear on
the next re-emit.

## kao.2: a configurable trust verifier

### Today

`src/cli_pointer.rs` (pack pointers) runs `tambo packs trust check <remote>
<sha> --pack <graph>` and fails closed on spawn error or nonzero exit.
`src/config/endpoint_pointers.rs` runs `tambo packs trust check-endpoint
<remote> <endpoint>` with a 5 s policy deadline and exit contract 0 = trusted,
3 = refused, other = unavailable. Repository identity comes from the
receiver's own command line, never from a downloaded manifest; that property
must survive.

### Target

- A configured **trust verifier**: `[quipu.trust] verifier = ["<program>",
  "<fixed args>..."]`. Quipu appends a documented argument vector:
  `pack <remote> <sha256-hex> <graph>` or `endpoint <remote> <endpoint>`.
- The exit contract becomes Quipu's documented contract: 0 trusted, 3 refused,
  anything else unavailable. The existing 5 s policy deadline and the request
  deadline apply. stdin is null and output is discarded, as today for
  endpoints.
- **Fail closed** when the verifier is unset, missing, killed or times out,
  exactly like a missing `tambo` today. An unset verifier is never "trust all".
- **The verifier is a security decision, so its configuration is protected.**
  The program must be an **absolute path**: no `PATH` lookup and no relative
  path. Today's `tambo` was a `PATH` lookup, so this also closes an existing
  gap. Quipu refuses (fails closed) when the program, any of its parent
  directories, or the configuration file that names it is writable by anyone
  other than the service owner or root. These checks run at every invocation,
  not only at startup, so a later permission change cannot silently weaken
  them. Tests cover a group-writable program, a world-writable config and a
  relative path: each is refused.
- **Repository identity still comes only from the receiver's own command
  line** (the remote the operator named), never from a downloaded manifest
  or pointer. The verifier receives that receiver-side value; nothing from
  the fetched pack chooses which repository is trusted.
- No program name appears in Quipu source. The book gives the installer's
  wrapper as one example of a verifier.

### Migration

1. **Installer (tambo):** add a thin `tambo packs trust verify <kind> ...`
   entry point that accepts Quipu's argument vector, or document a two-line
   wrapper script with the same contract. Land and release.
2. **Deployment config:** set `[quipu.trust] verifier` to that entry point,
   through the normal configuration path, on every host that loads pack or
   endpoint pointers. Read the config back.
3. **Quipu:** replace both `trust_command` functions with the configured
   verifier. Tests: a stub verifier for each exit code (0/3/other), unset →
   refused, deadline → refused, plus a negative control that no `tambo`
   literal remains.

### Rollback

Revert the Quipu change. The old `tambo` subcommands remain in the installer,
so pointer loads keep working. The extra config key is ignored by the old
binary.

### Risk

Step 3 must not deploy to a host where step 2 has not been applied, or every
pointer load there fails closed. That is safe but an outage of pointer loads.
The deploy gate for step 3 is a read-back of the verifier key on each target.

## kao.3: work-item queries as data

### Today

`src/mcp/named_query.rs` compiles four queries into the catalog. All four
walk the chain `WorkItem <-implements- GitCommit -modifies-> entity`, with
the `http://aegis.gastown.local/ontology/` and Quechua prefixes inlined.
`quipu_cooccurrence` (`src/mcp/governance.rs`, definition in
`src/mcp/definitions/policy_overlays.rs`) runs the same chain as a dedicated
tool and is the only one that is bitemporal (`valid_at`, `tx`). Callers:
yupana's briefing advertises `quipu_ask brief_ground|brief_related`, and the
chain itself is produced by camayoc's git-provenance ingress.

The stored named-query registry (quipu #79) already exists. Dispatch is
**built-in first**: a stored query with a built-in's name is accepted but
never dispatched (`a_builtin_name_is_never_shadowed_by_a_stored_one`). That
rule is what makes a gap-free migration possible.

### Target

- The four queries are **stored queries owned by the producer of the chain**
  (camayoc), shipped in its pack or provisioning with the same names,
  parameters and results. Quipu's catalog keeps only client-neutral entries
  (for example `entity_summary`).
- **Generic enabler, in Quipu:** `quipu_ask` accepts optional `valid_at` and
  `tx` and evaluates any named query (built-in or stored) under that temporal
  context, the same `TemporalContext` the dedicated tool uses. This is a
  capability for every client, not a work-item feature.
- `quipu_cooccurrence` becomes the stored query `work_item_cooccurrence`. The
  tool is kept for one release as a deprecated alias that forwards to
  `quipu_ask` with that name, then removed. While the alias exists it is the one
  place Quipu source still names a component concept. That is deliberate and
  time-boxed: the removal (step 4) belongs to the kao.3 assignee, who records
  the removal release on kao.3 when step 3 lands.

### Migration

1. **Quipu (generic):** `valid_at`/`tx` on `quipu_ask`, with tests on neutral
   fixtures (example.org): the same stored query answers differently as of
   two transactions. Land and deploy. Nothing changes for current callers.
2. **Camayoc:** ship the five stored queries, with the same names and
   parameter specs plus `work_item_cooccurrence`, through the stored-query
   load path at provisioning. Verify with `quipu_ask` (no name): each of the
   four names is listed twice, `source: builtin` and `source: stored`. Built-in
   still wins, so behaviour is unchanged.
3. **Quipu:** delete the four built-ins. Turn `quipu_cooccurrence` into the
   deprecated forwarder. Tests: dispatch reaches the stored copy. Acceptance on
   the deployment: before/after answers of `brief_ground`/`brief_related` for
   one item with recorded provenance, and of `entity_work` for one entity, are
   identical. For the bitemporal query, which is the only one that runs through
   the new temporal path: record `quipu_cooccurrence` for one work item that has
   recorded provenance, at one fixed `valid_at` and one fixed `tx` (both before
   the most recent provenance write for that item, so the as-of answer differs
   from the current one), before step 3. After step 3, `quipu_ask
   work_item_cooccurrence` with the same item, `valid_at` and `tx` must return
   the same rows in the same order, and the current (no `valid_at`/`tx`) answer
   must match too.
4. **Next release:** remove the `quipu_cooccurrence` alias after its callers
   use `quipu_ask` (`rg quipu_cooccurrence` across the stack repos = 0).

### Rollback

- Step 3: revert. The built-ins return and win again; the stored copies stay
  harmless.
- Step 2: unload the stored queries.
- Step 1: revert; the new parameters are optional.

### Risk

If step 3 deploys where step 2 never loaded, the four names become "unknown
named query". Yupana's briefing would then advertise handles that do not
redeem. The step 3 deploy gate is the step 2 read-back above, done on the
deployment that receives the binary.

## Order and sizing

Each item is independent. Suggested order by blast radius: kao.3 step 1 (pure
addition), kao.2 (fails closed, small), kao.1 (needs a managed-host re-emit
observation), then the removal steps. Every Quipu step is its own deploy,
with rollback = revert. The client-side steps land first in each item.

## Out of scope

Namespace migration of stored IRIs (aegis-kao.4 covers how code *obtains*
namespaces, not renaming them); the other kao children; any change to the
client tools' own internal formats beyond accepting Quipu's documented
contracts.
