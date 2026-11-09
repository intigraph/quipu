//! Private page preparation and acknowledgement, separated from live commit.
use super::*;
use crate::write_observation::{Stage, observe};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Binding {
    pub identity: String,
    pub ontologies: Vec<(String, String, String)>,
    pub premise_head: i64,
    pub timestamp: String,
    pub generation: String,
    pub deadline: i64,
}
impl Binding {
    pub fn capture(snapshot: &Snapshot, job: &pages::Job) -> Result<Self> {
        if job.identity != snapshot.identity
            || job.ontologies != snapshot.ontologies
            || job.premise_head != snapshot.premise_head
            || job.timestamp != snapshot.timestamp
        {
            return Err(invalid("OWL private job binding changed"));
        }
        if matches!(job.phase.as_str(), "FAILED" | "INVALIDATED") {
            return Err(invalid("OWL private job is failed or invalidated"));
        }
        if !matches!(
            job.phase.as_str(),
            "FIXPOINT_COMPLETE" | "APPLYING" | "COMPLETE"
        ) {
            return Err(invalid("OWL private job has not reached a fixpoint"));
        }
        Ok(Self {
            identity: job.identity.clone(),
            ontologies: job.ontologies.clone(),
            premise_head: job.premise_head,
            timestamp: job.timestamp.clone(),
            generation: job.generation.clone(),
            deadline: job.deadline,
        })
    }
    pub fn validate(&self, live: &Store) -> Result<()> {
        pages::check_deadline(self.deadline)?;
        validate_premises(&self.identity, &self.ontologies, self.premise_head, live)
    }
}
pub(super) fn invalid(message: &str) -> Error {
    Error::InvalidValue(message.into())
}

pub(super) fn validate_premises(
    identity: &str,
    ontologies: &[(String, String, String)],
    head: i64,
    live: &Store,
) -> Result<()> {
    if live.store_id()? != identity || live.list_ontologies()? != ontologies {
        return Err(invalid("OWL snapshot identity or ontology changed"));
    }
    let freshness = live.lookup(DERIVED_AS_OF_TX)?.unwrap_or(-1);
    for graph in std::iter::once(0).chain(live.lookup(ROOT_INFERRED_GRAPH_IRI)?) {
        let retracted: bool = live.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM facts INDEXED BY idx_retracted_tx \
             WHERE g=?1 AND retracted_tx>?2 AND NOT (g!=0 AND e=?1 AND a=?3))",
            params![graph, head, freshness],
            |row| row.get(0),
        )?;
        if retracted {
            return Err(invalid("OWL snapshot premises retracted; retry required"));
        }
    }
    Ok(())
}

enum SemanticValue {
    Ref(String),
    Literal(Value),
}
struct SemanticDatum {
    entity: String,
    attribute: String,
    value: SemanticValue,
}

/// Immutable semantic page: no live term IDs, and at most 64 keys.
pub struct PreparedPage {
    binding: Binding,
    start: i64,
    end: i64,
    digest: String,
    datums: Vec<SemanticDatum>,
}
/// Only successful live publication can construct this acknowledgement token.
pub struct CommittedPage {
    page: PreparedPage,
}

impl Snapshot {
    /// Read and digest one private page outside live writer admission.
    pub fn prepare_page(&self) -> Result<Option<PreparedPage>> {
        observe(Stage::Prepare, || {
            if !self.derived {
                return Err(invalid("OWL snapshot has not been derived"));
            }
            pages::check_budget(&self.scratch)?;
            let job = pages::load(&self.scratch)?;
            prepare_page(self, &job, job.apply_cursor)
        })
    }
    /// Persist only a committed page's exact cursor, after the live guard drops.
    pub fn reconcile_page(&mut self, committed: Result<CommittedPage>) -> Result<usize> {
        let committed = committed.inspect_err(|error| self.record_invalidation(error))?;
        observe(Stage::Acknowledge, || {
            pages::checkpoint("after_live_commit")?;
            let page = committed.page;
            let mut job = pages::load(&self.scratch)?;
            let current = prepare_page(self, &job, job.apply_cursor)?
                .ok_or_else(|| invalid("OWL acknowledgement cursor changed"))?;
            if current.binding != page.binding
                || current.start != page.start
                || current.end != page.end
                || current.digest != page.digest
                || current.datums.len() != page.datums.len()
            {
                return Err(invalid(
                    "OWL acknowledgement generation/page/cursor changed",
                ));
            }
            // No deadline renewal or new assertions here. Even a commit whose
            // acknowledgement crossed the deadline must retain its receipt.
            job.apply_cursor = page.end;
            job.phase = "APPLYING".into();
            pages::save(&self.scratch, &job)?;
            Ok(page.datums.len())
        })
    }
    pub(super) fn record_invalidation(&self, error: &Error) {
        if let Error::InvalidValue(reason) = error
            && matches!(
                reason.as_str(),
                "OWL snapshot identity or ontology changed"
                    | "OWL snapshot premises retracted; retry required"
            )
            && let Ok(mut job) = pages::load(&self.scratch)
        {
            job.phase = "INVALIDATED".into();
            job.failure = Some(reason.clone());
            let _ = pages::save(&self.scratch, &job);
        }
    }
}

pub(super) fn prepare_page(
    snapshot: &Snapshot,
    job: &pages::Job,
    cursor: i64,
) -> Result<Option<PreparedPage>> {
    if cursor < 0 {
        return Err(invalid("OWL invalid cumulative cursor"));
    }
    let binding = Binding::capture(snapshot, job)?;
    let rows = pages::read_page(
        &snapshot.scratch,
        "owl_page_all",
        cursor,
        APPLY_BATCH,
        &snapshot.timestamp,
    )?;
    if rows.is_empty() {
        return Ok(None);
    }
    let end = rows.last().expect("nonempty page").0;
    let mut datums = Vec::with_capacity(rows.len());
    let mut digest = sha2::Sha256::new();
    use sha2::Digest;
    for (_, datum) in rows {
        let entity = snapshot.scratch.resolve(datum.entity)?;
        let attribute = snapshot.scratch.resolve(datum.attribute)?;
        let (value, encoded) = match datum.value {
            Value::Ref(id) => {
                let text = snapshot.scratch.resolve(id)?;
                let encoded = format!("ref:{text}").into_bytes();
                (SemanticValue::Ref(text), encoded)
            }
            value => {
                let encoded = value.to_bytes();
                (SemanticValue::Literal(value), encoded)
            }
        };
        for field in [entity.as_bytes(), attribute.as_bytes(), encoded.as_slice()] {
            digest.update((field.len() as u64).to_le_bytes());
            digest.update(field);
        }
        datums.push(SemanticDatum {
            entity,
            attribute,
            value,
        });
    }
    Ok(Some(PreparedPage {
        binding,
        start: cursor,
        end,
        digest: format!("{:x}", digest.finalize()),
        datums,
    }))
}

impl PreparedPage {
    pub(super) fn receipt(&self) -> String {
        format!("{}:{}:{}", self.binding.generation, self.start, self.end)
    }
    pub(super) fn end(&self) -> i64 {
        self.end
    }
    pub(super) fn digest(&self) -> &str {
        &self.digest
    }
    pub(super) fn len(&self) -> usize {
        self.datums.len()
    }
    /// Normal live guards and atomic receipt transaction; no scratch access.
    pub fn apply(self, live: &mut Store) -> Result<CommittedPage> {
        crate::write_observation::page(
            &self.binding.generation,
            self.start,
            self.end,
            self.datums.len(),
        );
        observe(Stage::Validate, || self.binding.validate(live))?;
        let batch = observe(Stage::Remap, || {
            self.datums
                .iter()
                .map(|datum| {
                    Ok(Datum {
                        entity: live.intern(&datum.entity)?,
                        attribute: live.intern(&datum.attribute)?,
                        value: match &datum.value {
                            SemanticValue::Ref(text) => Value::Ref(live.intern(text)?),
                            SemanticValue::Literal(value) => value.clone(),
                        },
                        valid_from: self.binding.timestamp.clone(),
                        valid_to: None,
                        op: Op::Assert,
                    })
                })
                .collect::<Result<Vec<_>>>()
        })?;
        let graph = live.intern(ROOT_INFERRED_GRAPH_IRI)?;
        pages::check_deadline(self.binding.deadline)?;
        observe(Stage::Transaction, || {
            live.transact_owl_page(
                &batch,
                &self.binding.timestamp,
                graph,
                (&self.receipt(), &self.digest),
                "owl:materialize",
            )
        })?;
        Ok(CommittedPage { page: self })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expired_prepared_binding_refuses_before_any_live_mutation() {
        let mut live = super::super::pages_tests::corpus(2);
        let mut snapshot = Snapshot::capture(
            &live,
            Snapshot::temporary_store().unwrap(),
            "2026-01-01T00:00:00Z",
        )
        .unwrap();
        snapshot.derive().unwrap();
        let deadline = pages::load(&snapshot.scratch).unwrap().deadline;
        let mut page = snapshot.prepare_page().unwrap().unwrap();
        page.binding.deadline = 0;
        let head = live.transaction_head().unwrap();
        let error = snapshot.reconcile_page(page.apply(&mut live)).unwrap_err();
        assert!(error.to_string().contains("deadline"));
        assert_eq!(live.transaction_head().unwrap(), head);
        let job = pages::load(&snapshot.scratch).unwrap();
        assert_eq!(job.apply_cursor, 0);
        assert_eq!(job.deadline, deadline);
    }
}
