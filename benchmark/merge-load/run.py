#!/usr/bin/env python3
"""Real, local merge transports scored against an independent operation ledger."""
import argparse
import hashlib
import json
import os
import resource
import signal
import sqlite3
from pathlib import Path
import shutil
import subprocess
import tarfile
import time
import sys

import rdflib
from rdflib import Graph, RDF
import model


class CountingGraph(Graph):
    def __init__(self):
        super().__init__()
        self.statements = 0

    def add(self, triple):
        self.statements += 1
        return super().add(triple)


def graph(text):
    parsed = CountingGraph()
    parsed.parse(data=text, format="nt")
    if parsed.statements != len(parsed):
        raise AssertionError("duplicate exported RDF statements")
    return set(parsed)


def equal(actual, expected):
    a, e = graph(actual), graph(model.render(expected))
    if a != e:
        raise AssertionError(f"oracle mismatch: missing={len(e-a)} extra={len(a-e)}; "
                             f"missing_sample={sorted(map(str, e-a))[:3]}")
    return hashlib.sha256("\n".join(sorted(str(t) for t in a)).encode()).hexdigest()


class Run:
    def __init__(self, root, driver, cli, timeout):
        self.root, self.driver, self.cli = root, driver, cli
        self.timeout, self.samples = timeout, []
        self.identities = {}
        (root / "home").mkdir()

    def cmd(self, argv, cwd=None, expected=(0,)):
        i = len(self.samples)
        rss = self.root / f"time-{i}.txt"
        start = time.perf_counter()
        child = subprocess.Popen(["/usr/bin/time", "-f", "%M", "-o", str(rss), *map(str, argv)],
                                 cwd=cwd or self.root, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                 start_new_session=True,
                                 env={**os.environ, "HOME": str(self.root/"home"),
                                      "XDG_CONFIG_HOME": str(self.root/"home/.config"),
                                      "XDG_DATA_HOME": str(self.root/"home/.local/share"),
                                      "XDG_STATE_HOME": str(self.root/"home/.local/state"),
                                      "XDG_CACHE_HOME": str(self.root/"home/.cache"),
                                      "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull})
        try:
            stdout, stderr = child.communicate(timeout=self.timeout)
        except subprocess.TimeoutExpired:
            # Killing only `time` leaves its benchmark running unowned. This
            # process group belongs solely to this invocation, never the crew.
            os.killpg(child.pid, signal.SIGTERM)
            try:
                stdout, stderr = child.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                stdout, stderr = child.communicate()
            (self.root/f"stdout-{i}.log").write_text(stdout)
            (self.root/f"stderr-{i}.log").write_text(stderr)
            raise TimeoutError(f"operation exceeded {self.timeout}s; owned process group stopped") from None
        r = subprocess.CompletedProcess(argv, child.returncode, stdout, stderr)
        self.samples.append({"operation": str(argv[0]).split("/")[-1], "wall_s": time.perf_counter()-start,
                             "peak_rss_kib": int(rss.read_text().splitlines()[-1]), "exit": r.returncode})
        (self.root / f"stdout-{i}.log").write_text(r.stdout)
        (self.root / f"stderr-{i}.log").write_text(r.stderr)
        if r.returncode not in expected:
            raise RuntimeError(f"command failed exit{r.returncode}: {r.stderr[-1200:]}")
        return r.stdout

    def call(self, op, db=None, **kwargs):
        p = self.root / f"request-{len(self.samples)}.json"
        p.write_text(json.dumps({"op": op, "db": str(db), **{k: str(v) if isinstance(v, Path) else v for k, v in kwargs.items()}}))
        result = json.loads(self.cmd([self.driver, p]))
        self.samples[-1]["operation"] = op
        if op == "seed":
            identity = result["store_id"]
            if identity in self.identities.values():
                raise AssertionError("fixture stores unexpectedly share identity")
            self.identities[Path(db).name] = identity
        return result

    def edit(self, db, operations):
        request = model.edit_request(db, operations)
        self.call(**request)

    def export(self, db):
        p = self.root / "actual.nt"
        self.call("export", db, out=p)
        return p.read_text()

    def merged(self, db, incoming, expected_slots):
        size = db.stat().st_size
        before = self.export(db)
        first_sample = len(self.samples)
        result = json.loads(self.cmd([self.cli, "merge", incoming, "--db", db], expected=(0, 2)))
        self.samples[-1]["operation"] = "CLI merge"
        slots = {(r["subject"].strip("<>"), r["predicate"]) for r in result["conflicts"]}
        if slots != expected_slots:
            raise AssertionError(f"conflicts: actual{len(slots)} expected{len(expected_slots)}")
        decisions = self.root / f"decisions-{first_sample}.json"
        self.cmd([self.cli, "merge", incoming, "--emit-decisions", decisions, "--db", db])
        self.samples[-1]["operation"] = "CLI --emit-decisions"
        file = json.loads(decisions.read_text())
        if len(file["rows"]) != len(expected_slots):
            raise AssertionError("decision rows do not cover the seeded slots")
        labels = file.get("labels", {})
        if expected_slots and not all(s in labels and model.STATUS in labels for s, _ in expected_slots):
            raise AssertionError("decision labels missing")
        if expected_slots:
            if graph(self.export(db)) != graph(before):
                raise AssertionError("unresolved native merge wrote facts")
            for row in file["rows"]:
                row["decision"] = {"choose": "ours"}
            decided = decisions.with_suffix(".applied.json")
            decided.write_text(json.dumps(file, indent=2))
            self.cmd([self.cli, "merge", incoming, "--decisions", decided, "--reviewer", "load-fixture", "--db", db])
            self.samples[-1]["operation"] = "CLI apply decisions"
        after = self.export(db)
        # A repeated conflicting input may require a fresh review, but must not
        # mutate facts. A clean replay must likewise be fact-idempotent.
        replay = json.loads(self.cmd([self.cli, "merge", incoming, "--db", db], expected=(0, 2)))
        self.samples[-1]["operation"] = "CLI replay"
        if graph(after) != graph(self.export(db)):
            raise AssertionError("replay changed facts")
        return {"conflicts": len(slots), "decision_rows": len(file["rows"]),
                "decision_bytes": decisions.stat().st_size, "decision_labels": len(labels),
                "store_growth_bytes": db.stat().st_size-size, "replay_outcome": replay["outcome"],
                "merge_samples": self.samples[first_sample:], "facts": after}


def prepare(run, repo, n, seed, replicas):
    base, edits, conflicts = model.scenario(n, replicas, seed)
    catalogue = Graph().parse(repo / "tests/fixtures/share-catalogue.ttl")
    base.update((str(s), str(p), o.n3()) for s, p, o in catalogue)
    base.add((model.STATUS, model.LABEL, model.literal("status")))
    facts, shapes = run.root / "base.nt", run.root / "shapes.ttl"
    facts.write_text(model.render(base))
    shapes.write_text(model.SHAPES + "\n".join(
        f"[] a <http://www.w3.org/ns/shacl#NodeShape>; <http://www.w3.org/ns/shacl#targetClass> <{t}> ."
        for t in set(catalogue.objects(None, RDF.type))))
    db = run.root / "base.db"
    run.call("seed", db, facts=facts, shapes=shapes)
    pack = run.root / "base"
    manifest = run.call("share", db, out=pack)
    equal((pack / "export.nt").read_text(), base)
    return base, edits, conflicts, db, pack, manifest


def participant(run, base_db, edits, name, same_producer=False):
    db = run.root / f"{name}.db"
    if same_producer:
        # Preserve the pendant producer, but explicitly give this local branch
        # a fresh placement/epoch rather than silently writing a raw DB copy.
        def identity(path):
            with sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True) as conn:
                return (conn.execute("SELECT store_id FROM store_identity").fetchone()[0],
                        conn.execute("SELECT epoch FROM log_epoch").fetchone()[0])
        source = identity(base_db)
        run.cmd([run.cli, "store", "restore", base_db, "--db", db])
        restored = identity(db)
        if restored[0] != source[0] or restored[1] == source[1] or identity(base_db) != source:
            raise AssertionError("explicit Git fixture restore must preserve producer and mint a new epoch without changing source")
    else:
        run.call("seed", db, facts=run.root/"base.nt", shapes=run.root/"shapes.ttl")
    run.edit(db, edits)
    return db


def native(run, fixture, replicas=2):
    base, edits, conflicts, db, parent, manifest = fixture
    local = participant(run, db, edits[0], "ours")
    result, details = None, []
    for i in range(1, replicas):
        remote = participant(run, db, edits[i], f"remote-{i}")
        incoming = run.root / f"incoming-{i}"
        run.call("share", remote, out=incoming, parent_id=manifest["share_id"])
        result = run.merged(local, incoming, conflicts)
        equal(result.pop("facts"), model.oracle(base, edits[:i+1]))
        details.append(result)
    return {"participants": replicas, "merges": details, "oracle": "PASS"}


def delta(run, fixture, chunk):
    base, edits, conflicts, db, parent, manifest = fixture
    remote = run.root / "chain.db"
    run.call("seed", remote, facts=run.root/"base.nt", shapes=run.root/"shapes.ttl")
    chain_parent = parent
    chain_local = participant(run, db, edits[0], "chain-ours")
    chain_results = []
    chunks = 0
    for offset in range(0, len(edits[1]), chunk):
        run.edit(remote, edits[1][offset:offset+chunk])
        d, material = run.root / f"delta-{chunks}", run.root / f"material-{chunks}"
        run.call("delta", remote, parent=chain_parent, out=d)
        run.call("materialize", parent=chain_parent, delta=d, out=material)
        equal((material / "export.nt").read_text(), model.apply(base, edits[1][:offset+chunk]))
        seeded = {(s, p) for op in edits[1][offset:offset+chunk] if op["kind"] == "conflict" for s, p, _ in op["remove"]}
        merged = run.merged(chain_local, material, seeded)
        equal(merged.pop("facts"), model.oracle(base, [edits[0], edits[1][:offset+chunk]]))
        chain_results.append(merged)
        chain_parent, chunks = material, chunks+1
    large = run.root / "large-delta"
    run.call("delta", remote, parent=parent, out=large)
    large_pack = run.root / "large-material"
    run.call("materialize", parent=parent, delta=large, out=large_pack)
    equal((chain_parent / "export.nt").read_text(), model.apply(base, edits[1]))
    if graph((chain_parent / "export.nt").read_text()) != graph((large_pack / "export.nt").read_text()):
        raise AssertionError("delta chain differs from one large delta")
    local = participant(run, db, edits[0], "ours")
    result = run.merged(local, large_pack, conflicts)
    equal(result.pop("facts"), model.oracle(base, edits[:2]))
    equal(run.export(chain_local), model.oracle(base, edits[:2]))
    return {"chunks": chunks, "chunk_edits": chunk, "chain_large_parity": True, "chain_merges": chain_results, **result, "oracle": "PASS"}


def git_path(run, fixture):
    base, edits, conflicts, db, parent, manifest = fixture
    repo = run.root / "git"
    repo.mkdir()
    def git(*args):
        return run.cmd(["git", "-c", "user.name=Merge load", "-c", "user.email=fixture@example.test", "-c", "commit.gpgsign=false", *args], cwd=repo).strip()
    git("init", "-b", "base")
    git("config", "user.name", "Merge load")
    git("config", "user.email", "fixture@example.test")
    git("config", "commit.gpgsign", "false")
    shutil.copytree(parent, repo / "pendant")
    (repo / ".gitattributes").write_text("pendant/export.nt merge=quipu\npendant/shapes.ttl merge=quipu\npendant/manifest.json merge=quipu\n")
    git("add", "."); git("commit", "-m", "base")
    git_base_bytes = sum(p.stat().st_size for p in (repo/".git").rglob("*") if p.is_file())
    graph_base_bytes = (repo/"pendant/export.nt").stat().st_size
    base_ref = git("rev-parse", "HEAD")
    refs = []
    for i, side in enumerate(("ours", "theirs")):
        git("checkout", "-b", side, base_ref)
        changed = participant(run, db, edits[i], side, same_producer=True)
        pack = run.root / side
        written = run.call("share", changed, out=pack, parent_id=manifest["share_id"])
        if written["store_id"] != manifest["store_id"]:
            raise AssertionError("Git branches must retain pendant producer identity")
        shutil.copytree(pack, repo / "pendant", dirs_exist_ok=True)
        git("add", "."); git("commit", "-m", side)
        refs.append(git("rev-parse", "HEAD"))
    git("checkout", "ours")
    held_before = (repo/"pendant/export.nt").read_bytes()
    foreign = participant(run, db, edits[1], "foreign")
    foreign_pack = run.root/"foreign-pack"
    run.call("share", foreign, out=foreign_pack, parent_id=manifest["share_id"])
    git("checkout", "-b", "foreign", base_ref)
    shutil.copytree(foreign_pack, repo/"pendant", dirs_exist_ok=True)
    git("add", "."); git("commit", "-m", "foreign producer")
    foreign_ref = git("rev-parse", "HEAD")
    git("checkout", "ours")
    run.cmd([run.cli, "git-merge", foreign_ref], cwd=repo, expected=(1,))
    if "incompatible pendant store/scope/destination/layout" not in (run.root/f"stderr-{len(run.samples)-1}.log").read_text():
        raise AssertionError("foreign Git producer refused for an unverified reason")
    if git("status", "--porcelain") or (repo/"pendant/export.nt").read_bytes() != held_before:
        raise AssertionError("foreign producer refusal changed Git state")
    run.cmd([run.cli, "git-merge", refs[1]], cwd=repo, expected=(2,))
    decisions = json.loads((repo / "pendant/decisions.json").read_text())
    actual = {(x["subject"].strip("<>"), x["predicate"]) for x in decisions["conflicts"]}
    if actual != conflicts or decisions["aliases"]:
        raise AssertionError("Git conflict/alias set differs from fixture")
    equal((repo / "pendant/export.nt").read_text(), model.oracle(base, edits[:2], hold=True))
    git("add", "."); git("commit", "-m", "held conflicts")
    # Resolve on a fresh re-run only when decisions can be batched. Thousands
    # of repeated whole-pack CLI resolutions would measure the review UI loop.
    run.cmd([run.cli, "pendant-check", base_ref, *refs, "HEAD"], cwd=repo, expected=(1,))
    if "unresolved conflicts/alias proposals" not in (run.root/f"stderr-{len(run.samples)-1}.log").read_text():
        raise AssertionError("Git gate refusal was not about unresolved decisions")
    held = (repo / "pendant/export.nt").read_bytes()
    run.cmd([run.cli, "git-merge", refs[1]], cwd=repo, expected=(0, 2))
    if held != (repo / "pendant/export.nt").read_bytes():
        raise AssertionError("repeated Git merge changed held facts")
    return {"oracle": "PASS", "conflicts": len(actual), "decision_bytes": (repo/"pendant/decisions.json").stat().st_size,
            "producer_identity": "same-producer branches; distinct foreign producer refused without writes",
            "git_object_growth_bytes": sum(p.stat().st_size for p in (repo/".git").rglob("*") if p.is_file())-git_base_bytes,
            "graph_growth_bytes": (repo/"pendant/export.nt").stat().st_size-graph_base_bytes,
            "resolution": "unresolved base-held artifact verified; mandatory check refuses it",
            "idempotence": "PASS: replay preserves held snapshot; unresolved review still required"}


def wasm(run, fixture, module, runner):
    base, edits, conflicts, db, parent, manifest = fixture
    if not module:
        return {"status": "UNSUPPORTED", "reason": "actual Explorer WASM module not supplied"}
    archive = run.root / "base.pendant.tar.gz"
    with tarfile.open(archive, "w:gz") as tar:
        for p in parent.iterdir():
            tar.add(p, arcname=p.name)
    request = model.edit_request(db, edits[1])
    # Knot handles additions; retract exact slots through Explorer.retract.
    (run.root / "wasm-edits.json").write_text(json.dumps(request))
    out = run.root / "wasm-delta"
    run.cmd(["node", runner, module, archive, run.root / "wasm-edits.json", out])
    material = run.root / "wasm-material"
    run.call("materialize", parent=parent, delta=out, out=material)
    equal((material / "export.nt").read_text(), model.apply(base, edits[1]))
    local = participant(run, db, edits[0], "ours")
    result = run.merged(local, material, conflicts)
    equal(result.pop("facts"), model.oracle(base, edits[:2]))
    return {"oracle": "PASS", "wasm": json.loads((out / "load-metrics.json").read_text()), **result}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--driver", type=Path, required=True)
    parser.add_argument("--quipu", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--sizes", default="100,1000,5000")
    parser.add_argument("--paths", default="native,delta,wasm,git,replicas")
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--chunk", type=int, default=100)
    parser.add_argument("--timeout", type=int, default=600)
    parser.add_argument("--wasm-module", type=Path)
    args = parser.parse_args()
    args.out = args.out.resolve()
    paths = args.paths.split(",")
    if not set(paths) <= {"native", "delta", "wasm", "git", "replicas"} or len(set(paths)) != len(paths):
        parser.error("choose distinct supported paths")
    if args.timeout < 1:
        parser.error("timeout must be positive")
    repo = Path(__file__).resolve().parents[2]
    args.out.mkdir(parents=True, exist_ok=False)
    artifacts = [args.driver, args.quipu, *Path(__file__).parent.glob("*.py"), Path(__file__).parent/"wasm.cjs"]
    if args.wasm_module:
        artifacts.extend([args.wasm_module, args.wasm_module.with_name("quipu_wasm_explorer_bg.wasm")])
    report = {"schema": "merge-load/v1", "seed": args.seed, "source": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip(),
              "artifacts": {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in artifacts},
              "oracle_runtime": {"python": sys.version, "rdflib": rdflib.__version__},
              "sizes": args.sizes, "paths": args.paths, "rows": []}
    for n in map(int, args.sizes.split(",")):
        if n < 10 or args.chunk < 1:
            raise ValueError("size>=10 and chunk>=1 required")
        for path in args.paths.split(","):
            root = args.out / f"{path}-{n}"
            root.mkdir()
            run = Run(root, args.driver.resolve(), args.quipu.resolve(), args.timeout)
            start = time.perf_counter()
            row = {"path": path, "edits_per_replica": n, "seed": args.seed}
            try:
                fixture = prepare(run, repo, n, args.seed, 5 if path == "replicas" else 2)
                result = {"native": lambda: native(run, fixture), "replicas": lambda: native(run, fixture, 5),
                          "delta": lambda: delta(run, fixture, args.chunk), "git": lambda: git_path(run, fixture),
                          "wasm": lambda: wasm(run, fixture, args.wasm_module, repo/"benchmark/merge-load/wasm.cjs")}[path]()
                row.update({"status": "PASS", **result})
            except Exception as e:
                row.update(status="FAIL", reason=str(e))
            row.update(wall_s=time.perf_counter()-start, max_child_peak_rss_kib=max((s["peak_rss_kib"] for s in run.samples), default=None),
                       oracle_process_peak_rss_kib_cumulative=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
                       store_identities=run.identities,
                       samples=run.samples, scratch_bytes=sum(p.stat().st_size for p in root.rglob("*") if p.is_file()))
            report["rows"].append(row)
            (args.out / "results.json").write_text(json.dumps(report, indent=2)+"\n")
            print(json.dumps({k:v for k,v in row.items() if k not in ("samples", "merges", "merge_samples", "chain_merges")}), flush=True)
    raise SystemExit(0 if all(r["status"] == "PASS" for r in report["rows"]) else 1)


if __name__ == "__main__":
    main()
