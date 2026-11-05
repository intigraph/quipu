//! Pure competency member sealing and receiver-side recompilation.
//!
//! This layer performs no file, store, command or network operations.
use super::{Compiled, FILE, parse};
use crate::error::{Error, Result};
use crate::share::sha256;
use crate::share_queries::SharedQuery;
use serde::{Deserialize, Serialize};

/// Fixed command RDF member; a manifest must never choose an arbitrary path.
pub const COMMANDS_FILE: &str = "commands.ttl";

/// Exact source and compiled bytes carried together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Members {
    /// Verbatim authoring text, including comments and whitespace.
    pub authoring: String,
    /// Deterministic compiled question RDF.
    pub queries: String,
    /// Deterministic compiled command RDF, never executed by import.
    pub commands: String,
}

/// Digests to include in the enclosing share identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seals {
    /// SHA-256 of exact competency.toml bytes.
    pub competency_hash: String,
    /// SHA-256 of exact compiled question bytes.
    pub queries_hash: String,
    /// SHA-256 of exact commands.ttl bytes.
    pub commands_hash: String,
}

impl Members {
    /// Compile without changing the authoring bytes or executing definitions.
    ///
    /// # Errors
    /// Invalid authoring contract or unresolved query references.
    pub fn compile(authoring: &str, registry: &[SharedQuery]) -> Result<Self> {
        let contract = parse(authoring, registry)?;
        let ids: std::collections::BTreeSet<_> =
            contract.questions.iter().map(|q| q.id.as_str()).collect();
        if contract
            .questions
            .iter()
            .any(|q| q.query_ref.as_deref().is_some_and(|r| ids.contains(r)))
        {
            return Err(Error::InvalidValue(
                "competency query_ref must name an independent carried query".into(),
            ));
        }
        if registry.iter().any(|q| ids.contains(q.query.name.as_str())) {
            return Err(Error::InvalidValue(
                "competency question id collides with an independent carried query".into(),
            ));
        }
        let Compiled { queries, commands } = contract.compile(registry)?;
        let independent: Vec<_> = registry
            .iter()
            .filter(|q| !ids.contains(q.query.name.as_str()))
            .cloned()
            .collect();
        let queries = format!(
            "{}{}",
            crate::share_queries::to_turtle(&independent),
            queries
        );
        Ok(Self {
            authoring: authoring.into(),
            queries,
            commands,
        })
    }

    /// Seal all three exact byte streams; formatting changes alter identity.
    #[must_use]
    pub fn seals(&self) -> Seals {
        Seals {
            competency_hash: sha256(self.authoring.as_bytes()),
            queries_hash: sha256(self.queries.as_bytes()),
            commands_hash: sha256(self.commands.as_bytes()),
        }
    }

    /// Check seals, then recompile source and refuse any compiled difference.
    ///
    /// Matching hashes alone are insufficient: an author can reseal inconsistent
    /// source and RDF. The receiver always runs its own compiler before staging.
    ///
    /// # Errors
    /// Hash mismatch, invalid source, missing reference or differing RDF bytes.
    pub fn verify(&self, seals: &Seals, registry: &[SharedQuery]) -> Result<()> {
        let actual = self.seals();
        for (name, expected, observed) in [
            (FILE, &seals.competency_hash, &actual.competency_hash),
            (
                crate::share_queries::QUERIES_FILE,
                &seals.queries_hash,
                &actual.queries_hash,
            ),
            (COMMANDS_FILE, &seals.commands_hash, &actual.commands_hash),
        ] {
            if expected != observed {
                return Err(Error::InvalidValue(format!("share {name} hash mismatch")));
            }
        }
        let rebuilt = Self::compile(&self.authoring, registry)?;
        if rebuilt.queries != self.queries {
            return Err(Error::InvalidValue(
                "competency recompilation differs from queries.ttl".into(),
            ));
        }
        if rebuilt.commands != self.commands {
            return Err(Error::InvalidValue(
                "competency recompilation differs from commands.ttl".into(),
            ));
        }
        Ok(())
    }
}

pub(crate) fn compile_optional(
    source: Option<&str>,
    registry: &[SharedQuery],
) -> Result<(Option<Members>, Option<String>)> {
    let members = source.map(|s| Members::compile(s, registry)).transpose()?;
    let queries = match &members {
        Some(m) => Some(m.queries.clone()),
        None => (!registry.is_empty()).then(|| crate::share_queries::to_turtle(registry)),
    };
    Ok((members, queries))
}

/// Reject a graph-only consumer that cannot preserve the carried contract.
///
/// # Errors
/// Any competency declaration is present.
pub fn refuse_unsupported(manifest: &crate::share::ShareManifest, consumer: &str) -> Result<()> {
    if manifest.competency_hash.is_some()
        || manifest.commands_hash.is_some()
        || manifest.files.competency.is_some()
        || manifest.files.commands.is_some()
    {
        return Err(Error::InvalidValue(format!(
            "{consumer} does not support competency members; use share/import"
        )));
    }
    Ok(())
}

fn independent_queries(authoring: &str, turtle: &str) -> Result<String> {
    use oxrdf::{NamedOrBlankNode, Term};
    use oxrdfio::{RdfFormat, RdfParser};
    // The unvalidated source is used only to identify candidate blocks to remove.
    // Full lint/compile equality below decides whether any result can be used.
    let contract: super::Contract = toml::from_str(authoring)
        .map_err(|e| Error::InvalidValue(format!("competency source: {e}")))?;
    let subjects: std::collections::BTreeSet<_> = contract
        .questions
        .iter()
        .map(|q| format!("urn:quipu:query:{}", q.id))
        .collect();
    let triples = RdfParser::from_format(RdfFormat::Turtle)
        .for_reader(turtle.as_bytes())
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::InvalidValue(format!("competency query RDF: {e}")))?;
    let mut children = std::collections::BTreeSet::new();
    for triple in &triples {
        if matches!(&triple.subject, NamedOrBlankNode::NamedNode(n) if subjects.contains(n.as_str()))
            && let Term::BlankNode(b) = &triple.object
        {
            children.insert(b.as_str().to_owned());
        }
    }
    let mut plain = String::new();
    for triple in triples {
        let excluded = match &triple.subject {
            NamedOrBlankNode::NamedNode(n) => subjects.contains(n.as_str()),
            NamedOrBlankNode::BlankNode(b) => children.contains(b.as_str()),
        };
        if !excluded {
            plain.push_str(&format!("{triple} .\n"));
        }
    }
    Ok(plain)
}

/// Verify the complete sealed contract before returning a plain query projection.
/// The projection is internal only: it is never a substitute for checking full RDF.
///
/// # Errors
/// Partial declarations, wrong names, seals, source or recompilation differences.
pub fn verified_projection(
    request: &crate::share_import::ShareImportRequest,
) -> Result<Option<String>> {
    let m = &request.manifest;
    let all_absent = m.competency_hash.is_none()
        && m.commands_hash.is_none()
        && m.files.competency.is_none()
        && m.files.commands.is_none()
        && request.competency_toml.is_none()
        && request.commands_turtle.is_none();
    if all_absent {
        return Ok(None);
    }
    let refuse = || {
        Error::InvalidValue(
            "competency members must be declared, named and present together".into(),
        )
    };
    if m.files.competency.as_deref() != Some(FILE)
        || m.files.commands.as_deref() != Some(COMMANDS_FILE)
    {
        return Err(refuse());
    }
    let members = Members {
        authoring: request.competency_toml.clone().ok_or_else(refuse)?,
        queries: request.queries_turtle.clone().ok_or_else(refuse)?,
        commands: request.commands_turtle.clone().ok_or_else(refuse)?,
    };
    let seals = Seals {
        competency_hash: m.competency_hash.clone().ok_or_else(refuse)?,
        queries_hash: m.queries_hash.clone().ok_or_else(refuse)?,
        commands_hash: m.commands_hash.clone().ok_or_else(refuse)?,
    };
    // Parse only an internal candidate projection. No store writes or registry
    // installation occurs until the full source/RDF recompile comparison passes.
    let plain = independent_queries(&members.authoring, &members.queries)?;
    let registry = crate::share_queries::from_turtle(&plain)?;
    members.verify(&seals, &registry)?;
    Ok(Some(crate::share_queries::to_turtle(&registry)))
}

/// Verify optional competency carriage with no side effects.
///
/// # Errors
/// See [`verified_projection`].
pub fn verify_request(request: &crate::share_import::ShareImportRequest) -> Result<()> {
    verified_projection(request).map(|_| ())
}

/// Retain verified source/RDF together as inert import evidence, not graph data.
pub(crate) fn retain(
    store: &crate::Store,
    request: &crate::share_import::ShareImportRequest,
) -> Result<()> {
    if let Some(authoring) = &request.competency_toml {
        store.conn.execute_batch("CREATE TABLE IF NOT EXISTS share_competency_members(share_id TEXT PRIMARY KEY, authoring TEXT NOT NULL, queries TEXT NOT NULL, commands TEXT NOT NULL)")?;
        store.conn.execute("INSERT INTO share_competency_members(share_id,authoring,queries,commands) VALUES(?1,?2,?3,?4) ON CONFLICT(share_id) DO NOTHING", rusqlite::params![request.manifest.share_id, authoring, request.queries_turtle, request.commands_turtle])?;
    }
    Ok(())
}

/// Read retained verified contract data; this grants no execution authority.
///
/// # Errors
/// Store read errors.
pub fn imported_members(store: &crate::Store, share_id: &str) -> Result<Option<Members>> {
    use rusqlite::OptionalExtension;
    let exists: bool = store.conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='share_competency_members')", [], |r| r.get(0))?;
    if !exists {
        return Ok(None);
    }
    Ok(store
        .conn
        .query_row(
            "SELECT authoring,queries,commands FROM share_competency_members WHERE share_id=?1",
            [share_id],
            |r| {
                Ok(Members {
                    authoring: r.get(0)?,
                    queries: r.get(1)?,
                    commands: r.get(2)?,
                })
            },
        )
        .optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    const SOURCE: &str = r#"# preserve this comment
schema = "quipu-competency/1"
[[question]]
id = "present"
ask = "Is someone present?"
query = "ASK { ?p a <urn:Person> }"
justifies = ["urn:PersonShape"]
[[command]]
id = "record"
does = "Record a person"
inputs = { person = "iri" }
template = '<{person}> a <urn:Person> .'
satisfies = ["urn:PersonShape"]
write_class = "record-person"
key = ["person"]
example = { person = "urn:alice" }
"#;

    #[test]
    fn verbatim_source_and_both_compiled_members_verify_without_execution() {
        let members = Members::compile(SOURCE, &[]).unwrap();
        assert_eq!(members.authoring, SOURCE);
        members.verify(&members.seals(), &[]).unwrap();
        let formatted = Members::compile(&format!("{SOURCE}\n"), &[]).unwrap();
        assert_ne!(
            members.seals().competency_hash,
            formatted.seals().competency_hash
        );
        assert_eq!(members.queries, formatted.queries);
        assert_eq!(members.commands, formatted.commands);
    }

    #[test]
    fn each_unsealed_member_change_is_refused() {
        let original = Members::compile(SOURCE, &[]).unwrap();
        for field in 0..3 {
            let mut changed = original.clone();
            match field {
                0 => changed.authoring.push(' '),
                1 => changed.queries.push(' '),
                _ => changed.commands.push(' '),
            }
            assert!(
                changed
                    .verify(&original.seals(), &[])
                    .unwrap_err()
                    .to_string()
                    .contains("hash mismatch")
            );
        }
    }

    #[test]
    fn resealing_cannot_hide_source_or_compiled_member_disagreement() {
        let original = Members::compile(SOURCE, &[]).unwrap();
        for field in 0..3 {
            let mut changed = original.clone();
            match field {
                0 => {
                    changed.authoring =
                        SOURCE.replace("Is someone present?", "Is a person present?");
                }
                1 => changed.queries.push_str("\n# forged question member\n"),
                _ => {
                    changed.commands = changed.commands.replace("record-person", "different-class");
                }
            }
            assert!(
                changed
                    .verify(&changed.seals(), &[])
                    .unwrap_err()
                    .to_string()
                    .contains("recompilation differs")
            );
        }
    }

    #[test]
    fn resealed_invalid_authoring_and_missing_references_are_refused() {
        let mut members = Members::compile(SOURCE, &[]).unwrap();
        members.authoring = SOURCE.replace("ASK {", "BROKEN {");
        assert!(members.verify(&members.seals(), &[]).is_err());
        let referenced = SOURCE.replace(
            "query = \"ASK { ?p a <urn:Person> }\"",
            "query_ref = \"missing\"",
        );
        assert!(Members::compile(&referenced, &[]).is_err());
    }
}
