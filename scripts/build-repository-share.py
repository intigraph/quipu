"""Build one outward stack share using the existing Quipu/Bobbin boundary.

Source and tooling are separate fresh, tag-verified checkouts. Index configuration,
live policy, databases and receiver probes stay in the private working directory.
Only the verified share directory leaves this function.
"""

import hashlib
import io
import json
import os
import re
import shutil
import sqlite3
import subprocess
import sys
import tarfile
from pathlib import Path


# Used by the producer and its native five-document integration probe.
INDEX_CONFIG = """quipu_push_chunks = true

[index]
include = [
  "**/*.rs", "**/*.md", "**/*.rst", "**/*.toml", "**/*.yml", "**/*.yaml", "**/*.py",
  "**/*.sh", "**/*.js", "**/*.mjs", "**/*.ts", "**/*.tsx", "**/*.json", "**/*.ttl", "**/*.go",
]
exclude = [
  "**/target/**", "**/.git/**", "**/.bobbin/**", "**/.agents/**", "**/.claude/**", "**/.codex/**",
  "**/.mcp.json", "**/CLAUDE.local.md", "**/node_modules/**", "**/book/book/**", "**/vendor/**",
]
use_gitignore = true
entities = false

[embedding]
model = "all-MiniLM-L6-v2"
batch_size = 64
"""


def agent_references(source, paths):
    """Require tracked, nonempty agent references before any publication work."""
    references = (
        "docs/book/src/reference/cli-commands.md",
        "docs/book/src/reference/mcp-tool-index.md",
        "docs/book/src/getting-started/agent-primer.md",
        "src/primer.md",
    )
    if any(
        p not in paths or not (source / p).is_file() or not (source / p).stat().st_size
        for p in references
    ):
        raise ValueError("required agent reference missing from release")
    return references


def isolated_environment(private):
    """Native workers must not inherit a host endpoint or credential catalogue."""
    private = Path(private)
    env = dict(os.environ)
    for key in list(env):
        if key in {
            "BOBBIN_SERVER",
            "QUIPU_SERVER",
            "QUIPU_DB",
            "QUIPU_AUTH_TOKEN",
            "QUIPU_AUTH_TOKEN_FILE",
            "GH_TOKEN",
            "GITHUB_TOKEN",
        } or key.startswith("BOBBIN_QUIPU_"):
            env.pop(key)
    for key, name in (
        ("HOME", "native-home"),
        ("XDG_CONFIG_HOME", "native-config"),
        ("XDG_DATA_HOME", "native-data"),
        ("XDG_CACHE_HOME", "native-cache"),
    ):
        directory = private / name
        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        env[key] = str(directory)
    return env


def command(args, *, env=None, cwd=None):
    if env is None and cwd is not None:
        env = isolated_environment(cwd)
    return subprocess.run(
        [str(v) for v in args],
        check=True,
        capture_output=True,
        text=True,
        env=env,
        cwd=cwd,
    ).stdout


def local_mcp(binary, database, calls):
    """Short-lived native process over a private DB; no HTTP or live graph."""
    import selectors

    requests = [
        {
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "repository-pendant-build", "version": "1"},
            },
        },
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
    ]
    requests.extend(
        {
            "jsonrpc": "2.0",
            "id": number,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments},
        }
        for number, (name, arguments) in enumerate(calls, 1)
    )
    values = []
    with subprocess.Popen(
        [str(binary), "mcp", "--db", str(database)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        cwd=Path(database).parent,
        env=isolated_environment(Path(database).parent),
    ) as process:
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ)
        try:
            for request in requests:
                process.stdin.write(json.dumps(request) + "\n")
                process.stdin.flush()
                if "id" not in request:
                    continue
                if not selector.select(timeout=30):
                    raise ValueError("native MCP response timeout")
                response = json.loads(process.stdout.readline())
                if (
                    response.get("id") != request["id"]
                    or "result" not in response
                    or "error" in response
                    or response["result"].get("isError", False)
                ):
                    raise ValueError(
                        "native MCP refused operation: " + json.dumps(response)
                    )
                if request["id"]:
                    content = response["result"].get("content", [])
                    if len(content) != 1 or content[0].get("type") != "text":
                        raise ValueError("unexpected native MCP result envelope")
                    values.append(json.loads(content[0]["text"]))
        finally:
            selector.close()
            process.stdin.close()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.terminate()
                process.wait(timeout=10)
        if process.returncode:
            raise ValueError("native MCP process failed")
    return values


def register_queries(binary, database, definitions):
    if not isinstance(definitions, list) or not definitions:
        raise ValueError("explicit repository stored query definitions required")
    for definition in definitions:
        if (
            not isinstance(definition, dict)
            or definition.get("action", "load") != "load"
        ):
            raise ValueError("stored query must be an explicit load definition")
    results = local_mcp(
        binary,
        database,
        [("quipu_queries", {**d, "action": "load"}) for d in definitions],
    )
    if [result.get("loaded") for result in results] != [d["name"] for d in definitions]:
        raise ValueError("native registry did not return the exact loaded query names")


def indexed_labels(binary, database, subjects, *, file_paths=False):
    if any(
        not re.fullmatch(r'https?://[^\s<>"{}|\\^`]+', subject) for subject in subjects
    ):
        raise ValueError("invalid indexed subject IRI")
    predicate = (
        "http://aegis.gastown.local/ontology/filePath" if file_paths
        else "http://www.w3.org/2000/01/rdf-schema#label"
    )
    # The receiver's typed Document reader must find these pointers too.
    document = (
        " ?s <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> "
        "<http://aegis.gastown.local/ontology/Document> ." if file_paths else ""
    )
    query = (
        "SELECT ?s ?label WHERE { VALUES ?s { "
        + " ".join("<" + s + ">" for s in subjects)
        + "} ?s <" + predicate + "> ?label ." + document + " }"
    )
    (result,) = local_mcp(
        binary, database, [("quipu_query", {"query": query, "verbose": True})]
    )
    if result.get("truncated") is not False:
        raise ValueError("indexed pointer lookup was incomplete")
    labels = {}
    for row in result["rows"]:
        subject, label = row["s"], row["label"]
        if isinstance(subject, dict):
            subject = subject["value"]
        if isinstance(label, dict):
            label = label["value"]
        if subject in labels and labels[subject] != label:
            raise ValueError("ambiguous indexed pointer label")
        labels[subject] = label
    return labels


# The receiver holds the released pendant, whose documents carry their public
# w3id names (aegis-ro425e.18), not the producer's internal names.
PUBLIC_DOCUMENT_BASE = "https://w3id.org/intigraph/quipu/id/doc/"


def verify_agent_references(binary, database, paths):
    """Require real imported document pointers, never mint substitutes."""
    from urllib.parse import quote

    subjects = [PUBLIC_DOCUMENT_BASE + quote(p, safe="") for p in paths]
    expected = dict(zip(subjects, paths))
    if (
        set(indexed_labels(binary, database, subjects)) != set(subjects)
        or indexed_labels(binary, database, subjects, file_paths=True) != expected
    ):
        raise ValueError("repository agent references missing after import")


def materialize_release(source, revision, destination):
    """Index only exact tracked release bytes in a new private source tree."""
    destination = Path(destination)
    destination.mkdir(mode=0o700)
    archive = subprocess.run(
        ["git", "-C", str(source), "archive", "--format=tar", revision],
        check=True,
        capture_output=True,
    ).stdout
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:") as bundle:
        for member in bundle.getmembers():
            relative = Path(member.name)
            if (
                relative.is_absolute()
                or ".." in relative.parts
                or not (member.isfile() or member.isdir())
            ):
                raise ValueError("unsupported tracked release member")
            target = destination / relative
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with (
                    bundle.extractfile(member) as original,
                    target.open("xb") as copied,
                ):
                    shutil.copyfileobj(original, copied)
    return destination


def alignment_carrier(source):
    """An explicit empty reviewed carrier differs from missing or corrupt data.

    Accepted nonempty mapping transport awaits an actual curated artifact. Do
    not interpret a dependency, lexical match, or candidate as an identity.
    """
    source = Path(source)
    data = (source / "docs/knowledge/alignments.ttl").read_bytes()
    status = json.loads((source / "docs/knowledge/alignment-status.json").read_text())
    if (
        status.get("status") != "no-approved-mappings"
        or type(status.get("approved_record_count")) is not int
        or status["approved_record_count"] != 0
        or status.get("carrier_sha256") != hashlib.sha256(data).hexdigest()
    ):
        raise ValueError("unreviewed or corrupt alignment carrier")
    # Only declarations/comments are permitted in the explicit-empty profile.
    # The native data validator below still parses the complete Turtle input.
    for line in data.decode().splitlines():
        if (
            line.strip()
            and not line.lstrip().startswith("#")
            and line.strip()
            not in {
                "@prefix sssom: <https://w3id.org/sssom/> .",
                "@prefix owl: <http://www.w3.org/2002/07/owl#> .",
            }
        ):
            raise ValueError("empty alignment profile contains mapping data")
    return data.decode()


def native_alignments(binary, private):
    """Engines that advertise --alignments carry the reviewed physical file.

    Older engines keep the inline empty carrier and never emit an undeclared
    archive member that an older receiver would refuse.
    """
    usage = subprocess.run(
        [str(binary), "share", "--help"],
        capture_output=True,
        text=True,
        env=isolated_environment(private),
        cwd=private,
    )
    return "--alignments <file>" in usage.stdout + usage.stderr


def verify_carried_alignments(stage, alignments):
    data = Path(alignments).read_bytes()
    carried = json.loads((Path(stage) / "manifest.json").read_text())
    copied = Path(stage) / "alignments.ttl"
    if (
        carried.get("alignments_hash") != "sha256:" + hashlib.sha256(data).hexdigest()
        or carried.get("files", {}).get("alignments") != "alignments.ttl"
        or not copied.is_file()
        or copied.read_bytes() != data
    ):
        raise ValueError("native share did not carry the reviewed alignments")


FORMER_NAME_LEGACY = "http://aegis.gastown.local/ontology/"
FORMER_IDEA = "https://quipu.dev/knowledge/"
PUBLIC_ENTITY = "https://w3id.org/intigraph/"
STACK_DIRECTIVE = "repo-embedded-knowledge-databases-directive-2026-09-03"
SAME_AS = "http://www.w3.org/2002/07/owl#sameAs"
RDF_TYPE = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type"
# The repository segment, exactly as the release query matches it.
OWNER = r"[A-Za-z0-9-]+"

# W3C RDF 1.1 N-Triples terms. Blank node labels are held to ASCII PN_CHARS:
# a label outside that refuses rather than being guessed at.
_UCHAR = r"\\u[0-9A-Fa-f]{4}|\\U[0-9A-Fa-f]{8}"
_IRIREF = re.compile(rf'<((?:[^\x00-\x20<>"{{}}|^`\\]|{_UCHAR})*)>')
_BNODE = re.compile(r"_:([A-Za-z0-9_](?:[A-Za-z0-9_.\-]*[A-Za-z0-9_\-])?)")
_LITERAL = re.compile(
    rf'"((?:[^"\\\n\r]|\\[tbnrf"\'\\]|{_UCHAR})*)"'
    rf'(?:@[a-zA-Z]+(?:-[a-zA-Z0-9]+)*|\^\^<(?:[^\x00-\x20<>"{{}}|^`\\]|{_UCHAR})*>)?'
)


def _unescape_iri(raw):
    return re.sub(_UCHAR, lambda m: chr(int(m.group(0)[2:], 16)), raw)


def ntriples(text):
    """Triples of strict N-Triples ``text`` as ``(kind, value)`` terms.

    Any whitespace the grammar allows (spaces, tabs, none between terms) and
    any UCHAR escape in an IRI are read the way an RDF importer reads them, so
    a check over these terms sees exactly what a receiver loads. Anything else
    refuses.
    """
    for number, line in enumerate(re.split(r"\r\n|\n|\r", text), 1):
        position, terms = 0, []
        while True:
            while position < len(line) and line[position] in " \t":
                position += 1
            if position == len(line) or line[position] == "#":
                break
            if len(terms) == 3:
                if line[position] != ".":
                    raise ValueError(f"export.nt line {number}: expected '.'")
                terms.append(".")
                position += 1
                continue
            if len(terms) == 4:
                raise ValueError(f"export.nt line {number}: text after '.'")
            for kind, pattern in (("iri", _IRIREF), ("bnode", _BNODE), ("literal", _LITERAL)):
                match = pattern.match(line, position)
                if match:
                    value = _unescape_iri(match.group(1)) if kind == "iri" else match.group(0)
                    terms.append((kind, value))
                    position = match.end()
                    break
            else:
                raise ValueError(f"export.nt line {number}: not an N-Triples term")
        if not terms:
            continue
        if len(terms) != 4:
            raise ValueError(f"export.nt line {number}: not a triple")
        subject, predicate, value, _ = terms
        if subject[0] == "literal" or predicate[0] != "iri":
            raise ValueError(f"export.nt line {number}: not a triple")
        yield subject, predicate, value


def public_name(name):
    """The public name the release query gives legacy ``name``, or None."""
    match = re.fullmatch(rf"{re.escape(FORMER_NAME_LEGACY)}(code|doc|chunk)/({OWNER})/(.+)", name)
    if match:
        kind, owner, tail = match.groups()
        return f"{PUBLIC_ENTITY}{owner}/id/{kind}/{tail}"
    match = re.fullmatch(rf"{re.escape(FORMER_NAME_LEGACY)}repo_({OWNER})", name)
    if match:
        return f"{PUBLIC_ENTITY}{match.group(1)}/id/repo"
    if name == FORMER_NAME_LEGACY + STACK_DIRECTIVE:
        return f"{PUBLIC_ENTITY}stack/id/{STACK_DIRECTIVE}"
    tail = name[len(FORMER_IDEA):] if name.startswith(FORMER_IDEA) else ""
    if tail and not tail.startswith("publication-policy/"):
        return f"{PUBLIC_ENTITY}quipu/id/idea/{tail}"
    return None


def former_name(public):
    """The exact name the release query renamed ``public`` from, or None."""
    match = re.fullmatch(rf"{re.escape(PUBLIC_ENTITY)}({OWNER})/id/(.+)", public)
    if not match:
        return None
    owner, rest = match.groups()
    kind, _, tail = rest.partition("/")
    if kind in ("code", "doc", "chunk") and tail:
        former = f"{FORMER_NAME_LEGACY}{kind}/{owner}/{tail}"
    elif rest == "repo":
        former = f"{FORMER_NAME_LEGACY}repo_{owner}"
    elif owner == "quipu" and kind == "idea" and tail:
        former = FORMER_IDEA + tail
    elif owner == "stack" and rest == STACK_DIRECTIVE:
        former = FORMER_NAME_LEGACY + rest
    else:
        return None
    # Only a name the query would actually produce has a former name.
    return former if public_name(former) == public else None


def is_legacy_entity(name):
    """A legacy entity name the release query renames (or should have)."""
    return public_name(name) is not None or bool(
        re.match(rf"{re.escape(FORMER_NAME_LEGACY)}((code|doc|chunk)/|repo_)", name))


def verify_former_names(export):
    """Admit owl:sameAs only from a public name to its exact former name.

    Every public entity subject carries exactly one such link; a legacy entity
    name appears nowhere except as that link's object (rdf:type objects are
    classes, never entities); and a name under the public entity prefix that
    the release query cannot produce refuses. Returns the number of links.
    """
    linked, subjects = set(), set()
    for subject, predicate, value in ntriples(Path(export).read_text()):
        same = predicate == ("iri", SAME_AS)
        if same:
            if subject[0] != "iri" or value[0] != "iri":
                raise ValueError("owl:sameAs between non-IRIs")
            expected = former_name(subject[1])
            if expected is None or value[1] != expected:
                raise ValueError(f"owl:sameAs is not a former-name link: <{subject[1]}> <{value[1]}>")
            if subject[1] in linked:
                raise ValueError(f"more than one former name: <{subject[1]}>")
            linked.add(subject[1])
        positions = [subject] + ([] if same or predicate == ("iri", RDF_TYPE) else [value])
        for kind, name in positions:
            if kind != "iri":
                continue
            if is_legacy_entity(name):
                raise ValueError(f"legacy entity name published unrenamed: <{name}>")
            if re.match(rf"{re.escape(PUBLIC_ENTITY)}[^/]+/id/", name) and former_name(name) is None:
                raise ValueError(f"not a public entity name: <{name}>")
        if subject[0] == "iri" and former_name(subject[1]) is not None:
            subjects.add(subject[1])
    if subjects - linked:
        raise ValueError(f"public entity without its former-name link: <{min(subjects - linked)}>")
    return len(linked)


def build(revision, source, tooling, binary, bobbin, output):
    repository = "scbrown/quipu"
    source, tooling, output = map(Path, (source, tooling, output))
    if output.exists():
        raise ValueError("repository share output already exists")
    if not re.fullmatch(r"scbrown/[a-z][a-z0-9-]*", repository):
        raise ValueError("invalid repository identity")
    slug = repository.split("/")[1]
    if (
        not re.fullmatch(r"[0-9a-f]{40}", revision)
        or command(["git", "-C", source, "rev-parse", "HEAD"]).strip() != revision
        or command(["git", "-C", source, "status", "--porcelain"]).strip()
    ):
        raise ValueError("repository source is not the exact clean release")
    paths = command(["git", "-C", source, "ls-files", "-z"]).split("\0")
    control = next((p for p in ("README.md", "README.rst") if p in paths), None)
    if control is None:
        raise ValueError("release lacks a supported public corpus positive control")
    references = agent_references(source, paths)
    private = output.parent / "build-private"
    private.mkdir(mode=0o700)

    def run(args, *, env=None):
        return command(args, env=env, cwd=private)

    policy = private / "policy.ttl"
    run(
        ["python3", tooling / "scripts/private-share-policy.py", policy],
        env=dict(os.environ),
    )
    corpus = materialize_release(source, revision, private / "source")
    (corpus / ".bobbin").mkdir(exist_ok=True)
    index_database = private / "indexed-graph.db"
    # Bobbin resolves embedded graph state at --source, not at the vector
    # index root. Explicit private configuration avoids global-store fallback.
    (corpus / ".bobbin/config.toml").write_text(
        "[quipu]\nstore_path = " + json.dumps(str(index_database)) + "\n"
    )
    index = private / "index"
    (index / ".bobbin").mkdir(parents=True)
    (index / ".bobbin/config.toml").write_text(INDEX_CONFIG)
    env = isolated_environment(private)
    run(
        [
            bobbin,
            "index",
            index,
            "--source",
            corpus,
            "--repo",
            slug,
            "--force",
            "--skip-calibrate",
            "--json",
        ],
        env=env,
    )
    database = private / "producer.db"
    if not index_database.is_file():
        raise ValueError("indexer did not produce the repository graph")
    with sqlite3.connect(
        "file:" + str(index_database) + "?mode=ro", uri=True
    ) as original, sqlite3.connect(database) as target:
        original.backup(target)
    shapes = sorted((tooling / "shapes").glob("*.ttl"))
    if not shapes:
        raise ValueError("verified engine source has no receiver shapes")
    for shape in shapes:
        run([binary, "shapes", "load", shape.stem, shape, "--db", database])
    run(
        [
            binary,
            "shapes",
            "load",
            "publication-policy",
            tooling / "docs/knowledge/publication-policy.shapes.ttl",
            "--db",
            database,
        ]
    )
    run(
        [
            binary,
            "knot",
            tooling / "docs/knowledge/publication-policy.ttl",
            "--db",
            database,
        ]
    )
    run(
        [
            binary,
            "knot",
            policy,
            "--graph",
            "urn:quipu:private-release-policy",
            "--db",
            database,
        ]
    )
    # No guessed repository/entity graph identifier. Actual indexed pointers
    # and sealed source_repository/source_revision provenance identify this corpus.
    contributor = private / "contributor-knowledge.ttl"
    run(
        [
            "node",
            source / "scripts/build-contributor-knowledge.mjs",
            source,
            contributor,
        ]
    )
    run([binary, "knot", contributor, "--db", database])
    # Curated repository content and validating shapes are repo-owned. Refuse
    # an incomplete source proposal rather than minting a pointer-only stub.
    registry_path = source / "docs/knowledge/repository-knowledge.json"
    knowledge = private / "repository-knowledge.ttl"
    constraints = source / "docs/knowledge/repository-knowledge.shapes.ttl"
    queries = source / "docs/knowledge/stored-queries.json"
    alignments = source / "docs/knowledge/alignments.ttl"
    for required in (registry_path, constraints, queries, alignments):
        if not required.is_file() or not required.stat().st_size:
            raise ValueError(
                "required repository knowledge member missing: " + required.name
            )
    import importlib.util

    spec = importlib.util.spec_from_file_location(
        "repository_knowledge", source / "scripts/build-repository-knowledge.py"
    )
    generator = importlib.util.module_from_spec(spec)
    _previous_bytecode = sys.dont_write_bytecode
    try:
        sys.dont_write_bytecode = True
        spec.loader.exec_module(generator)
    finally:
        sys.dont_write_bytecode = _previous_bytecode
    records = json.loads(registry_path.read_text())["records"]
    labels = indexed_labels(
        binary, database, sorted({record["subject"] for record in records})
    )
    knowledge.write_text(
        generator.render(source, revision, lambda subject: labels.get(subject))
        + "\n"
        + alignment_carrier(source)
    )
    run([binary, "validate", "--shapes", constraints, "--data", knowledge])
    run(
        [
            binary,
            "shapes",
            "load",
            "repository-knowledge",
            constraints,
            "--db",
            database,
        ]
    )
    run([binary, "knot", knowledge, "--db", database])
    register_queries(binary, database, json.loads(queries.read_text()))
    query = (source / "queries/repository-share-quipu.rq").read_text()
    stage = private / "share"
    native = native_alignments(binary, private)
    run(
        [binary, "share", "--construct", query, "--turtle", "--output", stage]
        + (["--alignments", alignments] if native else [])
        + ["--db", database]
    )
    if native:
        verify_carried_alignments(stage, alignments)
    if (
        not (stage / "shapes.ttl").is_file()
        or (stage / "shapes.ttl").stat().st_size == 0
    ):
        raise ValueError("repository share omitted receiver shapes")
    verify_former_names(stage / "export.nt")
    receiver = private / "receiver.db"
    run(
        [
            binary,
            "shapes",
            "load",
            "repository-share",
            stage / "shapes.ttl",
            "--db",
            receiver,
        ]
    )
    imported = json.loads(run([binary, "import", stage, "--db", receiver]))
    if (
        imported.get("outcome") != "staged"
        or imported.get("promotion", {}).get("eligible") is not True
        or type(imported.get("triples", {}).get("accepted")) is not int
        or imported["triples"]["accepted"] <= 3
    ):
        raise ValueError("repository receiver did not admit a non-stub corpus")
    run([binary, "import", "promote", imported["share_id"], "--db", receiver])
    verify_agent_references(binary, receiver, (control, *references))
    run(
        [
            "python3",
            tooling / "scripts/verify-repository-share-receiver.py",
            binary,
            stage,
            receiver,
            private,
        ]
    )
    if run(["git", "-C", source, "status", "--porcelain"]).strip():
        raise ValueError("indexing changed the exact repository source")
    shutil.move(stage, output)
    return {
        "triples": imported["triples"]["accepted"],
        "positive_file": control,
        "agent_references": list(references),
    }


if __name__ == "__main__":
    import sys

    if len(sys.argv) != 7:
        raise SystemExit(
            "usage: build-repository-share.py <revision> <source> <engine-source> <quipu-bin> <bobbin-bin> <new-output>"
        )
    build(*sys.argv[1:])
