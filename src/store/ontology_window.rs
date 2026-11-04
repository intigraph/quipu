//! Bounded cleanup of an explicitly selected OWL materialization window.
//!
//! OWL's source is shared by all ontologies. A named ontology and a transaction
//! interval are an operator-selected rollback receipt, not causal attribution.
//! Each call closes at most 5,000 selected statements via the normal write path.
use super::{
    Datum, Store, inferred,
    source_claims::{ClaimWindow, WindowGuard},
};
use crate::{Error, Op, Result, Value};
use rusqlite::params;
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};

const BATCH: usize = 5_000;
const SOURCE: &str = "owl:materialize";

#[cfg(test)]
#[path = "ontology_window_tests.rs"]
mod tests;

struct Plan {
    guard: WindowGuard,
    graph: Option<i64>,
    remaining: i64,
    datums: Vec<Datum>,
    token: String,
}

impl Store {
    fn ontology_window_plan(&self, name: &str, window: ClaimWindow) -> Result<Plan> {
        if name.trim().is_empty() || window.after < 0 || window.through <= window.after {
            return Err(Error::InvalidValue(
                "name and a nonempty (after_tx, through_tx] window are required".into(),
            ));
        }
        let head = self.latest_tx_id()?;
        if window.through > head {
            return Err(Error::InvalidValue(
                "through_tx exceeds the current transaction head".into(),
            ));
        }
        let ontology = self
            .list_ontologies()?
            .into_iter()
            .find(|o| o.0 == name)
            .ok_or_else(|| Error::InvalidValue("named ontology is not currently loaded".into()))?;
        let graph = self.lookup(inferred::ROOT_INFERRED_GRAPH_IRI)?;
        let (remaining, datums) = if let Some(graph) = graph {
            let remaining = self.conn.query_row(
                "SELECT count(*) FROM transactions t JOIN facts f ON f.tx=t.id WHERE t.source=?1 AND f.tx>?2 AND f.tx<=?3 AND +f.g=?4 AND f.op=1 AND f.valid_to IS NULL",
                params![SOURCE, window.after, window.through, graph], |r| r.get(0),
            )?;
            // LIMIT bounds allocation even for a multi-million-row interval.
            // Physical duplicate claims are collapsed below before auditing.
            let mut stmt = self.conn.prepare(
                "SELECT f.e,f.a,f.v,f.valid_from FROM transactions t JOIN facts f ON f.tx=t.id WHERE t.source=?1 AND f.tx>?2 AND f.tx<=?3 AND +f.g=?4 AND f.op=1 AND f.valid_to IS NULL ORDER BY f.tx,f.e,f.a,f.v LIMIT ?5",
            )?;
            let rows = stmt.query_map(
                params![SOURCE, window.after, window.through, graph, BATCH as i64],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                },
            )?;
            let mut datums: Vec<Datum> = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for row in rows {
                let (entity, attribute, bytes, valid_from) = row?;
                let value = Value::from_bytes(&bytes)?;
                if seen.insert((entity, attribute, value.term_key())) {
                    datums.push(Datum {
                        entity,
                        attribute,
                        value,
                        valid_from,
                        valid_to: None,
                        op: Op::Retract,
                    });
                }
            }
            (remaining, datums)
        } else {
            (0, Vec::new())
        };
        let batch: Vec<_> = datums
            .iter()
            .map(|d| json!([d.entity, d.attribute, d.value.to_bytes(), d.valid_from]))
            .collect();
        let contract = json!([
            "ontology-window/v1",
            self.store_id()?,
            head,
            ontology,
            inferred::ROOT_INFERRED_GRAPH_IRI,
            SOURCE,
            window.after,
            window.through,
            remaining,
            batch
        ]);
        let token = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&contract).map_err(|e| Error::Serialization(e.to_string()))?
            )
        );
        Ok(Plan {
            guard: WindowGuard { head, ontology },
            graph,
            remaining,
            datums,
            token,
        })
    }

    /// Preview or apply one bounded materialization-window cleanup batch.
    /// `expect` must be copied from a fresh preview. Registry removal remains
    /// the separate existing `remove` action, after remaining reaches zero.
    pub fn retract_ontology_window(
        &mut self,
        name: &str,
        after_tx: i64,
        through_tx: i64,
        expect: Option<&str>,
        timestamp: &str,
        actor: Option<&str>,
    ) -> Result<Json> {
        let window = ClaimWindow {
            after: after_tx,
            through: through_tx,
        };
        let plan = self.ontology_window_plan(name, window)?;
        let mut tx = None;
        if let Some(expect) = expect {
            if expect != plan.token {
                return Err(Error::InvalidValue(
                    "expect does not match the current ontology window plan; preview again".into(),
                ));
            }
            if !plan.datums.is_empty() {
                tx = Some(self.transact_to_graph_window_checked(
                    &plan.datums,
                    timestamp,
                    actor,
                    Some(inferred::OWL_UNLOAD_SOURCE),
                    plan.graph.expect("a nonempty plan has a graph"),
                    Some(SOURCE),
                    &[],
                    None,
                    Some(window),
                    Some(&plan.guard),
                )?);
            }
        }
        let after = self.ontology_window_plan(name, window)?;
        Ok(json!({"action":"retract_materialization", "name":name,
            "graph":inferred::ROOT_INFERRED_GRAPH_IRI, "source":SOURCE,
            "repair_source":inferred::OWL_UNLOAD_SOURCE,
            "after_tx":after_tx, "through_tx":through_tx,
            "planned":plan.remaining, "batch_statements":plan.datums.len(),
            "expect":after.token, "applied":expect.is_some(), "tx_id":tx,
            "retracted":plan.remaining-after.remaining, "remaining":after.remaining,
            "window_complete":after.remaining==0, "ontology_removed":false}))
    }
}
