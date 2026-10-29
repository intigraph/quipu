#!/usr/bin/env python3
"""Bounded, restart-safe private outcome journal reader (Python 3.11+).

Consumer state is not a second capture store: source JSONL remains authoritative.
No network, query execution, journal deletion, or request-counter replay occurs.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import sqlite3
import stat
import sys
import time

SURFACES = {"library", "cli_local", "http", "mcp"}
OPERATIONS = {"search", "hybrid_search", "search_nodes", "search_facts", "search_query", "query", "named_query"}
OUTCOMES = {"hits", "empty", "no_context", "error", "timeout", "refused", "unknown"}
REASONS = {"no_result_above_floor", "top_result_below_floor", "identifier_not_covered",
           "insufficient_term_coverage", "subject_not_covered", "query_too_long", "query_terms_unavailable"}
FILE = re.compile(r"search-[0-9a-f]{32}\.jsonl\Z")
IDENTITY = re.compile(r"[0-9a-f]{32}\Z")


def private_directory(path):
    if not path.is_absolute():
        raise ValueError("private directory must be absolute")
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o077:
        raise ValueError("directory must be private, owned, and not a symlink")


def connect(path):
    private_directory(path.parent)
    try:
        fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY | os.O_NOFOLLOW, 0o600)
    except FileExistsError:
        info = path.lstat()
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o077:
            raise ValueError("state must be a private owned regular file") from None
    else:
        os.close(fd)
    db = sqlite3.connect(path, timeout=1)
    version = db.execute("PRAGMA user_version").fetchone()[0]
    if version not in (0, 2):
        db.close()
        raise ValueError("unsupported consumer state version")
    db.executescript("""
        PRAGMA synchronous=FULL;
        CREATE TABLE IF NOT EXISTS files (
            name TEXT PRIMARY KEY, device INTEGER, inode INTEGER, offset INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS events (
            id TEXT PRIMARY KEY, digest TEXT NOT NULL, surface TEXT NOT NULL,
            operation TEXT NOT NULL, outcome TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS counts (
            surface TEXT, operation TEXT, outcome TEXT, value INTEGER NOT NULL,
            PRIMARY KEY(surface,operation,outcome));
        CREATE TABLE IF NOT EXISTS zeros (
            surface TEXT, operation TEXT, kind TEXT, value INTEGER NOT NULL,
            PRIMARY KEY(surface,operation,kind));
        CREATE TABLE IF NOT EXISTS misses (
            surface TEXT, operation TEXT, reason TEXT, value INTEGER NOT NULL,
            PRIMARY KEY(surface,operation,reason));
    """)
    if version == 0:
        if db.execute("SELECT 1 FROM events LIMIT 1").fetchone():
            db.close()
            raise ValueError("legacy nonempty consumer state needs explicit migration")
        db.execute("PRAGMA user_version=2")
    return db


def validate(raw):
    event = json.loads(raw)
    if not isinstance(event, dict) or event.get("schema") != "quipu-search-outcome/v1":
        raise ValueError("unknown event schema")
    identity = event.get("logical_call_id")
    summary = event.get("summary")
    if not isinstance(identity, str) or not IDENTITY.fullmatch(identity) or not isinstance(summary, dict):
        raise ValueError("invalid event identity or summary")
    surface, operation, outcome = event.get("surface"), event.get("operation"), summary.get("outcome")
    if surface not in SURFACES or operation not in OPERATIONS or outcome not in OUTCOMES:
        raise ValueError("unknown bounded label")
    digest = hashlib.sha256(raw).hexdigest()
    kind = zero_kind(summary)
    reason = None
    if outcome in {"empty", "no_context"}:
        reason = {"no_candidates": "no_candidates", "empty_rows": "empty_rows"}.get(kind)
        if reason is None:
            details = event.get("details")
            reported = details.get("context_reason") if isinstance(details, dict) else None
            reason = reported if isinstance(reported, str) and reported in REASONS else (
                "unknown" if reported is None else "other")
    return (identity, digest, surface, operation, outcome), kind, reason


def zero_kind(summary):
    def count(key):
        value = summary.get(key)
        return value if type(value) is int and value >= 0 else None
    outcome, kind = summary.get("outcome"), summary.get("zero_kind")
    if outcome == "empty" and kind == "no_candidates" and count("candidate_count") == 0:
        return kind
    if outcome == "empty" and kind == "empty_rows" and count("result_count") == 0:
        return kind
    candidates = count("candidate_count")
    if (outcome == "no_context" and kind == "all_rejected" and candidates is not None
            and candidates > 0 and count("admitted_count") == 0):
        return kind
    return None


def cursors(db):
    known = dict(db.execute("SELECT name,offset FROM files LIMIT 100001"))
    if len(known) > 100000:
        raise ValueError("consumer source history limit exceeded")
    return known


def replay(directory, db, max_records=1000, max_files=128):
    private_directory(directory)
    info = directory.stat()
    source_identity = f"{directory}:{info.st_dev}:{info.st_ino}"
    identity = db.execute("SELECT value FROM meta WHERE key='source'").fetchone()
    if identity and identity[0] != source_identity:
        raise ValueError("state belongs to a different journal directory")
    with db:
        db.execute("INSERT OR IGNORE INTO meta VALUES ('source',?)", (source_identity,))
    # Directory enumeration has an explicit ceiling; exceeding it is UNKNOWN
    # coverage, never a successful empty scan. Rotate archives externally only
    # after retention and all consumer dispositions have been satisfied.
    names = []
    with os.scandir(directory) as entries:
        for index, entry in enumerate(entries):
            if index >= 100000:
                raise ValueError("journal directory enumeration limit exceeded")
            if FILE.fullmatch(entry.name):
                names.append(entry.name)
    names.sort()
    known = cursors(db)
    gaps = len(set(known) - set(names))
    last = db.execute("SELECT value FROM meta WHERE key='last_file'").fetchone()
    if last:
        names = [name for name in names if name > last[0]] + [name for name in names if name <= last[0]]
    processed = errors = partial = inspected = 0
    for name in names[:max_files]:
        if processed >= max_records:
            break
        inspected += 1
        with os.fdopen(os.open(directory / name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK), "rb") as source:
            info = os.fstat(source.fileno())
            if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o077:
                raise ValueError("journal must be private and owned")
            previous = db.execute("SELECT device,inode,offset FROM files WHERE name=?", (name,)).fetchone()
            offset = previous[2] if previous else 0
            if previous and (previous[:2] != (info.st_dev, info.st_ino) or info.st_size < offset):
                gaps += 1
                continue
            with db:
                db.execute("INSERT OR IGNORE INTO files VALUES (?,?,?,0)", (name, info.st_dev, info.st_ino))
            source.seek(offset)
            while processed < max_records:
                raw = source.readline(16385)
                if not raw:
                    break
                if not raw.endswith(b"\n"):
                    if len(raw) >= 16385:
                        errors += 1
                    else:
                        partial += 1
                    break
                try:
                    event, kind, reason = validate(raw)
                    prior = db.execute("SELECT digest FROM events WHERE id=?", (event[0],)).fetchone()
                    if prior and prior[0] != event[1]:
                        raise ValueError("conflicting logical identity")
                except (ValueError, TypeError, KeyError, RecursionError):
                    errors += 1
                    break  # Preserve offending record and cursor for repair.
                with db:
                    inserted = db.execute("INSERT OR IGNORE INTO events VALUES (?,?,?,?,?)", event)
                    if inserted.rowcount == 1:
                        db.execute("""INSERT INTO counts VALUES (?,?,?,1)
                            ON CONFLICT(surface,operation,outcome)
                            DO UPDATE SET value=value+1""", event[2:])
                        if kind is not None:
                            db.execute("""INSERT INTO zeros VALUES (?,?,?,1)
                                ON CONFLICT(surface,operation,kind)
                                DO UPDATE SET value=value+1""", (*event[2:4], kind))
                        if reason is not None:
                            db.execute("""INSERT INTO misses VALUES (?,?,?,1)
                                ON CONFLICT(surface,operation,reason)
                                DO UPDATE SET value=value+1""", (*event[2:4], reason))
                    db.execute("UPDATE files SET offset=? WHERE name=?", (source.tell(), name))
                processed += 1
        with db:
            db.execute("INSERT OR REPLACE INTO meta VALUES ('last_file',?)", (name,))
    # Backlog is complete only when every current source has a known cursor.
    tracked = cursors(db)
    backlog = sum(max(0, (directory / name).lstat().st_size - tracked.get(name, 0)) for name in names)
    return {"records_processed": processed, "files_inspected": inspected,
            "errors": errors, "partial_tails": partial, "source_gaps": gaps,
            "backlog_bytes": backlog, "files_uninspected": max(0, len(names) - inspected)}


def metrics(db, health):
    lines = ["# HELP quipu_search_journal_requests_total Unique durable outcomes; do not add to live request counters.",
             "# TYPE quipu_search_journal_requests_total counter"]
    for surface, operation, outcome, count in db.execute(
        "SELECT surface,operation,outcome,value FROM counts ORDER BY 1,2,3"
    ):
        lines.append(f'quipu_search_journal_requests_total{{surface="{surface}",operation="{operation}",outcome="{outcome}"}} {count}')
    for table, family, label in [("zeros", "zero_results", "kind"), ("misses", "miss_reasons", "reason")]:
        lines.append(f"# TYPE quipu_search_journal_{family}_total counter")
        for surface, operation, value, count in db.execute(
            f"SELECT surface,operation,{label},value FROM {table} ORDER BY 1,2,3"
        ):
            lines.append(f'quipu_search_journal_{family}_total{{surface="{surface}",operation="{operation}",{label}="{value}"}} {count}')
    for key, value in health.items():
        lines.extend([f"# TYPE quipu_search_journal_{key} gauge", f"quipu_search_journal_{key} {value}"])
    lines.extend(["# TYPE quipu_search_journal_scan_timestamp_seconds gauge",
                  f"quipu_search_journal_scan_timestamp_seconds {int(time.time())}"])
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--journal-dir", required=True, type=Path)
    parser.add_argument("--state", required=True, type=Path)
    parser.add_argument("--max-records", type=int, default=1000)
    parser.add_argument("--max-files", type=int, default=128)
    args = parser.parse_args()
    if not 1 <= args.max_records <= 100000 or not 1 <= args.max_files <= 10000:
        parser.error("budgets must be positive and bounded (records <=100000; files <=10000)")
    os.umask(0o077)
    try:
        with connect(args.state) as db:
            health = replay(args.journal_dir, db, args.max_records, args.max_files)
            output = metrics(db, health)
        print(output, end="")
        return 2 if health["errors"] or health["source_gaps"] else 0
    except (OSError, ValueError, sqlite3.Error):
        # Paths and raw events can contain private material; do not echo them.
        print("search outcome replay failed; capture coverage is UNKNOWN", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
