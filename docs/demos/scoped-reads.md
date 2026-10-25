# Scoped current reads: private library demonstration

P has query permission for graph A only. The same explicit query of B returns
`fixture-secret` through the unchanged legacy API and receives `scoped read
denied` through the scoped view. Watch the allowed-A positive control return
`true`, then the same owned view refuse its buffered publication recheck after
the owner commits a revocation.

This compares the two APIs in one candidate with synthetic in-memory data; it
does not run two server revisions. The original base is
`0d0b769d7311304583de2815c8f35b8e480b1991`, which lacks the scoped API. The
demonstrated product/example source is commit
`a3d6aa0e6f1e14fb39375c7f51608d214dd38fe3`, integrating main
`b3057958ab0a6e581d593b9a65e4f7c30d182cdf`. Later qualification commits change
tests and recording artifacts; the demonstrated product/example source is
unchanged. There is no network request, listener, credential
file, production store or runtime acceptance in this demonstration.

Download the [recorded output](scoped-reads.output.gz) and
[original timing](scoped-reads.timing), then replay with util-linux:

```sh
gzip -dc scoped-reads.output.gz > scoped-reads.output
scriptreplay --log-out scoped-reads.output --log-timing scoped-reads.timing --maxdelay 1
```

Replay was checked. The gzip contains the original output bytes, including CRLF;
the timing file is unchanged. This is a terminal recording requiring download
and `scriptreplay`, without an inline video player. The small recording is
retained in this repository beside the [readable transcript](scoped-reads.txt).
Compilation warnings are redirected by the reproduction script; failure still
prints diagnostics and returns a failing exit status. The recording ends with
`Demonstration exit: 0`.

Reproduce from the reviewed checkout:

```sh
bash docs/demos/scoped-reads.sh
```

The demonstration ran under the HOME/resource guard with no watched changes.
Paired real-store and actual-router tests, enforcement sensitivity controls and
CI supply the wider source qualification. R3 publication barriers/offline leases
and R4 operator admission, deployment and natural acceptance remain separate.
