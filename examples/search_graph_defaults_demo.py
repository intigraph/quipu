import os, json, subprocess, sqlite3
from pathlib import Path
for kind in ["existing", "new"]:
    db = Path(kind + ".db")
    print("$ search_graph_defaults_fixture", kind, db, flush=True)
    run = subprocess.run([os.environ["QUIPU_FIXTURE"], kind, str(db)], text=True, capture_output=True, check=True)
    data = json.loads(run.stdout)
    for label in ["keyword", "prepared_default", "explicit_all"]:
        result = data[label]
        print(json.dumps({"operation": label, "count": result.get("count"), "search_scope": result.get("search_scope"), "entities": [r.get("entity") for r in result.get("results", [])]}, sort_keys=True), flush=True)
    print(json.dumps({"unprepared_semantic": data["unprepared_semantic"]}, sort_keys=True), flush=True)
    with sqlite3.connect(db) as connection:
        before = {"policy": connection.execute("SELECT * FROM search_default_policy").fetchall(), "identity": connection.execute("SELECT * FROM store_identity").fetchall(), "facts": connection.execute("SELECT * FROM facts").fetchall()}
    print('$ prior_quipu query "SELECT ?name WHERE { <urn:root> <http://www.w3.org/2000/01/rdf-schema#label> ?name }" --db ' + str(db), flush=True)
    old = subprocess.run([os.environ["QUIPU_PRIOR"], "query", "SELECT ?name WHERE { <urn:root> <http://www.w3.org/2000/01/rdf-schema#label> ?name }", "--db", str(db)], text=True, capture_output=True)
    print(old.stdout.strip(), flush=True)
    if old.returncode: raise RuntimeError("old reader failed: " + old.stderr)
    assert "shared root" in old.stdout, old.stdout
    with sqlite3.connect(db) as connection:
        after = {"policy": connection.execute("SELECT * FROM search_default_policy").fetchall(), "identity": connection.execute("SELECT * FROM store_identity").fetchall(), "facts": connection.execute("SELECT * FROM facts").fetchall()}
    assert before == after, "old reader changed facts, identity or policy"
    print("old reader: positive ROOT query PASS; facts, identity and policy unchanged", flush=True)
    assert data["keyword"]["count"] == (1 if kind == "existing" else 2), data["keyword"]
    assert data["prepared_default"]["results"][0]["entity"] == ("urn:root" if kind == "existing" else "urn:named")
    assert data["explicit_all"]["results"][0]["entity"] == "urn:named"
    assert ("refused" in data["unprepared_semantic"]) == (kind == "new")
print("isolated migration + new-store + old-reader controls PASS; no installed service tested", flush=True)
