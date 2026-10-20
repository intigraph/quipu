#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["rdflib==7.6.0"]
# ///
"""Offline warning-only RDF propagation audit; never loads a shape."""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
from rdflib import BNode, Graph, OWL, RDF, RDFS, SH, URIRef

from rdflib.compare import to_canonical_graph

def git(repo, *args):
    return subprocess.check_output(["git", "-C", str(repo), *args], timeout=30)


def snapshot(repo, ref, prefix="shapes/"):
    """Resolve once; read immutable blobs, never the caller's working files."""
    sha = git(repo, "rev-parse", "--verify", ref + "^{commit}").decode().strip()
    paths = git(repo, "ls-tree", "-r", "--name-only", sha, "--", prefix).decode().splitlines()
    graph = Graph()
    for path in sorted(p for p in paths if p.endswith(".ttl")):
        # Per-file blank-node scopes prevent accidental joins between documents.
        part = Graph().parse(data=git(repo, "show", sha + ":" + path).decode(), format="turtle")
        graph += part
    return sha, graph


def component_digest(graph, seed):
    sub = Graph()
    pending, seen = [seed], set()
    while pending:
        node = pending.pop()
        if node in seen:
            continue
        seen.add(node)
        if len(seen) > 10000:
            raise ValueError("constraint component exceeds audit budget")
        for triple in graph.triples((node, None, None)):
            sub.add(triple)
            if isinstance(triple[2], BNode):
                pending.append(triple[2])
    lines = sorted(" ".join(x.n3() for x in triple) + " ." for triple in to_canonical_graph(sub))
    return hashlib.sha256(("\n".join(lines) + "\n").encode()).hexdigest()


def definitions(graph):
    kinds = {}
    for kind, types in {"class": (RDFS.Class, OWL.Class),
                        "property": (RDF.Property, OWL.ObjectProperty, OWL.DatatypeProperty),
                        "NodeShape": (SH.NodeShape,)}.items():
        for typ in types:
            for subject in graph.subjects(RDF.type, typ):
                if isinstance(subject, URIRef):
                    kinds.setdefault(subject, set()).add(kind)
    shape_roots = set(graph.subjects(RDF.type, SH.NodeShape))
    for predicate in (SH.targetClass, SH.targetNode, SH.targetSubjectsOf, SH.targetObjectsOf):
        # A targeted property shape is not implicitly a node shape.
        shape_roots.update(subject for subject in graph.subjects(predicate, None)
                           if (subject, RDF.type, SH.PropertyShape) not in graph
                           and not any(graph.objects(subject, SH.path)))
    for subject in shape_roots:
        if isinstance(subject, URIRef):
            kinds.setdefault(subject, set()).add("NodeShape")
    # Shapes often declare their governed vocabulary only through targets/paths.
    for predicate, kind in ((SH.targetClass, "class"), (SH.path, "property")):
        for subject in graph.objects(None, predicate):
            if isinstance(subject, URIRef):
                kinds.setdefault(subject, set()).add(kind)
    result = {}
    for subject, kind in kinds.items():
        components = []
        # Canonicalize independent constraint components separately. Unioning
        # hundreds of symmetric anonymous properties makes RDF canonicalization
        # needlessly exponential. Preserve multiplicity in the sorted digest list.
        for seed in [subject, *graph.subjects(SH.targetClass, subject), *graph.subjects(SH.path, subject)]:
            components.append(component_digest(graph, seed))
        result[str(subject)] = {"kinds": sorted(kind), "sha256": hashlib.sha256(
            ("\n".join(sorted(components)) + "\n").encode()).hexdigest()}
    # Anonymous roots have no stable production IRI to correlate across snapshots.
    # Compare their canonical components as a multiset under one report-local key.
    # This key is never written to RDF, and evidence covers the exact collection.
    anonymous = sorted(component_digest(graph, subject) for subject in shape_roots
                       if isinstance(subject, BNode))
    if anonymous:
        result["_:anonymous-NodeShapes"] = {"kinds": ["NodeShape"],
            "sha256": hashlib.sha256(("\n".join(anonymous) + "\n").encode()).hexdigest()}
    return result


def changes(before, after):
    old, new = definitions(before), definitions(after)
    return [{"iri": iri, "change": "added" if iri not in old else "removed" if iri not in new else "modified",
             "before": old.get(iri), "after": new.get(iri)}
            for iri in sorted(old.keys() | new.keys()) if old.get(iri) != new.get(iri)]


def audit(delta, catalogue, terms, cq_root):
    """Evidence is exact-revision scoped; mere term/name presence is insufficient."""
    rows = catalogue.get("entries", [])
    if not isinstance(rows, list) or len({r["iri"] for r in rows}) != len(rows):
        raise ValueError("duplicate or malformed catalogue")
    by_iri = {r["iri"]: r for r in rows}
    result = []
    for change in delta:
        row = by_iri.get(change["iri"], {})
        problems = []
        expected = (change["after"] or change["before"])["sha256"]
        if row.get("definition_sha256") != expected:
            problems.append("missing-or-stale-definition-evidence")
        disposition = row.get("disposition")
        if disposition == "mapped":
            target = row.get("term")
            if not target or not any(terms.triples((URIRef(target), None, None))):
                problems.append("term-not-defined")
            elif any(str(value).lower() == 'true' for value in terms.objects(URIRef(target), OWL.deprecated)):
                problems.append("mapped-term-deprecated")
            elif 'property' in (change['after'] or change['before'])['kinds'] and not any(
                    (URIRef(target), RDF.type, typ) in terms for typ in
                    (RDF.Property, OWL.ObjectProperty, OWL.DatatypeProperty)):
                problems.append("mapped-term-wrong-kind")
            elif 'class' in (change['after'] or change['before'])['kinds'] and not any(
                    (URIRef(target), RDF.type, typ) in terms for typ in (RDFS.Class, OWL.Class)):
                problems.append("mapped-term-wrong-kind")
        elif disposition != "quipu-local" or not row.get("reason"):
            problems.append("missing-term-or-explicit-local-reason")
        cq = row.get("cq")
        waiver = row.get("waiver", {})
        if cq:
            path = (cq_root / cq.get("path", "")).resolve()
            if (not path.is_relative_to(cq_root.resolve()) or not path.is_file()
                    or hashlib.sha256(path.read_bytes()).hexdigest() != cq.get("sha256")):
                problems.append("cq-missing-or-stale")
            else:
                try:
                    document = json.loads(path.read_text())
                    query = document.get('query', document.get('template'))
                    if not isinstance(query, str) or not query.strip():
                        raise ValueError('stored query missing')
                except (ValueError, AttributeError, UnicodeError):
                    problems.append('cq-not-a-stored-query')
        elif not (waiver.get("reason") and waiver.get("receipt") and waiver.get("reviewer")):
            problems.append("missing-cq-or-reviewed-waiver")
        # Placement is explicitly proposed; it grants no load/unload authority.
        result.append({**change, "disposition": disposition, "gaps": problems})
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--terms", type=Path, required=True)
    parser.add_argument("--catalogue", type=Path, required=True)
    parser.add_argument("--cq-root", type=Path, required=True)
    args = parser.parse_args()
    base, old = snapshot(args.repo, args.base)
    head, new = snapshot(args.repo, args.head)
    rows = audit(changes(old, new), json.loads(args.catalogue.read_text()),
                 Graph().parse(args.terms, format="turtle"), args.cq_root)
    report = {"schema": "shape-propagation/v1", "base": base, "head": head,
              "mode": "WARN", "changes": rows, "gaps": sum(bool(r["gaps"]) for r in rows)}
    print(json.dumps(report, indent=2, sort_keys=True))
    # Warning-only until the October backfill and enforcement activation are accepted.
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
