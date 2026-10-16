# Read-only replica demo

This is a replayable terminal recording of a **scratch candidate**, not an
installed replica. The retained baseline used installed server `8170851c` and
its legacy HTTP-only `server.read_only` flag: 100 positive reads, writes refused
403, and this fixture's DB counter/mtime unchanged. It did not reproduce the
separate CLI-open mutation observed by the replication owner.

The candidate is based on `4d1dcfe7` plus the read-only-replica proposal alongside
this file. It uses the new opt-in `server.replica` configuration. Watch writes
change to **503 naming the primary**, with 100 positive reads, unchanged main DB
hash/counter/mtime, no pending-subscription delivery, and lag **7 → UNKNOWN**
after the fake primary fails. The paired read/file checks use the same kind of
seeded SQLite fixture; configuration necessarily differs for the new role.

- [Readable output](output.log) and [original timing](timing.log).
- Replay after downloading both files:
  `scriptreplay --log-out output.log --log-timing timing.log`.
- Reproduce from the repository root with candidate/compatible binaries:
  `IF3_SERVER=/path/to/quipu-server IF3_CLI=/path/to/quipu bash docs/probes/pr-demos/read-only-replica/run.sh`.

Playback was checked locally with util-linux `scriptreplay`; no inline player is
promised. These small sanitized artifacts stay in the repository with the
proposal. The recording prints a **retained** baseline result, then executes the
candidate probe; it does not rerun the baseline server. It uses no production
endpoints, credentials, replication stream or real replica activation.
