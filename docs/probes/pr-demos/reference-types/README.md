# Explicit reference-type context

This synthetic library fixture writes an Action in a project graph, then an
ephemeral Action that depends on it. The same ephemeral write is refused with
the default context and admitted with explicit `[ephemeral, project]` context.
The fixture exercises both new and existing destination graphs and asserts
foreign-project, ROOT-only, missing-target and pseudo-Action refusals, separate
cardinality, rollback and reverse-direction isolation.

Run from the repository root:

```sh
bash docs/probes/pr-demos/reference-types/run.sh
```

The [recording](demo.cast) preserves the original terminal output and timing in
asciicast v2 format. Download it and replay with an asciinema client:

```sh
asciinema play demo.cast
```

It demonstrates the candidate library's default and explicit scopes on identical
input. It does not demonstrate a deployed service, a Seeds integration, or the
historical binary. The source base is `ce63a435`; the demonstrated candidate is
the commit containing this fixture and recording. Retain these small artifacts
with the source so reviewers can reproduce the observation. Output-frame bytes
and timing were checked against the original `script` recording; inline playback
is not assumed.
