# Classification travels with a new shape

This private-fixture terminal recording adds the same synthetic Turtle file to
a copied source tree. The existing invariant checker accepts it; the new
mandatory manifest checker refuses it while unclassified. An explicit exclusion
passes without selecting the profile, and an explicit load selects its stable
registry name. Neither classification calls a service or changes a live schema.

The baseline invariant checker is unchanged from source base `bdd3b6e2`; the
candidate implementation is the commit containing this recording. This proves
source classification and plan selection, not installed consumer acceptance.

Reproduce from the repository root:

```sh
python3 docs/demos/shape-manifest/demo.py
```

Download [output.log](output.log) and [timing.log](timing.log), then replay with:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

The output log is also the readable transcript. The small sanitized recording
is retained with this reproducer so it remains available after review.
