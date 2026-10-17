use super::*;

/// Receipt for an explicit, repository-scoped reversal of local suppression.
#[derive(Debug, Serialize)]
pub struct TombstoneRevocation {
    pub repo: String,
    pub pack_key: String,
    pub statement: String,
    pub revoked: bool,
    pub graphs_restored: usize,
    pub reason: String,
}

/// Undo one local retraction, including its carried-forward policy.
///
/// Restore only matching composition facts already retracted in this repo's
/// owned graphs. Deleting the ledger row alone is insufficient: the next
/// composition would harvest those retractions again. CURRENT/PREVIOUS pointers
/// do not move; their matching retracted facts become visible immediately.
/// Facts omitted before a snapshot was stored are never fabricated: replaying
/// or rolling back that stripped graph stays stripped. A fresh source-pendant
/// snapshot can restore the statement after suppression is revoked.
pub fn revoke_tombstone(
    store: &mut Store,
    repo: &str,
    pack_key: &str,
    statement: &str,
    reason: &str,
    timestamp: &str,
    actor: Option<&str>,
) -> Result<TombstoneRevocation> {
    check_name("repository", repo)?;
    check_name("pack key", pack_key)?;
    if reason.trim().is_empty() || reason.len() > 1000 {
        return Err(Error::InvalidValue(
            "revocation needs a nonblank reason of at most 1000 bytes".into(),
        ));
    }
    let triples = RdfParser::from_format(RdfFormat::NTriples)
        .for_reader(statement.as_bytes())
        .map(|q| {
            q.map(Triple::from)
                .map_err(|e| Error::InvalidValue(e.to_string()))
        })
        .collect::<Result<Vec<_>>>()?;
    if triples.len() != 1 {
        return Err(Error::InvalidValue(
            "revocation requires exactly one N-Triples statement".into(),
        ));
    }
    let triple = &triples[0];
    if matches!(triple.subject, NamedOrBlankNode::BlankNode(_))
        || matches!(triple.object, Term::BlankNode(_))
    {
        return Err(Error::InvalidValue(
            "blank-node tombstones cannot be carried or revoked".into(),
        ));
    }
    let statement = format!("{triple} .");
    savepoint(store, "quipu_pendant_revoke", |store| {
        let exists = tombstones_for(store, repo, pack_key)?.contains(&statement);
        let mut restored = 0;
        {
            let graphs: Vec<String> = {
                let mut q = store.conn.prepare("SELECT graph FROM pendant_repo_graphs WHERE repo = ?1 AND pack_key = ?2 ORDER BY graph")?;
                q.query_map(params![repo, pack_key], |row| row.get(0))?
                    .collect::<std::result::Result<_, _>>()?
            };
            for graph in graphs {
                let Some(g) = store.lookup(&graph)? else {
                    continue;
                };
                let facts = store.retracted_composition_facts_in_graph(g)?;
                let bytes = crate::rdf::serialize_facts(store, &facts, RdfFormat::NTriples)?;
                let text =
                    String::from_utf8(bytes).map_err(|e| Error::Serialization(e.to_string()))?;
                let mut matching = false;
                for line in text.lines() {
                    matching |= canonical_line(line)?.as_ref() == Some(&statement);
                }
                if !matching {
                    continue;
                }
                let datum = crate::store::Datum {
                    entity: crate::rdf::intern_subject(store, &triple.subject)?,
                    attribute: store.intern(triple.predicate.as_str())?,
                    value: crate::rdf::term_to_value(store, &triple.object)?,
                    valid_from: timestamp.into(),
                    valid_to: None,
                    op: crate::types::Op::Assert,
                };
                store.transact_to_graph(
                    &[datum],
                    timestamp,
                    actor,
                    Some("pendant-current:revoke"),
                    g,
                )?;
                restored += 1;
            }
            store.conn.execute("DELETE FROM pendant_repo_tombstones WHERE repo = ?1 AND pack_key = ?2 AND statement = ?3", params![repo, pack_key, statement])?;
        }
        Ok(TombstoneRevocation {
            repo: repo.into(),
            pack_key: pack_key.into(),
            statement: statement.clone(),
            revoked: exists || restored > 0,
            graphs_restored: restored,
            reason: reason.into(),
        })
    })
}
