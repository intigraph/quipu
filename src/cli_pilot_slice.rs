//! `pilot export-slice` / `pilot import-slice`: one producer's current facts
//! carried between files as canonical terms, wrapping
//! `Store::export_routed_slice` and `Store::import_routed_slice`.
//!
//! The slice file names its tag and graph, carries every fact with an exactly
//! typed object, and pins `multiset_sha256`: sha256 over the library's own
//! sorted canonical forms (`slice_multiset`), one per line. Import recomputes
//! it before opening the owner file and again after writing, so a tampered or
//! truncated slice refuses and a completed import proves multiset equality.
//! `--into G --legacy-db LEGACY` lands a ROOT slice in the routed graph `G`
//! fenced for it in LEGACY, and records that retarget in the receipt.
use super::{Flags, Out, identity, open_ro, open_rw, parse, receipt, write_new_0600};
use quipu::store::{SliceFact, slice_multiset};
use quipu::types::Value;
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use std::path::Path;

/// Slice file format version written by `export-slice`.
const SLICE_VERSION: u64 = 1;
/// Upper bound on a slice file read by `import-slice`.
const MAX_SLICE_BYTES: u64 = 256 * 1024 * 1024;

fn literal_json(value: &Value) -> Result<Json, String> {
    Ok(match value {
        Value::Str(s) => json!({"str": s}),
        Value::Int(i) => json!({"int": i}),
        Value::Float(f) if f.is_finite() => json!({"float": f}),
        Value::Bool(b) => json!({"bool": b}),
        Value::Bytes(b) => json!({"bytes_hex": hex::encode(b)}),
        Value::Lang { lexical, lang } => json!({"lexical": lexical, "lang": lang}),
        Value::Typed { lexical, datatype } => json!({"lexical": lexical, "datatype": datatype}),
        _ => return Err("slice holds a value kind the v1 slice file cannot carry".into()),
    })
}

fn literal_value(object: &Json) -> Result<Value, String> {
    let bad = || "malformed slice fact object".to_string();
    let o = object.as_object().ok_or_else(bad)?;
    let text = |k: &str| o.get(k).and_then(Json::as_str).map(str::to_string);
    Ok(match (o.len(), o.keys().next().map(String::as_str)) {
        (1, Some("str")) => Value::Str(text("str").ok_or_else(bad)?),
        (1, Some("int")) => Value::Int(o["int"].as_i64().ok_or_else(bad)?),
        (1, Some("float")) => Value::Float(o["float"].as_f64().ok_or_else(bad)?),
        (1, Some("bool")) => Value::Bool(o["bool"].as_bool().ok_or_else(bad)?),
        (1, Some("bytes_hex")) => {
            Value::Bytes(hex::decode(text("bytes_hex").ok_or_else(bad)?).map_err(|_| bad())?)
        }
        (2, _) if o.contains_key("lang") => Value::Lang {
            lexical: text("lexical").ok_or_else(bad)?,
            lang: text("lang").ok_or_else(bad)?,
        },
        (2, _) if o.contains_key("datatype") => Value::Typed {
            lexical: text("lexical").ok_or_else(bad)?,
            datatype: text("datatype").ok_or_else(bad)?,
        },
        _ => return Err(bad()),
    })
}

fn fact_json(fact: &SliceFact) -> Result<Json, String> {
    let object = match &fact.object {
        Ok(iri) => json!({"iri": iri}),
        Err(value) => literal_json(value)?,
    };
    Ok(json!({
        "subject": fact.subject,
        "predicate": fact.predicate,
        "object": object,
        "valid_from": fact.valid_from,
    }))
}

fn fact_from_json(row: &Json) -> Result<SliceFact, String> {
    let field = |k: &str| {
        row.get(k)
            .and_then(Json::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("slice fact lacks `{k}`"))
    };
    let object = row.get("object").ok_or("slice fact lacks `object`")?;
    let object = match object.get("iri").and_then(Json::as_str) {
        Some(iri) if object.as_object().is_some_and(|o| o.len() == 1) => Ok(iri.to_string()),
        _ => Err(literal_value(object)?),
    };
    Ok(SliceFact {
        subject: field("subject")?,
        predicate: field("predicate")?,
        object,
        valid_from: field("valid_from")?,
    })
}

/// sha256 over the sorted canonical multiset, one form per line.
pub(super) fn multiset_sha256(facts: &[SliceFact]) -> String {
    let mut hasher = Sha256::new();
    for line in slice_multiset(facts) {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

fn existing(flags: &Flags, name: &str) -> Result<String, String> {
    let path = flags.get(name)?;
    if !Path::new(path).is_file() {
        return Err(format!("{name} must name an existing file"));
    }
    Ok(path.to_string())
}

pub(super) fn export_slice(rest: &[String]) -> Out {
    let flags = parse(rest, &["--db", "--tag", "--graph", "--out"], &["--execute"])?;
    let db = existing(&flags, "--db")?;
    let (tag, graph) = (flags.get("--tag")?, flags.get("--graph")?);
    let out = Path::new(flags.get("--out")?);
    if out.exists() {
        return Err("--out already exists; a slice file is never overwritten".into());
    }
    let execute = flags.on("--execute");
    let before = identity(Path::new(&db));
    let out_before = identity(out);
    let store = open_ro(&db)?;
    let facts = store
        .export_routed_slice(tag, graph)
        .map_err(|e| e.to_string())?;
    drop(store);
    let digest = multiset_sha256(&facts);
    let rows = facts.iter().map(fact_json).collect::<Result<Vec<_>, _>>()?;
    let doc = json!({
        "version": SLICE_VERSION, "tag": tag, "graph": graph,
        "count": facts.len(), "multiset_sha256": digest, "facts": rows,
    });
    if execute {
        let mut bytes = serde_json::to_vec_pretty(&doc).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        write_new_0600(out, &bytes)?;
    }
    Ok(receipt(
        "export-slice",
        execute,
        before,
        identity(Path::new(&db)),
        json!({
            "tag": tag, "graph": graph, "count": facts.len(), "multiset_sha256": digest,
            "out_before": out_before, "out_after": identity(out),
        }),
    ))
}

/// A parsed and digest-checked slice file.
struct Slice {
    tag: String,
    graph: String,
    facts: Vec<SliceFact>,
    digest: String,
}

fn read_slice(path: &Path) -> Result<Slice, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("cannot stat --in: {e}"))?;
    if meta.len() > MAX_SLICE_BYTES {
        return Err("--in exceeds the slice file size bound".into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read --in: {e}"))?;
    let doc: Json = serde_json::from_slice(&bytes).map_err(|_| "--in is not slice JSON")?;
    if doc.get("version").and_then(Json::as_u64) != Some(SLICE_VERSION) {
        return Err("unsupported slice file version".into());
    }
    let text = |k: &str| {
        doc.get(k)
            .and_then(Json::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("slice file lacks `{k}`"))
    };
    let facts = doc
        .get("facts")
        .and_then(Json::as_array)
        .ok_or("slice file lacks `facts`")?
        .iter()
        .map(fact_from_json)
        .collect::<Result<Vec<_>, _>>()?;
    let digest = text("multiset_sha256")?;
    if doc.get("count").and_then(Json::as_u64) != Some(facts.len() as u64)
        || multiset_sha256(&facts) != digest
    {
        return Err("slice count or multiset_sha256 does not match its facts".into());
    }
    Ok(Slice {
        tag: text("tag")?,
        graph: text("graph")?,
        facts,
        digest,
    })
}

/// `--into G --legacy-db LEGACY`: land a ROOT slice in the routed graph `G`.
///
/// Admitted only when LEGACY (the slice's source file) has `G` fenced with a
/// verified fence, has not routed the tag to another graph, and still holds
/// exactly this slice for the tag in ROOT. Read-only; returns the receipt's
/// retarget record.
fn retarget(slice: &Slice, into: &str, legacy: &str) -> Result<Json, String> {
    let root = quipu::schema::ROOT_GRAPH_IRI;
    if slice.graph != root {
        return Err("--into retargets a slice exported from ROOT only".into());
    }
    if into == root {
        return Err("--into names a routed graph, not ROOT".into());
    }
    let before = identity(Path::new(legacy));
    let store = open_ro(legacy)?;
    let fence = store
        .routed_graph_fence_rows()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|(iri, _, _)| iri == into);
    let Some((_, revision, generation)) = fence else {
        return Err(
            "--into graph is not fenced in --legacy-db; it is not routed for this source".into(),
        );
    };
    store.verify_routed_fence().map_err(|e| e.to_string())?;
    let routed = store
        .routed_source_fence_rows()
        .map_err(|e| e.to_string())?;
    if routed
        .iter()
        .any(|(tag, graph, _, _)| *tag == slice.tag && graph != into)
    {
        return Err("--legacy-db routes this tag to another graph".into());
    }
    let source = store
        .export_routed_slice(&slice.tag, root)
        .map_err(|e| e.to_string())?;
    drop(store);
    if multiset_sha256(&source) != slice.digest {
        return Err("--legacy-db does not hold this slice for its tag in ROOT".into());
    }
    Ok(json!({
        "from_graph": root, "into_graph": into,
        "legacy_fence": {"catalog_revision": revision, "generation": generation},
        "legacy_before": before, "legacy_after": identity(Path::new(legacy)),
    }))
}

pub(super) fn import_slice(rest: &[String]) -> Out {
    let flags = parse(
        rest,
        &["--owner-file", "--in", "--at", "--into", "--legacy-db"],
        &["--execute"],
    )?;
    let owner = existing(&flags, "--owner-file")?;
    let path = Path::new(&owner);
    let mut slice = read_slice(Path::new(&existing(&flags, "--in")?))?;
    let execute = flags.on("--execute");
    let before = identity(path);
    // The multiset is always the slice's own; only the landing graph moves.
    let retargeted = match (flags.opt("--into"), flags.opt("--legacy-db")) {
        (None, None) => None,
        (Some(into), Some(_)) => {
            let record = retarget(&slice, into, &existing(&flags, "--legacy-db")?)?;
            slice.graph = into.to_string();
            Some(record)
        }
        _ => return Err("--into and --legacy-db are given together or not at all".into()),
    };
    // Read-only preflight of the same conditions the library refuses on, so a
    // refused import never opens the owner file for writing.
    let store = open_ro(&owner)?;
    let present = store
        .export_routed_slice(&slice.tag, &slice.graph)
        .map_err(|e| e.to_string())?;
    drop(store);
    if !present.is_empty() {
        return Err(
            "the tag already owns current facts in this graph; a slice imports once".into(),
        );
    }
    let mut plan = json!({
        "tag": slice.tag, "graph": slice.graph, "count": slice.facts.len(),
        "multiset_sha256": slice.digest,
    });
    if let Some(record) = retargeted {
        plan["retarget"] = record;
    }
    if !execute {
        return Ok(receipt(
            "import-slice",
            false,
            before,
            identity(path),
            json!({"plan": plan}),
        ));
    }
    let at = flags
        .opt("--at")
        .map_or_else(quipu::time::now_iso, str::to_string);
    let mut store = open_rw(&owner)?;
    let tx = store
        .import_routed_slice(&slice.facts, &slice.tag, &slice.graph, &at)
        .map_err(|e| e.to_string())?;
    let landed = store
        .export_routed_slice(&slice.tag, &slice.graph)
        .map_err(|e| e.to_string())?;
    drop(store);
    let landed_digest = multiset_sha256(&landed);
    if landed_digest != slice.digest {
        return Err(format!(
            "imported multiset differs from the slice (tx {tx:?}); inspect before retrying"
        ));
    }
    Ok(receipt(
        "import-slice",
        true,
        before,
        identity(path),
        json!({
            "plan": plan, "tx": tx, "at": at, "landed_multiset_sha256": landed_digest,
        }),
    ))
}
