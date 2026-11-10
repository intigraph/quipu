#!/usr/bin/env python3
"""Exercise the actual store-free CLI against a private authenticated HTTP fixture."""
import base64
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
from urllib.parse import parse_qs, urlsplit

binary = str(Path(sys.argv[1]).resolve())
state = {"mode": "normal", "requests": 0}
authorization = "Basic " + base64.b64encode(b"fixture-user:fixture-password").decode()


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        assert self.headers.get("Authorization") == authorization
        state["requests"] += 1
        if state["mode"] == "redirect":
            self.send_response(302)
            self.send_header("Location", "http://127.0.0.1:1/must-not-follow")
            self.end_headers()
            return
        query = parse_qs(urlsplit(self.path).query)["query"][0]
        series = []
        if state["mode"] != "empty":
            if "_bucket" in query:
                series = [{"metric": {"endpoint": "private-host.internal", "le": bound},
                           "values": [[172800, "100"], [259200, "100"]]}
                          for bound in ["0.1", "+Inf"]]
            elif "client_requests" in query:
                series = [{"metric": {"client": "private-agent.internal", "task": "private-task"},
                           "values": [[172800, "10"], [259200, "20"]]}]
            else:
                value = "1440" if "count_over_time(up" in query else "2880" if "count_over_time" in query else "1"
                series = [{"metric": {"instance": "private-instance.internal"},
                           "values": [[172800, value], [259200, value]]}]
        response = {"status": "success", "data": {"resultType": "matrix", "result": series}}
        if state["mode"] == "partial":
            response["warnings"] = ["private backend warning"]
        body = b"x" * (4 * 1024 * 1024 + 1) if state["mode"] == "oversize" else json.dumps(response).encode()
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass


with tempfile.TemporaryDirectory(prefix="public-slo-", dir=Path.home()) as temporary:
    root = Path(temporary)
    (root / ".bobbin").mkdir()
    (root / ".bobbin/config.toml").write_text("INVALID private-config.internal [")
    secret = root / "credential"
    secret.write_text("fixture-password\n")
    secret.chmod(0o600)
    environment = {key: value for key, value in os.environ.items()
                   if key not in {"PROMETHEUS_BEARER_TOKEN_FILE", "PROMETHEUS_PASSWORD_FILE", "PROMETHEUS_USERNAME"}}
    environment.update(PROMETHEUS_PASSWORD_FILE=str(secret), PROMETHEUS_USERNAME="fixture-user")
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    command = [binary, "report", "--public", "--prometheus", f"http://127.0.0.1:{server.server_port}",
               "--days", "2", "--end", "1970-01-04T00:00:00Z"]
    try:
        reports = []
        for _ in range(2):
            before = state["requests"]
            done = subprocess.run(command, cwd=root, env=environment, capture_output=True, text=True, timeout=30)
            assert done.returncode == 0, done.stderr
            assert not done.stderr
            assert state["requests"] - before == 8
            report = json.loads(done.stdout)
            assert len(report["days"]) == 2
            row = report["days"][0]
            assert row["date"] == "1970-01-02"
            assert row["scrape_up"] == 1 and row["scrape_samples"] == 1440
            assert next(iter(row["latency_seconds"].values()))["p95"] == 0.095
            assert list(row["completed_requests_by_client"].values()) == [10]
            for private in ["private-host", "private-agent", "private-task", "private-instance", "fixture-password"]:
                assert private not in done.stdout + done.stderr
            reports.append(report)
        assert reports[0] != reports[1], "public label maps must be random and discarded"
        for mode in ["partial", "redirect", "oversize"]:
            state["mode"] = mode
            done = subprocess.run(command, cwd=root, env=environment, capture_output=True, text=True, timeout=30)
            assert done.returncode != 0 and not done.stdout
            assert "127.0.0.1" not in done.stderr and "private" not in done.stderr
        state["mode"] = "empty"
        done = subprocess.run(command, cwd=root, env=environment, capture_output=True, text=True, timeout=30)
        assert done.returncode == 0, done.stderr
        assert json.loads(done.stdout)["days"][0]["scrape_up"] is None
        assert sorted(path.name for path in root.iterdir()) == [".bobbin", "credential"]
        print("PASS actual CLI: eight authenticated reads; no graph/config open; private label controls; random maps; partial/redirect/size refusals; missing-data null")
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
