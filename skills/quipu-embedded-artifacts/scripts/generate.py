#!/usr/bin/env python3
"""Generate a bounded graph brief, rows, and a native pendant; never publish."""

import argparse
import base64
from datetime import datetime, timezone
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import urllib.request

from local_queries import load_queries
from row_ops import build_row_ops, resolve_ask_bindings

SCHEMA = "https://schema.org/"
SEEDS = "https://seeds.local/ontology/"
AEGIS = "http://aegis.gastown.local/ontology/"
RDF = "http://www.w3.org/1999/02/22-rdf-syntax-ns#"
RDFS = "http://www.w3.org/2000/01/rdf-schema#"
QUECHUA = "https://scbrown.github.io/quechua/ns#"
ASSETS = Path(__file__).resolve().parent.parent / "assets"


def iri(value):
    if not isinstance(value, str) or not re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:", value):
        raise ValueError("Expected an absolute IRI")
    if re.search(r'[\x00-\x20<>"{}|^`\\]', value):
        raise ValueError("Unsafe IRI")
    return "<" + value + ">"


def literal(value):
    return json.dumps(str(value), ensure_ascii=False)


def term(binding):
    if binding["type"] == "uri":
        return iri(binding["value"])
    if binding["type"] != "literal":
        raise ValueError("Blank nodes are outside this bounded brief projection")
    result = literal(binding["value"])
    if binding.get("xml:lang"):
        language = binding["xml:lang"]
        if not re.fullmatch(r"[A-Za-z]+(?:-[A-Za-z0-9]+)*", language):
            raise ValueError("Invalid language tag")
        return result + "@" + language
    if binding.get("datatype"):
        return result + "^^" + iri(binding["datatype"])
    return result


def read_json(path):
    return json.loads(Path(path).read_text())


class Reader:
    def __init__(self, config, work):
        self.config, self.work, self.number = config, work, 0

    def query(self, query, limit):
        self.number += 1
        headers = {"Content-Type": "application/json",
                   "Accept": "application/sparql-results+json",
                   "X-Quipu-Client": "graph-extract"}
        if self.config.get("task"):
            headers["X-Quipu-Task"] = self.config["task"]
        token = os.environ.get("QUIPU_AUTH_TOKEN", "")
        if token:
            headers["Authorization"] = "Bearer " + token
        request = urllib.request.Request(
            self.config["server"].rstrip("/") + "/query", headers=headers,
            data=json.dumps({"query": query}).encode(),
        )
        with urllib.request.urlopen(request, timeout=35) as response:
            raw = response.read(2_000_001)
        if len(raw) > 2_000_000:
            raise ValueError("Bounded response exceeded 2 MB")
        (self.work / f"query-{self.number}.rq").write_text(query)
        (self.work / f"query-{self.number}.json").write_bytes(raw)
        data = json.loads(raw)
        if data.get("truncated"):
            raise ValueError("Server reported truncated results")
        rows = data["results"]["bindings"]
        if len(rows) >= limit:
            raise ValueError("Result hit its sentinel limit; coverage UNKNOWN")
        return rows


def generate(config, out, work, bundle, binary):
    out, work = out.resolve(), work.resolve()
    if out == work or out in work.parents or work in out.parents:
        raise ValueError("Publish output and private work must not overlap")
    if out.exists() or work.exists():
        raise ValueError("Output and work directories must both be new")
    work.mkdir(parents=True, mode=0o700)
    ranks = read_json(config["ranks_file"])["ranks"]
    curated = read_json(config["asks_file"])
    binding_receipt = read_json(config["row_bindings_file"])
    bindings = binding_receipt.get("bindings", binding_receipt)
    bindings = resolve_ask_bindings(bindings, curated)
    # Validate actual target identities/versions before any remote read.
    build_row_ops([{"item": row["item"], "rank": row["rank"], "priority": 0, "owner": ""}
                   for row in ranks], curated["asks"], bindings)
    cards = read_json(config["desk_file"])["ops"]
    cards = [op["data"] for op in cards if op["op"] == "set"]
    extra = config.get("extra_asks", [])
    ids = sorted({r["item"] for r in ranks} | {c["bead"] for c in cards}
                 | {r["item"] for r in extra} | {r["item"] for r in curated["asks"]})
    if not ids or len(ids) > 100 or len({r["rank"] for r in ranks}) != len(ranks):
        raise ValueError("Need 1–100 exact IDs and unique curated ranks")
    if len({r["item"] for r in ranks}) != len(ranks):
        raise ValueError("Duplicate ranked item")
    reader = Reader(config, work)
    values = " ".join(literal(item) for item in ids)
    graph = iri(config["graph"])
    rows = reader.query(
        f"SELECT ?s ?id ?title ?priority ?status ?owner ?description WHERE {{ GRAPH {graph} {{ "
        f"VALUES ?id {{ {values} }} ?s <{SCHEMA}identifier> ?id ; <{SCHEMA}name> ?title ; "
        f"<{SEEDS}priority> ?priority ; <{SEEDS}status> ?status . "
        f"OPTIONAL {{ ?s <{SCHEMA}agent> ?owner }} "
        f"OPTIONAL {{ ?s <{SCHEMA}description> ?description }} }} }} LIMIT 101", 101)
    records = {r["id"]["value"]: r for r in rows}
    if set(records) != set(ids) or len(records) != len(rows):
        raise ValueError("Missing or ambiguous selected item; no partial brief")
    subjects = " ".join(iri(r["s"]["value"]) for r in rows)
    links = reader.query(
        f"SELECT ?s ?p ?o WHERE {{ GRAPH {graph} {{ VALUES ?s {{ {subjects} }} "
        f"VALUES ?p {{ <{SCHEMA}agent> <{QUECHUA}blockedOn> "
        f"<http://purl.org/dc/terms/relation> <{SCHEMA}url> }} ?s ?p ?o }} }} LIMIT 501", 501)
    linked = sorted({r["o"]["value"] for r in links if r["o"]["type"] == "uri"}
                    - {r["s"]["value"] for r in rows})
    if len(linked) > 100:
        raise ValueError("Linked-node bound exceeded")
    linked_facts = []
    if linked:
        linked_values = " ".join(iri(s) for s in linked)
        linked_facts = reader.query(
            f"SELECT ?s ?p ?o WHERE {{ GRAPH {graph} {{ VALUES ?s {{ {linked_values} }} "
            f"VALUES ?p {{ <{RDF}type> <{RDFS}label> <{SCHEMA}name> <{SCHEMA}identifier> "
            f"<{SEEDS}status> <{SEEDS}priority> <{SCHEMA}description> }} ?s ?p ?o }} }} LIMIT 1001", 1001)
    # Fetch only the native outward catalogue's operational fields, not its prose/history.
    catalogue = reader.query(
        f"SELECT ?s ?p ?o WHERE {{ ?s a <{AEGIS}InternalIdentifierPattern> . "
        f"VALUES ?p {{ <{RDF}type> <{AEGIS}regex> <{AEGIS}enforcementTier> <{RDFS}label> }} "
        "?s ?p ?o } LIMIT 101", 101)
    if not catalogue:
        raise ValueError("Outward identifier catalogue unavailable")
    policy = {}
    for fact in catalogue:
        policy.setdefault(fact["s"]["value"], {})[fact["p"]["value"]] = fact["o"]["value"]
    blockers = [re.compile(rule[AEGIS + "regex"]) for rule in policy.values()
                if rule.get(AEGIS + "enforcementTier") == "block"]
    if not blockers:
        raise ValueError("No blocking privacy rules; catalogue coverage UNKNOWN")
    triples = set()
    omissions = []

    def add(s, p, o):
        triples.add(f"{iri(s)} {iri(p)} {o} .\n")

    def projected(s, p, binding):
        # Exclude an optional private description, never rewrite identifiers or
        # silently remove a required title. The unchanged outward gate still runs.
        if (p == SCHEMA + "description" and binding["type"] == "literal"
                and any(rule.search(binding["value"]) for rule in blockers)):
            omissions.append({"entity": s, "predicate": p,
                              "reason": "Description withheld: private identifiers"})
        else:
            add(s, p, term(binding))

    predicates = {"id": SCHEMA + "identifier", "title": SCHEMA + "name",
                  "priority": SEEDS + "priority", "status": SEEDS + "status",
                  "owner": SCHEMA + "agent", "description": SCHEMA + "description"}
    for record in rows:
        subject = record["s"]["value"]
        add(subject, RDF + "type", iri(SCHEMA + "Action"))
        add(subject, RDFS + "label", term(record["title"]))
        for field, predicate in predicates.items():
            if field in record:
                projected(subject, predicate, record[field])
    for fact in links + linked_facts:
        projected(fact["s"]["value"], fact["p"]["value"], fact["o"])
    priorities = []
    for rank in sorted(ranks, key=lambda r: r["rank"]):
        record = records[rank["item"]]
        get = lambda key: record.get(key, {}).get("value", "")
        priority = int(get("priority"))
        priorities.append({"rank": rank["rank"], "item": rank["item"],
                           "title": get("title"), "priority": priority,
                           "owner": get("owner").rsplit("/", 1)[-1],
                           "state": get("status"), "next": ""})
        add(get("s"), SCHEMA + "position", literal(int(rank["rank"]))
            + "^^<http://www.w3.org/2001/XMLSchema#integer>")
    asks = [{"item": card["bead"], "ask": card["ask"], "where": "Decision Desk"}
            for card in cards]
    asks += [{"item": ask["item"], "ask": records[ask["item"]]["title"]["value"],
              "where": ask["where"]} for ask in extra]
    included = {ask["item"] for ask in asks}
    for ask in curated["asks"]:
        if ask["item"] not in included:
            # Curation does not change admission to the separate Decision Desk.
            asks.append({"item": ask["item"], "ask": records[ask["item"]]["title"]["value"],
                         "where": "board"})
    for order, ask in enumerate(asks, 1):
        ask["order"] = order
        subject = "urn:brief:ask:" + str(order)
        if any(rule.search(ask["ask"]) for rule in blockers):
            ask["ask"] = (records[ask["item"]]["title"]["value"]
                          + " (Full ask withheld: private identifiers.)")
            omissions.append({"entity": subject, "predicate": RDFS + "label",
                              "reason": "Private ask replaced by its source title and explicit notice"})
        add(subject, RDF + "type", iri(AEGIS + "Document"))
        add(subject, RDFS + "label", literal(ask["ask"]))
        add(subject, SCHEMA + "category", literal(ask["where"]))
        add(subject, SCHEMA + "about", term(records[ask["item"]]["s"]))
    snapshot = Path(config["snapshot_dir"])
    today = read_json(snapshot / "brief/today.json")
    updates = [read_json(p) for p in sorted((snapshot / "updates").glob("*.json"))]
    for index, document in enumerate([today, *updates]):
        subject = "urn:brief:status:" + str(index)
        add(subject, RDF + "type", iri(AEGIS + "Document"))
        add(subject, RDFS + "label", literal(document["headline"]))
        add(subject, SCHEMA + "description", literal(json.dumps(document, ensure_ascii=False)))
        for record in rows:
            add(subject, SCHEMA + "about", term(record["s"]))
    ask_by_item = {ask["item"]: ask for ask in asks}
    visible_asks = [{**ask_by_item[ask["item"]], "order": ask["order"]}
                    for ask in sorted(curated["asks"], key=lambda ask: ask["order"])]
    ops = build_row_ops(priorities, visible_asks, bindings)
    # Operator operations and source graph identity are not page assets. Keep
    # them beside the raw responses, never in the publish-ready directory.
    (work / "rows.json").write_text(json.dumps({"ops": ops}, indent=2) + "\n")
    (work / "provenance.json").write_text(json.dumps({"graph": config["graph"]}) + "\n")
    metadata = {"generated_at": datetime.now(timezone.utc).isoformat(),
                "items": {k: v["s"]["value"] for k, v in records.items()},
                "selected_items": len(records), "linked_nodes": len(linked),
                "coverage": "Curated items, exact selected fields and bounded one-hop links; no comments.",
                "omissions": omissions, "desk_count": len(cards)}
    metadata_text = json.dumps(metadata, indent=2, ensure_ascii=False) + "\n"
    # Browser metadata travels INSIDE the guarded, manifest-bound pendant.
    # There is no separately published JSON sidecar outside native admission.
    (work / "publication-metadata.json").write_text(metadata_text)
    add("urn:brief:publication-metadata", SCHEMA + "description", literal(metadata_text))
    (work / "data.nt").write_text("".join(sorted(triples)))
    (work / "catalogue.nt").write_text("".join(
        f"{term(r['s'])} {term(r['p'])} {term(r['o'])} .\n" for r in catalogue))
    shapes = ASSETS / "projection-shapes.ttl"
    database = work / "brief.sqlite3"

    def native(*arguments):
        result = subprocess.run([binary, *map(str, arguments), "--db", str(database)],
                                capture_output=True, text=True)
        with (work / "native.log").open("a") as log:
            log.write(result.stdout + result.stderr)
        result.check_returncode()

    native("shapes", "load", "brief-projection", shapes)
    native("knot", work / "data.nt")
    native("knot", work / "catalogue.nt", "--graph", "urn:brief:outward-policy")
    queries = read_json(ASSETS / "queries.json")
    load_queries(binary, database, queries)
    native("share", "--output", work / "pendant", "--shapes", "brief-projection")
    archive = io.BytesIO()
    with tarfile.open(fileobj=archive, mode="w:gz") as tar:
        for file in sorted((work / "pendant").iterdir()):
            info = tar.gettarinfo(str(file), file.name)
            info.uid = info.gid = info.mtime = 0
            info.uname = info.gname = ""
            info.mode = 0o644
            with file.open("rb") as source:
                tar.addfile(info, source)
    encoded = {"encoding": "base64", "sha256": hashlib.sha256(archive.getvalue()).hexdigest(),
               "data": base64.b64encode(archive.getvalue()).decode()}
    # All published data is in the native-checked pendant. A refusal leaves
    # no output directory; owner operations and provenance stay private.
    out.mkdir(parents=True)
    (out / "pendant.json").write_text(json.dumps(encoded) + "\n")
    (out / "pkg").mkdir()
    for name in ("quipu_wasm_explorer.js", "quipu_wasm_explorer_bg.wasm"):
        shutil.copyfile(bundle / name, out / "pkg" / name)
    for name in ("explorer.js", "worker.js", "explorer.css"):
        shutil.copyfile(ASSETS / name, out / name)
    print(json.dumps({"items": len(records), "priorities": len(priorities),
                      "visible_asks": len(visible_asks), "desk_cards": len(cards),
                      "operator_rows": str(work / "rows.json"),
                      "share_id": read_json(work / "pendant/manifest.json")["share_id"]}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for option in ("config", "out", "work", "bundle"):
        parser.add_argument("--" + option, type=Path, required=True)
    parser.add_argument("--quipu", default="quipu")
    args = parser.parse_args()
    generate(read_json(args.config), args.out, args.work, args.bundle, args.quipu)
