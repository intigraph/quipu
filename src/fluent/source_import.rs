//! Closed native event import with locally admitted primitive declarations.
use super::*;
use crate::fluent::{Scalar, writer::Prepared};
use oxrdf::{Literal, Term};
use std::str::FromStr;

impl Proof {
    pub(crate) fn materialize(&self, store: &Store, graph: i64, at: &str) -> Result<Prepared> {
        let identity = crate::transaction_auth::current_identity()
            .ok_or_else(|| invalid("native fluent import requires authenticated receiver"))?;
        writer::iri(&identity.principal)?;
        // Called after normal begin() holds the writer reservation. A previous
        // proof result is never a permit; authority is read again here.
        self.verify_governed_now(store)?;
        let slot = &self.body.slot;
        if store.graph_iri_of(graph) != slot.graph || profile(store)? != self.body.profile_sha256 {
            return Err(invalid(
                "source proof does not match receiver graph/profile",
            ));
        }
        let binding = writer::binding_for(
            store,
            graph,
            &slot.subject,
            &slot.predicate,
            &slot.property_shape,
        )?;
        if binding.declaration.policy != slot.policy {
            return Err(invalid(
                "source proof policy differs from receiver declaration",
            ));
        }
        // A new primitive descriptor is a LOCAL adoption of the source proof's
        // exact Slot through the receiver's loaded shapes and normal gates.
        // It retains importer authentication, not a fabricated source tx.
        // Existing records must match exactly; observables cannot be rebound.
        let (_, mut log) = native_registry::load(store, graph)?;
        let rows = match store.lookup(&slot.fluent)? {
            Some(entity) => store.entity_facts_in_graph(entity, graph)?,
            None => Vec::new(),
        };
        let descriptor = writer::descriptor(store, &binding, &slot.fluent, at)?;
        if !rows.is_empty()
            && (rows.len() != descriptor.len()
                || descriptor.iter().any(|d| {
                    !rows
                        .iter()
                        .any(|f| f.attribute == d.attribute && f.value == d.value)
                }))
        {
            return Err(invalid(
                "source proof differs from admitted receiver descriptor",
            ));
        }
        store.check_fluent_graph_binding(&slot.fluent, &slot.graph)?;
        let observation = &self.body.observation;
        store.check_fluent_event_graph(&observation.order.event, graph)?;
        if let Some(event) = store.lookup(&observation.order.event)? {
            if let Some((bytes, tx)) = store.fluent_origin_receipt(graph, event)? {
                if bytes != self.encode()? {
                    return Err(invalid("imported event proof identity/content changed"));
                }
                if !log.observations(&slot.fluent).contains(&observation) {
                    return Err(invalid("imported event receipt lacks matching record"));
                }
                return Ok(Prepared::Replay(tx));
            }
            if !store.entity_facts_in_graph(event, graph)?.is_empty() {
                return Err(invalid("imported event identity is already occupied"));
            }
        }
        log.append(observation.clone())?;
        log.current(&slot.fluent, slot.policy)?;
        let mut datums = if rows.is_empty() {
            descriptor
        } else {
            Vec::new()
        };
        datums.extend(self.datums(store, at)?);
        Ok(Prepared::Write(datums))
    }

    fn datums(&self, store: &Store, at: &str) -> Result<Vec<crate::store::Datum>> {
        let slot = &self.body.slot;
        let observation = &self.body.observation;
        let qp = crate::namespace::QUIPU_FLUENT;
        let value = match &observation.value {
            Scalar::Integer(n) => {
                Literal::new_typed_literal(n.to_string(), oxrdf::vocab::xsd::INTEGER).into()
            }
            Scalar::Term(text) => {
                Term::from_str(text).map_err(|_| invalid("invalid imported value"))?
            }
        };
        let fields = [
            (
                crate::namespace::RDF_TYPE.to_string(),
                writer::iri(&format!("{qp}FluentObservation"))?,
            ),
            (format!("{qp}forFluent"), writer::iri(&slot.fluent)?),
            (
                format!("{qp}originTx"),
                Literal::new_typed_literal(
                    observation.order.tx.to_string(),
                    oxrdf::vocab::xsd::INTEGER,
                )
                .into(),
            ),
            (
                format!("{qp}writer"),
                writer::iri(&observation.order.writer)?,
            ),
            (
                format!("{qp}asOf"),
                Literal::new_typed_literal(&observation.as_of, oxrdf::vocab::xsd::DATE_TIME).into(),
            ),
            (
                format!("{qp}recordedOn"),
                Literal::new_typed_literal(&observation.recorded_on, oxrdf::vocab::xsd::DATE_TIME)
                    .into(),
            ),
            (format!("{qp}observationValue"), value),
        ];
        fields
            .into_iter()
            .map(|(p, v)| writer::datum(store, &observation.order.event, &p, &v, at))
            .collect()
    }

    fn encode(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(self).map_err(|_| invalid("source proof encoding"))?;
        Self::decode(&bytes)?;
        Ok(bytes)
    }

    pub(crate) fn persist(&self, store: &Store, graph: i64, tx: i64) -> Result<()> {
        let event = store
            .lookup(&self.body.observation.order.event)?
            .ok_or_else(|| invalid("missing imported event"))?;
        store.persist_fluent_origin(graph, event, tx, &self.encode()?)
    }

    /// Relay the originally admitted proof, after verifying its local receipt.
    /// This does not turn the importer into a source witness.
    pub(crate) fn retained(store: &Store, graph: i64, event: &str) -> Result<Self> {
        let id = store
            .lookup(event)?
            .ok_or_else(|| invalid("missing relay event"))?;
        let (bytes, _) = store
            .fluent_origin_receipt(graph, id)?
            .ok_or_else(|| invalid("relay event has no admitted source proof"))?;
        let proof = Self::decode(&bytes)?;
        let log = writer::read_log(
            store,
            graph,
            &proof.body.slot.fluent,
            proof.body.slot.policy,
        )?;
        if !log
            .observations(&proof.body.slot.fluent)
            .contains(&&proof.body.observation)
        {
            return Err(invalid("relay receipt has no matching native observation"));
        }
        Ok(proof)
    }
}

/// Historical authority comes only from the immutable local admission receipt,
/// never from a sender-selected transaction/time or a generic bundle claim.
pub(in crate::fluent) fn imported_origin(
    store: &Store,
    graph: i64,
    event: i64,
    observation: &Observation,
) -> Result<Option<i64>> {
    let Some((bytes, tx)) = store.fluent_origin_receipt(graph, event)? else {
        return Ok(None);
    };
    let proof = Proof::decode(&bytes)?;
    let transaction = store
        .get_transaction(tx)?
        .ok_or_else(|| invalid("missing receiver admission transaction"))?;
    let auth = store
        .transaction_auth(tx)?
        .ok_or_else(|| invalid("missing receiver admission authentication"))?;
    writer::iri(&auth.principal)?;
    if transaction.source.as_deref() != Some("quipu:fluent")
        || proof.body.slot.graph != store.graph_iri_of(graph)
        || &proof.body.observation != observation
    {
        return Err(invalid(
            "imported event differs from its immutable origin receipt",
        ));
    }
    proof.verify_governed_at(
        store,
        &crate::governance::verifier_registry::Witness {
            tx: Some(tx),
            at: transaction.timestamp,
        },
    )?;
    Ok(Some(tx))
}
