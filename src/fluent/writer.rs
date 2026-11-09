//! Source-stage native append builder. Transport adapters remain unadmitted.
use super::{Binding, Log, Observation, Order, Policy, Scalar, bindings, invalid};
use crate::store::Datum;
use crate::types::{Op, Value};
use crate::{Result, Store};
use oxrdf::{Literal, NamedNode, Term};
use std::str::FromStr;

/// Callers declare a value and domain time, never a writer, tx or recording time.
#[derive(Debug, Clone)]
pub(crate) struct Append {
    pub fluent: String,
    pub event: String,
    pub subject: String,
    pub predicate: String,
    pub property_shape: String,
    pub as_of: String,
    pub value: Scalar,
}

pub(crate) enum Prepared {
    Write(Vec<Datum>),
    Replay(i64),
}

pub(super) fn iri(value: &str) -> Result<Term> {
    Ok(NamedNode::new(value)
        .map_err(|_| invalid("native identity must be an IRI"))?
        .into())
}

pub(super) fn literal(value: &str) -> Term {
    Literal::new_simple_literal(value).into()
}

pub(super) fn datum(
    store: &Store,
    subject: &str,
    predicate: &str,
    value: &Term,
    at: &str,
) -> Result<Datum> {
    Ok(Datum {
        entity: store.intern(subject)?,
        attribute: store.intern(predicate)?,
        value: crate::rdf::term_to_value(store, value)?,
        valid_from: at.into(),
        valid_to: None,
        op: Op::Assert,
    })
}

pub(super) fn policy_name(policy: Policy) -> &'static str {
    match policy {
        Policy::Latest => "latest",
        Policy::Max => "max",
        Policy::Sum => "sum",
        Policy::Set => "set",
    }
}

/// Closed set of materializers admitted to the native transactional pipeline.
#[derive(Clone, Copy)]
pub(crate) enum NativeWrite<'a> {
    Append(&'a Append),
    Register(&'a super::registration::Register),
    PendingConfig(
        &'a super::pending_config::Pending,
        &'a super::pending_config::Approval,
    ),
    Import(&'a super::source_witness::Proof),
    ImportBatch(&'a [super::source_witness::Proof]),
}

impl NativeWrite<'_> {
    pub(crate) fn materialize(
        self,
        store: &Store,
        graph: i64,
        tx: i64,
        timestamp: &str,
    ) -> Result<Prepared> {
        match self {
            Self::Append(request) => request.materialize(store, graph, tx, timestamp),
            Self::Register(request) => request.materialize(store, graph, tx, timestamp),
            Self::PendingConfig(request, approval) => {
                request.materialize(approval, store, graph, tx, timestamp)
            }
            Self::Import(proof) => proof.materialize(store, graph, timestamp),
            Self::ImportBatch(proofs) => {
                super::source_witness::materialize_batch(proofs, store, graph, timestamp)
            }
        }
    }

    pub(crate) fn persist_origin(self, store: &Store, graph: i64, tx: i64) -> Result<()> {
        match self {
            Self::Import(proof) => proof.persist(store, graph, tx)?,
            Self::ImportBatch(proofs) => {
                super::source_witness::persist_batch(proofs, store, graph, tx)?;
            }
            Self::PendingConfig(request, approval) => {
                request.persist_receipt(store, graph, tx, approval)?;
            }
            Self::Append(_) | Self::Register(_) => {}
        }
        Ok(())
    }
}

pub(super) fn binding_for(
    store: &Store,
    graph: i64,
    subject: &str,
    predicate: &str,
    property_shape: &str,
) -> Result<Binding> {
    // A source contract on disk is not schema admission in this Store.
    if !store.list_shapes()?.iter().any(|(name, turtle, _)| {
        name == "quipu-fluents-v1" && turtle == include_str!("../../shapes/fluents-v1.ttl")
    }) {
        return Err(invalid("native fluent schema is not loaded"));
    }
    let shapes = store
        .get_combined_shapes()?
        .ok_or_else(|| invalid("missing fluent shapes"))?;
    let graph_iri = store.graph_iri_of(graph);
    let facts = store.current_facts_in_graph(graph)?;
    let data = String::from_utf8(crate::rdf::serialize_facts(
        store,
        &facts,
        oxrdfio::RdfFormat::NTriples,
    )?)
    .map_err(|_| invalid("invalid graph serialization"))?;
    let mut selected = bindings(&shapes, &data, &graph_iri, subject)?
        .into_iter()
        .filter(|b| b.declaration.predicate == predicate && b.declaration.shape == property_shape);
    let binding = selected
        .next()
        .ok_or_else(|| invalid("property shape does not select this subject and graph"))?;
    if selected.next().is_some() {
        return Err(invalid("ambiguous native fluent binding"));
    }
    Ok(binding)
}

impl Append {
    /// Called only inside normal staging, after native transaction allocation.
    pub(crate) fn materialize(
        &self,
        store: &Store,
        graph: i64,
        tx: i64,
        timestamp: &str,
    ) -> Result<Prepared> {
        let identity = crate::transaction_auth::current_identity()
            .ok_or_else(|| invalid("native append requires authenticated writer evidence"))?;
        iri(&identity.principal)?;
        iri(&self.fluent)?;
        iri(&self.event)?;
        if self.fluent == self.event {
            return Err(invalid("fluent and event identities must differ"));
        }
        let graph_iri = store.graph_iri_of(graph);
        let binding = binding_for(
            store,
            graph,
            &self.subject,
            &self.predicate,
            &self.property_shape,
        )?;
        store.check_fluent_graph_binding(&self.fluent, &graph_iri)?;
        store.check_fluent_event_graph(&self.event, graph)?;
        let value = match (binding.declaration.policy, &self.value) {
            (Policy::Latest | Policy::Set, Scalar::Integer(n)) => Scalar::Term(
                Literal::new_typed_literal(n.to_string(), oxrdf::vocab::xsd::INTEGER).to_string(),
            ),
            _ => self.value.clone(),
        };
        let observation = Observation {
            fluent: self.fluent.clone(),
            order: Order {
                tx: u64::try_from(tx).map_err(|_| invalid("invalid native transaction"))?,
                writer: identity.principal,
                event: self.event.clone(),
            },
            as_of: self.as_of.clone(),
            recorded_on: timestamp.into(),
            value,
        };
        let mut log = read_log(store, graph, &self.fluent, binding.declaration.policy)?;
        let descriptor = descriptor(store, &binding, &self.fluent, timestamp)?;
        if let Some(entity) = store.lookup(&self.fluent)? {
            let prior = store.entity_facts_in_graph(entity, graph)?;
            // Immutable declaration, including unknown extra fields: never reinterpret.
            if !prior.is_empty()
                && (prior.len() != descriptor.len()
                    || descriptor.iter().any(|d| {
                        !prior
                            .iter()
                            .any(|f| f.attribute == d.attribute && f.value == d.value)
                    }))
            {
                return Err(invalid("native fluent declaration changed"));
            }
        }
        if let Some(prior) = log
            .observations(&self.fluent)
            .into_iter()
            .find(|o| o.order.event == self.event)
        {
            if let Some(event) = store.lookup(&self.event)?
                && store.fluent_origin_receipt(graph, event)?.is_some()
            {
                return Err(invalid("native append cannot replay a transported origin"));
            }
            // Replay compares caller fields plus the authenticated source, never
            // the fresh local tx/recording clock allocated for this attempt.
            if prior.order.writer != observation.order.writer
                || prior.as_of != observation.as_of
                || prior.value != observation.value
            {
                return Err(invalid(
                    "native event identity reused with different content",
                ));
            }
            return Ok(Prepared::Replay(
                i64::try_from(prior.order.tx)
                    .map_err(|_| invalid("invalid native replay transaction"))?,
            ));
        }
        if let Some(event) = store.lookup(&self.event)?
            && !store.entity_facts_in_graph(event, graph)?.is_empty()
        {
            return Err(invalid("native event identity already exists"));
        }
        log.append(observation.clone())?;
        log.current(&self.fluent, binding.declaration.policy)?;
        let qp = crate::namespace::QUIPU;
        let mut out = descriptor;
        for (predicate, value) in [
            (
                crate::namespace::RDF_TYPE.to_owned(),
                iri(&format!("{qp}FluentObservation"))?,
            ),
            (format!("{qp}forFluent"), iri(&self.fluent)?),
            (
                format!("{qp}originTx"),
                Literal::new_typed_literal(tx.to_string(), oxrdf::vocab::xsd::INTEGER).into(),
            ),
            (format!("{qp}writer"), iri(&observation.order.writer)?),
            (
                format!("{qp}asOf"),
                Literal::new_typed_literal(&self.as_of, oxrdf::vocab::xsd::DATE_TIME).into(),
            ),
            (
                format!("{qp}recordedOn"),
                Literal::new_typed_literal(timestamp, oxrdf::vocab::xsd::DATE_TIME).into(),
            ),
            (
                format!("{qp}observationValue"),
                match &self.value {
                    Scalar::Integer(n) => {
                        Literal::new_typed_literal(n.to_string(), oxrdf::vocab::xsd::INTEGER).into()
                    }
                    Scalar::Term(text) => {
                        Term::from_str(text).map_err(|_| invalid("invalid observation term"))?
                    }
                },
            ),
        ] {
            out.push(datum(store, &self.event, &predicate, &value, timestamp)?);
        }
        Ok(Prepared::Write(out))
    }
}

pub(super) fn descriptor(
    store: &Store,
    binding: &Binding,
    fluent: &str,
    at: &str,
) -> Result<Vec<Datum>> {
    let qp = crate::namespace::QUIPU;
    [
        (
            crate::namespace::RDF_TYPE.to_owned(),
            iri(&format!("{qp}Fluent"))?,
        ),
        (format!("{qp}slotSubject"), iri(&binding.subject)?),
        (
            format!("{qp}slotPredicate"),
            iri(&binding.declaration.predicate)?,
        ),
        (format!("{qp}inGraph"), iri(&binding.graph)?),
        (
            format!("{qp}propertyShape"),
            iri(&binding.declaration.shape)?,
        ),
        (
            format!("{qp}currentValuePolicy"),
            literal(policy_name(binding.declaration.policy)),
        ),
    ]
    .into_iter()
    .map(|(p, v)| datum(store, fluent, &p, &v, at))
    .collect()
}

/// Read only the selected graph's native RDF event fields. Missing or ambiguous
/// fields refuse rather than producing an apparently empty/current value.
pub(crate) fn read_log(store: &Store, graph: i64, fluent: &str, policy: Policy) -> Result<Log> {
    let qp = crate::namespace::QUIPU;
    let Some(fluent_id) = store.lookup(fluent)? else {
        return Ok(Log::default());
    };
    let Some(for_fluent) = store.lookup(&format!("{qp}forFluent"))? else {
        return Ok(Log::default());
    };
    let (Some(type_id), Some(observation_type)) = (
        store.lookup(crate::namespace::RDF_TYPE)?,
        store.lookup(&format!("{qp}FluentObservation"))?,
    ) else {
        return Ok(Log::default());
    };
    let facts = store.current_facts_in_graph(graph)?;
    // Only native admission can create this protected type. An authenticated
    // caller's source label and untyped predicate lookalikes grant no origin.
    let admitted: std::collections::BTreeSet<_> = facts
        .iter()
        .filter(|f| f.attribute == type_id && f.value == Value::Ref(observation_type))
        .map(|f| f.entity)
        .collect();
    let events: std::collections::BTreeSet<_> = facts
        .iter()
        .filter(|f| f.attribute == for_fluent && f.value == Value::Ref(fluent_id))
        .filter(|f| admitted.contains(&f.entity))
        .map(|f| f.entity)
        .collect();
    let mut log = Log::default();
    for event in events {
        let one = |name: &str| -> Result<Value> {
            let id = store
                .lookup(&format!("{qp}{name}"))?
                .ok_or_else(|| invalid("missing event field"))?;
            let values: Vec<_> = facts
                .iter()
                .filter(|f| f.entity == event && f.attribute == id)
                .collect();
            if values.len() != 1 {
                return Err(invalid("missing or ambiguous event field"));
            }
            Ok(values[0].value.clone())
        };
        let time = |name| -> Result<String> {
            match one(name)? {
                Value::Typed { lexical, datatype }
                    if datatype == oxrdf::vocab::xsd::DATE_TIME.as_str() =>
                {
                    Ok(lexical)
                }
                _ => Err(invalid("event time is not xsd:dateTime")),
            }
        };
        let value = one("observationValue")?;
        let value = match policy {
            Policy::Max | Policy::Sum => match value {
                Value::Int(n) => Scalar::Integer(n),
                _ => return Err(invalid("numeric event is not a signed integer")),
            },
            Policy::Latest | Policy::Set => {
                Scalar::Term(crate::rdf::value_to_term(store, &value)?.to_string())
            }
        };
        let writer = match one("writer")? {
            Value::Ref(id) => store.resolve(id)?,
            _ => return Err(invalid("event writer is not an IRI")),
        };
        let tx = match one("originTx")? {
            Value::Int(tx) if tx > 0 => {
                u64::try_from(tx).map_err(|_| invalid("invalid native origin"))?
            }
            _ => return Err(invalid("event origin transaction is not positive")),
        };
        let observation = Observation {
            fluent: fluent.into(),
            order: Order {
                tx,
                writer,
                event: store.resolve(event)?,
            },
            as_of: time("asOf")?,
            recorded_on: time("recordedOn")?,
            value,
        };
        let origin = if let Some(admission) =
            super::source_witness::imported_origin(store, graph, event, &observation)?
        {
            admission
        } else {
            let origin = i64::try_from(tx).map_err(|_| invalid("invalid native origin"))?;
            let provenance = store
                .transaction_auth(origin)?
                .ok_or_else(|| invalid("event lacks authenticated native origin"))?;
            let transaction = store
                .get_transaction(origin)?
                .ok_or_else(|| invalid("event native origin missing"))?;
            if provenance.principal != observation.order.writer
                || transaction.source.as_deref() != Some("quipu:fluent")
            {
                return Err(invalid("event native origin does not match its content"));
            }
            origin
        };
        if facts
            .iter()
            .filter(|f| f.entity == event)
            .any(|f| f.tx != origin)
        {
            return Err(invalid(
                "event facts do not match their admission transaction",
            ));
        }
        log.append(observation)?;
    }
    Ok(log)
}

#[cfg(test)]
include!("writer_tests.rs");
