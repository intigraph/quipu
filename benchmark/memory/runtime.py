"""Run-scoped real SQL server, with loopback bind and guaranteed child cleanup."""

import contextlib
import json
import os
import shutil
import socket
import subprocess
import time
from pathlib import Path

import pymysql


def port_open(port):
    with socket.socket() as sock:
        sock.settimeout(0.2)
        return sock.connect_ex(("127.0.0.1", port)) == 0


def sql(connection, query, params=None):
    with connection.cursor() as cursor:
        cursor.execute(query, params)
        result = cursor.fetchall()
        while cursor.nextset():
            pass
        return result


@contextlib.contextmanager
def server(root, seed, port):
    dolt = shutil.which("dolt")
    if dolt is None:
        raise RuntimeError("Dolt unavailable: no measurement")
    if port_open(port):
        raise RuntimeError("fixture port already occupied; refusing")
    data = Path(root) / str(seed)
    data.mkdir(parents=True, exist_ok=False)
    home = data / "home"
    home.mkdir()
    env = dict(os.environ, HOME=str(home), DOLT_ROOT_PATH=str(data / "dolt-root"))
    for key, value in (
        ("user.name", "Memory benchmark"),
        ("user.email", "benchmark@example.org"),
        ("metrics.disabled", "true"),
    ):
        subprocess.run(
            [dolt, "config", "--global", "--add", key, value],
            cwd=data,
            env=env,
            check=True,
            capture_output=True,
        )
    version = subprocess.check_output([dolt, "version"], env=env, text=True).strip()
    log = open(data / "server.log", "w")
    child = subprocess.Popen(
        [
            dolt,
            "sql-server",
            "--host",
            "127.0.0.1",
            "--port",
            str(port),
            "--data-dir",
            str(data),
            "--socket",
            str(data / "mysql.sock"),
            "--event-scheduler",
            "OFF",
        ],
        cwd=data,
        env=env,
        stdout=log,
        stderr=subprocess.STDOUT,
    )
    connection = None
    identity = {
        "version": version,
        "host": "127.0.0.1",
        "port": port,
        "data_dir": str(data),
        "pid": child.pid,
        "stopped": False,
    }
    try:
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            if child.poll() is not None:
                raise RuntimeError("SQL fixture exited; inspect server.log")
            try:
                connection = pymysql.connect(
                    host="127.0.0.1", port=port, user="root", autocommit=True, charset="utf8mb4"
                )
                break
            except pymysql.Error:
                time.sleep(0.1)
        if connection is None:
            raise RuntimeError("SQL fixture readiness timed out")
        sql(connection, "CREATE DATABASE memory_bench")
        sql(connection, "USE memory_bench")
        identity["served_version"] = sql(connection, "SELECT DOLT_VERSION()")[0][0]
        yield connection, identity
    finally:
        if connection is not None:
            connection.close()
        if child.poll() is None:
            child.terminate()
        try:
            child.wait(timeout=15)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=5)
        log.close()
        identity["exit_code"] = child.returncode
        identity["stopped"] = child.poll() is not None and not port_open(port)
        (data / "runtime.json").write_text(json.dumps(identity, indent=2) + "\n")
        if not identity["stopped"]:
            raise RuntimeError("Fixture cleanup failed")
