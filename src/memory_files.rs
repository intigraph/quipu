//! Versioned UTF-8 file records; native harness routing belongs to the caller.
//!
//! Revisions and receipts are ordinary graph facts, not a second database or
//! last-write-wins episode description. Public adapters must supply authenticated
//! authority; a local Store holder already has filesystem-level authority.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Datum, Error, Op, Result, Store, Value};

const VALUE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#value";
const TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const HEAD: &str = "https://quipu.dev/memory#head";
pub const MAX_FILE_BYTES: usize = 256 * 1024;
pub const MAX_FILES: usize = 128;
pub const MAX_OWNER_BYTES: usize = 4 * 1024 * 1024;

mod model;
pub(crate) mod write_scope;
pub use model::*;

fn invalid(message: &str) -> Error {
    Error::InvalidValue(message.into())
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|_| invalid("invalid memory record"))
}

fn owner_id(owner: &Owner) -> Result<String> {
    for value in [&owner.deployment, &owner.project, &owner.crew] {
        if value.len() > 2048 || oxrdf::NamedNode::new(value.as_str()).is_err() {
            return Err(invalid("memory owner requires absolute identity IRIs"));
        }
    }
    Ok(format!(
        "urn:quipu:memory:owner:{}",
        digest(json(owner)?.as_bytes())
    ))
}

fn validate(input: &Commit, authority: &Authority<'_>) -> Result<String> {
    match authority {
        Authority::Local => (),
        Authority::Named(principal) if principal.iri == input.owner.crew => (),
        _ => {
            return Err(Error::CannotVerify(
                "memory writer identity is unproven or differs".into(),
            ));
        }
    }
    let owner = owner_id(&input.owner)?;
    if json(input)?.len() > 1024 * 1024 {
        return Err(invalid("memory request exceeds wire limit"));
    }
    if input.path.is_empty()
        || input.path.len() > 1024
        || input.path.starts_with('/')
        || input.path.contains(['\\', '\0'])
        || input
            .path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
        || input.path.split('/').count() > 8
    {
        return Err(invalid("invalid relative memory path"));
    }
    if input
        .text
        .as_ref()
        .is_some_and(|s| s.len() > MAX_FILE_BYTES)
        || input.idempotency_key.is_empty()
        || input.idempotency_key.len() > 128
        || input.harness.is_empty()
        || input.harness.len() > 128
        || input.account.len() > 256
    {
        return Err(invalid("memory request exceeds limits"));
    }
    Ok(owner)
}

fn text_fact(store: &Store, graph: i64, subject: &str, predicate: &str) -> Result<Option<String>> {
    let (Some(entity), Some(attribute)) = (store.lookup(subject)?, store.lookup(predicate)?) else {
        return Ok(None);
    };
    let mut statement = store.conn.prepare(
        "SELECT DISTINCT v FROM facts WHERE g=?1 AND e=?2 AND a=?3 \
         AND op=1 AND valid_to IS NULL LIMIT 2",
    )?;
    let values = statement
        .query_map(rusqlite::params![graph, entity, attribute], |row| {
            row.get::<_, Vec<u8>>(0)
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    match values.as_slice() {
        [] => Ok(None),
        [bytes] => match Value::from_bytes(bytes)? {
            Value::Str(value) if value.len() <= 1024 * 1024 => Ok(Some(value)),
            _ => Err(invalid("invalid memory record value")),
        },
        _ => Err(invalid("ambiguous memory record")),
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(
    store: &Store,
    graph: i64,
    subject: &str,
) -> Result<Option<T>> {
    text_fact(store, graph, subject, VALUE)?
        .map(|value| serde_json::from_str(&value).map_err(|_| invalid("malformed memory record")))
        .transpose()
}

/// An explicit owner read. Cross-crew reads use the inherited graph boundary.
pub fn read(store: &Store, graph: i64, owner: &Owner) -> Result<Option<Snapshot>> {
    let subject = owner_id(owner)?;
    let Some(head) = text_fact(store, graph, &subject, HEAD)? else {
        return Ok(None);
    };
    let snapshot: Snapshot =
        read_json(store, graph, &head)?.ok_or_else(|| invalid("memory head has no snapshot"))?;
    if &snapshot.owner != owner || snapshot.revision != head || snapshot.files.len() > MAX_FILES {
        return Err(invalid("memory snapshot identity differs"));
    }
    Ok(Some(snapshot))
}

pub fn revision(store: &Store, graph: i64, receipt: &Receipt) -> Result<Revision> {
    let file: Revision = read_json(store, graph, &receipt.file.revision)?
        .ok_or_else(|| invalid("memory revision missing"))?;
    let data = file.text.as_deref().unwrap_or("");
    if file.owner != receipt.owner
        || file.path != receipt.path
        || digest(data.as_bytes()) != receipt.file.sha256
        || data.len() != receipt.file.bytes
        || file.text.is_none() != receipt.file.tombstone
    {
        return Err(invalid("memory revision readback differs"));
    }
    Ok(file)
}

/// Export one immutable owner snapshot, without changing native files.
pub fn export(store: &Store, graph: i64, owner: &Owner) -> Result<Export> {
    let snapshot = read(store, graph, owner)?;
    let mut files = Vec::new();
    if let Some(snapshot) = &snapshot {
        for (path, file) in &snapshot.files {
            let receipt = Receipt {
                owner: owner.clone(),
                snapshot: snapshot.revision.clone(),
                path: path.clone(),
                file: file.clone(),
                request_hash: String::new(),
                committed_at: snapshot.committed_at.clone(),
            };
            files.push(ExportFile {
                revision: file.revision.clone(),
                file: revision(store, graph, &receipt)?,
            });
        }
    }
    Ok(Export {
        format: "quipu.memory-files/v1".into(),
        coverage: "this_repository_owner_records".into(),
        snapshot,
        files,
    })
}

fn datum(
    store: &Store,
    subject: &str,
    predicate: &str,
    value: Value,
    timestamp: &str,
    op: Op,
) -> Result<Datum> {
    Ok(Datum {
        entity: store.intern(subject)?,
        attribute: store.intern(predicate)?,
        value,
        valid_from: timestamp.into(),
        valid_to: None,
        op,
    })
}

/// Commit one immutable file revision and a complete owner snapshot atomically.
///
/// Expected heads and receipt absence are checked with SQLite writer exclusion
/// inside the normal write savepoint. No outer transaction can erase durable
/// denial evidence. Nested callers refuse; replay reads precede stale checks.
pub fn commit(
    store: &mut Store,
    graph: i64,
    input: &Commit,
    authority: Authority<'_>,
    timestamp: &str,
) -> Result<Receipt> {
    let owner = validate(input, &authority)?;
    if !store.conn.is_autocommit() {
        return Err(invalid(
            "memory commit requires its own transaction boundary",
        ));
    }
    store.assert_write_target_allowed(graph, Some("memory-file"))?;
    let intent = digest(json(input)?.as_bytes());
    let receipt_id = format!(
        "urn:quipu:memory:receipt:{}",
        digest(json(&(&input.owner, &input.idempotency_key))?.as_bytes())
    );
    commit_locked(store, graph, input, &owner, &intent, &receipt_id, timestamp)
}

/// Read an exact intent's receipt without retrying a possibly committed write.
pub fn receipt(
    store: &Store,
    graph: i64,
    input: &Commit,
    authority: Authority<'_>,
) -> Result<Option<Receipt>> {
    validate(input, &authority)?;
    let intent = digest(json(input)?.as_bytes());
    let id = format!(
        "urn:quipu:memory:receipt:{}",
        digest(json(&(&input.owner, &input.idempotency_key))?.as_bytes())
    );
    replay(store, graph, input, &intent, &id)
}

fn replay(
    store: &Store,
    graph: i64,
    input: &Commit,
    intent: &str,
    receipt_id: &str,
) -> Result<Option<Receipt>> {
    // Replay precedes stale-parent rejection: the first commit moved heads.
    if let Some(receipt) = read_json::<Receipt>(store, graph, receipt_id)? {
        return if receipt.request_hash == intent
            && receipt.owner == input.owner
            && receipt.path == input.path
            && receipt.snapshot == format!("urn:quipu:memory:snapshot:{intent}")
            && receipt.file.revision == format!("urn:quipu:memory:revision:{intent}")
        {
            revision(store, graph, &receipt)?;
            Ok(Some(receipt))
        } else {
            Err(invalid("memory idempotency key intent differs"))
        };
    }
    Ok(None)
}

fn commit_locked(
    store: &mut Store,
    graph: i64,
    input: &Commit,
    owner: &str,
    intent: &str,
    receipt_id: &str,
    timestamp: &str,
) -> Result<Receipt> {
    if let Some(receipt) = replay(store, graph, input, intent, receipt_id)? {
        return Ok(receipt);
    }
    let previous = read(store, graph, &input.owner)?;
    let parent = previous.as_ref().map(|s| s.revision.clone());
    let mut files = previous
        .as_ref()
        .map(|s| s.files.clone())
        .unwrap_or_default();
    let file_parent = files.get(&input.path).map(|f| f.revision.clone());
    if input.expected_snapshot != parent || input.expected_file != file_parent {
        return Err(Error::ConditionalWriteConflict);
    }
    if input.text.is_none() && !files.contains_key(&input.path) {
        return Err(invalid("cannot tombstone an unknown memory file"));
    }
    let file_id = format!("urn:quipu:memory:revision:{intent}");
    let snapshot_id = format!("urn:quipu:memory:snapshot:{intent}");
    let data = input.text.as_deref().unwrap_or("");
    let file = FileHead {
        revision: file_id.clone(),
        sha256: digest(data.as_bytes()),
        bytes: data.len(),
        tombstone: input.text.is_none(),
    };
    files.insert(input.path.clone(), file.clone());
    if files.len() > MAX_FILES || files.values().map(|f| f.bytes).sum::<usize>() > MAX_OWNER_BYTES {
        return Err(invalid("memory owner snapshot exceeds limits"));
    }
    let snapshot = Snapshot {
        owner: input.owner.clone(),
        revision: snapshot_id.clone(),
        parent: parent.clone(),
        committed_at: timestamp.into(),
        files,
    };
    let version = Revision {
        owner: input.owner.clone(),
        path: input.path.clone(),
        parent: file_parent,
        text: input.text.clone(),
        harness: input.harness.clone(),
        account: input.account.clone(),
        committed_at: timestamp.into(),
    };
    let receipt = Receipt {
        owner: input.owner.clone(),
        snapshot: snapshot_id.clone(),
        path: input.path.clone(),
        file,
        request_hash: intent.into(),
        committed_at: timestamp.into(),
    };
    let mut datums = Vec::new();
    for (subject, value) in [
        (file_id.as_str(), json(&version)?),
        (snapshot_id.as_str(), json(&snapshot)?),
        (receipt_id, json(&receipt)?),
    ] {
        datums.push(datum(
            store,
            subject,
            VALUE,
            Value::Str(value),
            timestamp,
            Op::Assert,
        )?);
    }
    // Existing base types are reused; no Memory/Revision class is minted.
    // The JSON carries lossless source, while these edges permit attributed
    // cross-crew graph queries without parsing another crew's native files.
    for (subject, kind) in [
        (owner, "owner"),
        (file_id.as_str(), "revision"),
        (snapshot_id.as_str(), "snapshot"),
        (receipt_id, "receipt"),
    ] {
        datums.push(datum(
            store,
            subject,
            TYPE,
            Value::Ref(store.intern(&format!("{}Artifact", store.base_ns))?),
            timestamp,
            Op::Assert,
        )?);
        datums.push(datum(
            store,
            subject,
            "https://quipu.dev/memory#kind",
            Value::Str(kind.into()),
            timestamp,
            Op::Assert,
        )?);
        for (predicate, target) in [
            ("https://quipu.dev/memory#forCrew", &input.owner.crew),
            ("https://quipu.dev/memory#project", &input.owner.project),
            (
                "https://quipu.dev/memory#deployment",
                &input.owner.deployment,
            ),
        ] {
            datums.push(datum(
                store,
                subject,
                predicate,
                Value::Ref(store.intern(target)?),
                timestamp,
                Op::Assert,
            )?);
        }
    }
    if let Some(old) = parent.clone() {
        datums.push(datum(
            store,
            owner,
            HEAD,
            Value::Str(old),
            timestamp,
            Op::Retract,
        )?);
    }
    datums.push(datum(
        store,
        owner,
        HEAD,
        Value::Str(snapshot_id),
        timestamp,
        Op::Assert,
    )?);
    let actor = input.owner.crew.as_str(); // authenticated separately; Local remains filesystem authority.
    use crate::store::conditional::ExpectedValue;
    let expected = [
        ExpectedValue {
            entity: store.intern(owner)?,
            attribute: store.intern(HEAD)?,
            value: parent.map(Value::Str),
        },
        ExpectedValue {
            entity: store.intern(receipt_id)?,
            attribute: store.intern(VALUE)?,
            value: None,
        },
    ];
    let result = write_scope::protocol(graph, &datums, || {
        store.transact_to_graph_if(
            &datums,
            timestamp,
            Some(actor),
            Some("memory-file"),
            graph,
            &expected,
        )
    });
    if let Err(error) = result {
        if matches!(&error, Error::ConditionalWriteConflict)
            && let Some(receipt) = replay(store, graph, input, intent, receipt_id)?
        {
            return Ok(receipt);
        }
        return Err(error);
    }
    Ok(receipt)
}

#[cfg(test)]
mod tests;
