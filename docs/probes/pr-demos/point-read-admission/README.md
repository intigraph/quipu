# Point-read admission measurement

This terminal recording displays retained measurements from two private runs.
It does not replay load against a service. Run `python3 demo.py` to inspect the
truth controls and one-second assertion; `measurements.json` contains the data.
Replay the output and timing pair with:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

The baseline first point ASK took 9.23 seconds with 87 expensive requests in
flight. The candidate answered all six point probes correctly in 1–96 ms while
69–86 expensive requests remained in flight. The baseline's later probes ran
after load drained and do not discriminate scheduling. This experiment did not
reproduce a current client timeout.

Both runs used a fresh SQLite store containing 250 edges into a hub and 250
edges out, 90 concurrent `SELECT (COUNT(*) AS ?n) WHERE { ?s ?p ?o . ?o ?p2 ?o2 }`
requests, and known-true and known-false ground ASK controls. Every successful
COUNT returned 62500; other expensive outcomes were HTTP 408. Server caps were
CPU 25%, RAM 4 GiB, swap 0, tasks 64, two Tokio workers, pool size four,
10-second request admission and 30-second query timeout. The client driver
ran outside that cgroup. The candidate enabled `reserve_point_reads`; the
baseline did not. PID remained stable and every health sample succeeded.

Source: baseline 26e791ec, candidate 5fefecee. Later base changes touch only
CI/packaging. These measurements cover the narrow point-ASK profile, not
production owner-plate SELECT latency, full-store behavior or activation.
The feature remains off by default.
