# Disposable signed changeset loop

This terminal recording shows a signed cut applied to a disposable replica,
a private name edit preserved while a concurrent master note catches up,
master acceptance with a real receipt, an identical retry adding zero
transactions, a bad-signature refusal and replica convergence containing both
edits. It uses actual library code, the real shared signer, registry gate,
stream applier and SHACL validation; keys and stores are synthetic and disposable.
It contacts no running service and does not install or activate a client.

The base is `5d2fe613acd7cd2c172b23e3af8b2a5bddd612bf`. That revision has no
client-changeset library entry point, so the same library call cannot run on it.
The recording demonstrates the new source fixture with its refusal control;
it does not claim a live-service before/after comparison. The PR caption pins
the demonstrated source commit. Build chatter is excluded; fixture output and
real timing are retained. This is a terminal replay, not a video or GIF.

From the repository root:

```sh
bash docs/probes/pr-demos/client-changeset/run.sh
scriptreplay --log-out docs/probes/pr-demos/client-changeset/output.log \
  --log-timing docs/probes/pr-demos/client-changeset/timing.log
```

[Readable transcript](output.log), [timing data](timing.log), and
[repeatable command](run.sh) are small committed review evidence, retained with
the source. Playback requires downloading the two logs and `scriptreplay`.
The portable browser bridge has a separate real compiled-Wasm smoke; it is not
shown in this native recording.

The library remains unwired and supports one complete graph with open valid
intervals. This demonstration does not establish full P3 acceptance, runtime
authority or completion of context, interval, rebase and resolver workflows.
See [the library boundary](../../../design/client-changeset-library.md).
