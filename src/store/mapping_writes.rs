//! C1's protected master-side lifecycle. The accepted decision is explicit
//! data, under the existing graph write gates; row authors are not credentials.
//! Authenticated request evidence remains in `transaction_auth`, independently
//! of the declared decision author recorded in these facts.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{OptionalExtension, params};

use crate::shape_mapping::catalog::MappingReceipt;
use crate::shape_mapping::facts::{self, MappingStatus, Settlement};
use crate::shape_mapping::{RegistryKind, ShapeMapping, ShapePin};
use crate::{Error, Op, Result};

use super::mapping_atomic::Preparation;
use super::{Datum, Store};

const OPERATIONS: &str = "urn:quipu:graph:operations";

type StoredStateRow = (Vec<u8>, String, String, String, String, String, i64, String);

struct State {
    mapping: ShapeMapping,
    operations: String,
    status: MappingStatus,
    actor: String,
    authority: String,
    timestamp: String,
    tx: i64,
    record: String,
}

/// A decided mapping a registry change must flag: its state, the closed pin,
/// and its mapping and operations graph ids.
type FlaggedPin = (State, ShapePin, i64, i64);

/// The pins a registry change closes, the decided mappings it flags, and how
/// many closed versions predate digests.
type RegistryPlan = (Vec<ShapePin>, Vec<FlaggedPin>, usize);

fn invalid(message: &str) -> Error {
    Error::InvalidValue(format!("shape mapping: {message}"))
}

fn encoded<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|error| Error::Serialization(error.to_string()))
}

fn parse(store: &Store, rdf: &str, timestamp: &str) -> Result<Vec<Datum>> {
    crate::rdf::parse_rdf(
        store,
        rdf.as_bytes(),
        oxrdfio::RdfFormat::NTriples,
        None,
        timestamp,
    )
}

impl Store {
    pub(super) fn migrate_shape_mapping_state(conn: &rusqlite::Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS shape_mapping_state (
                mapping TEXT NOT NULL, tx INTEGER NOT NULL, digest TEXT NOT NULL, model BLOB NOT NULL,
                operations TEXT NOT NULL, status TEXT NOT NULL CHECK(status IN ('decided','flagged','retired')),
                actor TEXT NOT NULL, authority TEXT NOT NULL, timestamp TEXT NOT NULL,
                record TEXT NOT NULL, valid_to TEXT, PRIMARY KEY(mapping,tx));
             CREATE UNIQUE INDEX IF NOT EXISTS shape_mapping_one_current
                ON shape_mapping_state(mapping) WHERE valid_to IS NULL;
             CREATE INDEX IF NOT EXISTS shape_mapping_digest_history
                ON shape_mapping_state(mapping,digest,status,tx);
             CREATE TABLE IF NOT EXISTS shape_mapping_pins (
                mapping TEXT NOT NULL, registry TEXT NOT NULL, name TEXT NOT NULL,
                valid_from TEXT NOT NULL, digest TEXT NOT NULL,
                PRIMARY KEY(mapping,registry,name,valid_from,digest));
             CREATE INDEX IF NOT EXISTS shape_mapping_pinned_version
                ON shape_mapping_pins(registry,name,valid_from,digest);"
        )?;
        Ok(())
    }

    fn mapping_state(&self, id: &str) -> Result<Option<State>> {
        let row: Option<StoredStateRow> = self
            .conn
            .query_row(
                "SELECT model,operations,status,actor,authority,timestamp,tx,record
             FROM shape_mapping_state WHERE mapping=?1 ORDER BY tx DESC LIMIT 1",
                [id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .optional()?;
        row.map(
            |(model, operations, status, actor, authority, timestamp, tx, record)| {
                let mapping: ShapeMapping = serde_json::from_slice(&model)
                    .map_err(|error| invalid(&format!("stored model unreadable: {error}")))?;
                mapping.validate(true)?;
                let status = match status.as_str() {
                    "decided" => MappingStatus::Decided,
                    "flagged" => MappingStatus::Flagged,
                    "retired" => MappingStatus::Retired,
                    _ => return Err(invalid("stored status unreadable")),
                };
                Ok(State {
                    mapping,
                    operations,
                    status,
                    actor,
                    authority,
                    timestamp,
                    tx,
                    record,
                })
            },
        )
        .transpose()
    }

    fn mapping_graph(&self, iri: &str) -> Result<i64> {
        if iri == crate::schema::ROOT_GRAPH_IRI {
            return Err(invalid(
                "mapping and operations need explicit named committed graphs",
            ));
        }
        self.committed_graph_id(iri)?
            .ok_or_else(|| invalid("graph is not registered committed scope"))
    }

    fn mapping_receipt(&self, state: &State, replayed: bool) -> Result<MappingReceipt> {
        let mut writable = state.mapping.assert_writable()?;
        if state.status != MappingStatus::Decided {
            writable.values_mut().for_each(|value| *value = false);
        } else {
            // Inverse uniqueness is scoped over every active decided mapping,
            // not merely the rows in the mapping being returned.
            let mut inverse: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
            for row in &state.mapping.rows {
                inverse
                    .entry(row.object_id.clone())
                    .or_default()
                    .insert((row.subject_id.clone(), row.predicate_id.clone()));
            }
            let mut statement = self.conn.prepare(
                "SELECT mapping FROM shape_mapping_state WHERE valid_to IS NULL AND status='decided' AND mapping<>?1 ORDER BY mapping"
            )?;
            let ids = statement
                .query_map([&state.mapping.id], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for id in ids {
                let other = self
                    .mapping_state(&id)?
                    .ok_or_else(|| invalid("mapping catalog changed"))?;
                if state.mapping.maps_to.is_disjoint(&other.mapping.maps_to) {
                    continue;
                }
                for row in &other.mapping.rows {
                    if let Some(sources) = inverse.get_mut(&row.object_id) {
                        sources.insert((row.subject_id.clone(), row.predicate_id.clone()));
                    }
                }
            }
            for (target, allowed) in &mut writable {
                *allowed &= inverse
                    .get(target)
                    .is_some_and(|sources| sources.len() == 1);
            }
        }
        Ok(MappingReceipt {
            mapping: state.mapping.id.clone(),
            digest: state.mapping.digest()?,
            record: state.record.clone(),
            tx: state.tx,
            status: state.status.as_str().into(),
            replayed,
            assert_writable: writable,
        })
    }

    /// Read one mapping's current lifecycle receipt. Database/schema failures
    /// are errors, never an empty successful catalog.
    pub fn shape_mapping_receipt(&self, id: &str) -> Result<Option<MappingReceipt>> {
        self.mapping_state(id)?
            .as_ref()
            .map(|state| self.mapping_receipt(state, false))
            .transpose()
    }

    fn save_mapping_state(&self, state: &State, timestamp: &str) -> Result<()> {
        for pin in &state.mapping.pins {
            self.conn.execute("INSERT OR IGNORE INTO shape_mapping_pins(mapping,registry,name,valid_from,digest) VALUES(?1,?2,?3,?4,?5)",
                params![state.mapping.id,pin.registry.table(),pin.name,pin.valid_from,pin.digest])?;
        }
        self.conn.execute(
            "UPDATE shape_mapping_state SET valid_to=?2 WHERE mapping=?1 AND valid_to IS NULL",
            params![state.mapping.id, timestamp],
        )?;
        self.conn.execute(
            "INSERT INTO shape_mapping_state(mapping,tx,digest,model,operations,status,actor,authority,timestamp,record,valid_to)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![state.mapping.id,state.tx,state.mapping.digest()?,encoded(&state.mapping)?,state.operations,state.status.as_str(),state.actor,state.authority,state.timestamp,state.record,
                if state.status == MappingStatus::Retired { Some(timestamp) } else { None }],
        )?;
        Ok(())
    }

    fn mapping_event(&self, state: &State, event: &str) -> Result<()> {
        let payload = serde_json::json!({"mapping":state.mapping.id,"digest":state.mapping.digest()?,"record":state.record,"status":state.status.as_str(),"tx":state.tx});
        self.conn.execute("INSERT INTO events(type,ts,subject,group_id,tx_id,payload) VALUES(?1,?2,?3,NULL,?4,?5)",
            params![event,state.timestamp,state.mapping.id,state.tx,payload.to_string()])?;
        Ok(())
    }

    /// Build validity closures from the exact generated definition's subjects.
    /// This is a graph-local indexed read, never a walk of the mapping graph.
    fn close_mapping_projection(
        &self,
        state: &State,
        graph: i64,
        timestamp: &str,
    ) -> Result<Vec<Datum>> {
        let at = Settlement {
            actor: &state.actor,
            authority: &state.authority,
            timestamp: &state.timestamp,
            tx: state.tx,
        };
        let definition = facts::definition(&state.mapping, &at)?;
        let subjects: BTreeSet<_> = parse(self, &definition, &state.timestamp)?
            .into_iter()
            .map(|datum| datum.entity)
            .collect();
        let mut datums = Vec::new();
        for subject in subjects {
            for fact in self.entity_facts_in_graph(subject, graph)? {
                datums.push(Datum {
                    entity: fact.entity,
                    attribute: fact.attribute,
                    value: fact.value,
                    valid_from: timestamp.into(),
                    valid_to: None,
                    op: Op::Retract,
                });
            }
        }
        Ok(datums)
    }

    /// Apply an explicitly reviewed definition. This records the accepted
    /// mapping decision as its authority; it never treats a source tag or a
    /// declared actor as authenticated request evidence or graph permission.
    /// Both named graphs must already exist. No graph/grant is bootstrapped.
    pub fn apply_shape_mapping(
        &mut self,
        mapping: ShapeMapping,
        expected_digest: &str,
        actor: &str,
        operations_graph: Option<&str>,
    ) -> Result<MappingReceipt> {
        mapping.validate(true)?;
        if mapping.digest()? != expected_digest
            || mapping.rows.iter().any(|row| row.author_id != actor)
        {
            return Err(invalid(
                "expected digest or explicit decision author mismatch",
            ));
        }
        let operations = operations_graph.unwrap_or(OPERATIONS).to_owned();
        self.mapping_atomic(actor, |store| {
            store.propose_shape_mapping(mapping.clone())?;
            let graph = store.mapping_graph(&mapping.graph)?;
            let operation_graph = store.mapping_graph(&operations)?;
            for target in [graph,operation_graph] {
                store.assert_write_target_allowed(target,Some(super::mapping_atomic::SOURCE))?;
                store.enforce_graph_authority(target)?;
            }
            if graph == operation_graph || mapping.maps_from.contains(&operations) || mapping.maps_to.contains(&operations) {
                return Err(invalid("operations graph must be separate from mapping/data scope"));
            }
            let old = store.mapping_state(&mapping.id)?;
            if old.is_none()
                && let Some(entity) = store.lookup(&mapping.id)?
                && !store.entity_facts_in_graph(entity, graph)?.is_empty()
            {
                return Err(invalid("mapping identity already has facts without local lifecycle admission"));
            }
            if let Some(old) = &old {
                if old.mapping.graph != mapping.graph || old.operations != operations {
                    return Err(invalid("mapping location is immutable; use a new mapping identity"));
                }
                if old.mapping.digest()? == expected_digest {
                    let mut receipt = store.mapping_receipt(old, true)?;
                    // Return the original application record even after a
                    // retirement, while reporting the current retired status.
                    let (tx, record): (i64, String) = store.conn.query_row(
                        "SELECT tx,record FROM shape_mapping_state WHERE mapping=?1 AND digest=?2 AND status='decided' ORDER BY tx LIMIT 1",
                        params![mapping.id,expected_digest], |row| Ok((row.get(0)?,row.get(1)?)),
                    )?;
                    receipt.tx = tx;
                    receipt.record = record;
                    return Ok(Preparation::Replay(receipt));
                }
            }
            let mut statement = store.conn.prepare("SELECT mapping FROM shape_mapping_state WHERE valid_to IS NULL AND status='decided' AND mapping<>?1 ORDER BY mapping")?;
            let ids = statement.query_map([&mapping.id], |row| row.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
            for id in ids {
                let other = store.mapping_state(&id)?.ok_or_else(|| invalid("mapping catalog changed"))?;
                mapping.check_other(&other.mapping)?;
            }
            Ok(Preparation::Write((mapping, operations, graph, operation_graph, old)))
        }, |store, (mapping,operations,graph,operation_graph,old), timestamp,tx| {
            let authority = facts::decision_iri(&mapping,"apply",tx)?;
            let at = Settlement { actor, authority: &authority, timestamp, tx };
            let mut datums = if let Some(old) = &old { store.close_mapping_projection(old, graph, timestamp)? } else { Vec::new() };
            datums.extend(parse(store, &facts::definition(&mapping, &at)?, timestamp)?);
            let old_digest = old.as_ref().map(|state| state.mapping.digest()).transpose()?;
            let (record,audit) = facts::record(&mapping,&at,old.as_ref().map(|state|state.status),MappingStatus::Decided,old_digest.as_deref(),None)?;
            let mut evidence = parse(store, &facts::decision(&mapping,&at,"apply")?,timestamp)?;
            evidence.extend(parse(store,&audit,timestamp)?);
            let state = State { mapping,operations,status:MappingStatus::Decided,actor:actor.into(),authority,
                timestamp:timestamp.into(),tx,record };
            store.save_mapping_state(&state,timestamp)?;
            store.mapping_event(&state,"mapping.decided")?;
            Ok((store.mapping_receipt(&state,false)?,vec![(graph,datums),(operation_graph,evidence)]))
        })
    }

    /// Retire by closing valid time. Prior declaration and correspondence
    /// assertions remain in history; the operations record remains current.
    pub fn retire_shape_mapping(
        &mut self,
        id: &str,
        expected_digest: &str,
        actor: &str,
    ) -> Result<MappingReceipt> {
        self.mapping_atomic(
            actor,
            |store| {
                let state = store
                    .mapping_state(id)?
                    .ok_or_else(|| invalid("unknown mapping"))?;
                let graph = store.mapping_graph(&state.mapping.graph)?;
                let operations = store.mapping_graph(&state.operations)?;
                for target in [graph, operations] {
                    store
                        .assert_write_target_allowed(target, Some(super::mapping_atomic::SOURCE))?;
                    store.enforce_graph_authority(target)?;
                }
                if state.mapping.digest()? != expected_digest {
                    return Err(invalid("stale retirement digest"));
                }
                if state.status == MappingStatus::Retired {
                    return Ok(Preparation::Replay(store.mapping_receipt(&state, true)?));
                }
                Ok(Preparation::Write((state, graph, operations)))
            },
            |store, (old, graph, operations), timestamp, tx| {
                let authority = facts::decision_iri(&old.mapping, "retire", tx)?;
                let at = Settlement {
                    actor,
                    authority: &authority,
                    timestamp,
                    tx,
                };
                let digest = old.mapping.digest()?;
                let (record, audit) = facts::record(
                    &old.mapping,
                    &at,
                    Some(old.status),
                    MappingStatus::Retired,
                    Some(&digest),
                    None,
                )?;
                let datums = store.close_mapping_projection(&old, graph, timestamp)?;
                let mut evidence = parse(store, &audit, timestamp)?;
                evidence.extend(parse(
                    store,
                    &facts::decision(&old.mapping, &at, "retire")?,
                    timestamp,
                )?);
                let state = State {
                    mapping: old.mapping,
                    operations: old.operations,
                    status: MappingStatus::Retired,
                    actor: actor.into(),
                    authority,
                    timestamp: timestamp.into(),
                    tx,
                    record,
                };
                store.save_mapping_state(&state, timestamp)?;
                store.mapping_event(&state, "mapping.retired")?;
                Ok((
                    store.mapping_receipt(&state, false)?,
                    vec![(graph, datums), (operations, evidence)],
                ))
            },
        )
    }

    /// What a registry change closes and which decided mappings it must flag.
    /// Shared by the atomic settlement and the enclosed path below, so both
    /// apply the same ownership, cycle, key-contract and pin-advance checks.
    fn registry_change_preparation(
        &self,
        kind: RegistryKind,
        name: &str,
        turtle: Option<&str>,
        timestamp: &str,
        new_digest: Option<&str>,
    ) -> Result<Preparation<RegistryPlan, bool>> {
        let store = self;
        // Legacy unpinned versions can have date-only timestamps. A C1 pin
        // itself requires RFC3339, including its closing version's instant.
        let epoch = crate::time::epoch_of_rfc3339(timestamp);
        // These rows decide what every later write is validated against.
        store.ensure_ownership_writable()?;
        let mut statement = store.conn.prepare(&format!(
            "SELECT valid_from,digest FROM {} WHERE name=?1 AND valid_to IS NULL ORDER BY valid_from", kind.table()
        ))?;
        let current = statement
            .query_map([name], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if turtle.is_none() && current.is_empty() {
            return Ok(Preparation::Replay(false));
        }
        let pinned_at_timestamp = store.conn.prepare(
            "SELECT 1 FROM shape_mapping_pins WHERE registry=?1 AND name=?2 AND valid_from=?3 LIMIT 1"
        )?.exists(params![kind.table(),name,timestamp])?;
        if turtle.is_some() && pinned_at_timestamp {
            if current.len() == 1
                && current[0].0 == timestamp
                && current[0].1.as_deref() == new_digest
            {
                return Ok(Preparation::Replay(true));
            }
            return Err(invalid(
                "a pinned version cannot be replaced at its original timestamp",
            ));
        }
        let old_pins: Vec<_> = current
            .iter()
            .filter_map(|(at, digest)| {
                digest.as_ref().map(|digest| ShapePin {
                    registry: kind,
                    name: name.into(),
                    valid_from: at.clone(),
                    digest: digest.clone(),
                })
            })
            .collect();
        for pin in &old_pins {
            let retained = store.conn.prepare("SELECT 1 FROM shape_mapping_pins WHERE registry=?1 AND name=?2 AND valid_from=?3 AND digest=?4 LIMIT 1")?
                .exists(params![kind.table(),name,pin.valid_from,pin.digest])?;
            if retained {
                let old_epoch = crate::time::epoch_of_rfc3339(&pin.valid_from)
                    .ok_or_else(|| invalid("retained pinned timestamp is unreadable"))?;
                if epoch.is_none_or(|epoch| epoch <= old_epoch) {
                    return Err(invalid(
                        "registry validity must advance beyond retained pinned history",
                    ));
                }
            }
        }
        let mut statement = store.conn.prepare("SELECT DISTINCT mapping FROM shape_mapping_pins WHERE registry=?1 AND name=?2 ORDER BY mapping")?;
        let ids = statement
            .query_map(params![kind.table(), name], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut flagged = Vec::new();
        for id in ids {
            let state = store
                .mapping_state(&id)?
                .ok_or_else(|| invalid("pinned lifecycle state is missing"))?;
            if state.status != MappingStatus::Decided {
                continue;
            }
            if let Some(pin) = state
                .mapping
                .pins
                .iter()
                .find(|pin| old_pins.contains(pin))
                .cloned()
            {
                let old_epoch = crate::time::epoch_of_rfc3339(&pin.valid_from)
                    .ok_or_else(|| invalid("pinned timestamp is unreadable"))?;
                if epoch.is_none_or(|epoch| epoch <= old_epoch) {
                    return Err(invalid(
                        "shape version must advance beyond the current pinned version",
                    ));
                }
                let graph = store.mapping_graph(&state.mapping.graph)?;
                let operations = store.mapping_graph(&state.operations)?;
                flagged.push((state, pin, graph, operations));
            }
        }
        if kind == RegistryKind::Shapes {
            // Checked under this settlement's writer reservation, so no
            // concurrent key allocation can race the shape contract edit.
            let others: Vec<_> = store
                .list_shapes()?
                .into_iter()
                .filter(|(n, _, _)| n != name)
                .map(|(_, rdf, _)| rdf)
                .collect();
            let candidate = match turtle {
                Some(turtle) => {
                    let candidate = format!("{}\n{turtle}", others.join("\n"));
                    crate::shacl_cycles::refuse_cyclic_shapes(&candidate)?;
                    candidate
                }
                None => others.join("\n"),
            };
            crate::entity_key_guard::check_shapes(store, &candidate)?;
        }
        let unverified_versions = current
            .iter()
            .filter(|(_, digest)| digest.is_none())
            .count();
        Ok(Preparation::Write((old_pins, flagged, unverified_versions)))
    }

    /// A registry change inside a caller's enclosing write: a schema proposal
    /// accepted with its migration, and that acceptance's always-rolled-back
    /// dry run. Flagging needs its own settlement transaction, so a change
    /// that would flag a decided mapping is refused here rather than closing a
    /// pinned version without its `MappingRecord`; accept that proposal
    /// without a migration, or retire the mapping first. Otherwise the version
    /// row and its control event land in the enclosing transaction, under the
    /// same checks as [`Self::mapping_registry_change`].
    pub(super) fn mapping_registry_change_enclosed(
        &self,
        kind: RegistryKind,
        name: &str,
        turtle: Option<&str>,
        timestamp: &str,
    ) -> Result<bool> {
        if self.conn.is_autocommit() {
            return Err(invalid(
                "an enclosed registry change requires the caller's open transaction",
            ));
        }
        let new_digest = turtle
            .map(crate::shape_mapping::canonical_shape_digest)
            .transpose()?;
        let (old_pins, flagged, unverified_versions) = match self.registry_change_preparation(
            kind,
            name,
            turtle,
            timestamp,
            new_digest.as_deref(),
        )? {
            Preparation::Replay(value) => return Ok(value),
            Preparation::Write(plan) => plan,
        };
        if !flagged.is_empty() {
            return Err(invalid(
                "closing a version pinned by a decided mapping needs its own mapping settlement, not an enclosing migration",
            ));
        }
        let closed = if let Some(turtle) = turtle {
            self.load_versioned(kind.table(), name, turtle, timestamp)?;
            true
        } else {
            self.close_versioned(kind.table(), name, timestamp)?
        };
        if !closed {
            return Ok(false);
        }
        let new_pins = if turtle.is_some() {
            self.current_shape_pins(kind, name)?
        } else {
            Vec::new()
        };
        let event = match (kind, turtle.is_some()) {
            (RegistryKind::Shapes, true) => "shapes.loaded",
            (RegistryKind::Shapes, false) => "shapes.removed",
            (RegistryKind::Ontologies, true) => "ontology.loaded",
            (RegistryKind::Ontologies, false) => "ontology.removed",
        };
        let tx = self.latest_tx_id()?;
        let payload = serde_json::json!({"name":name,"registry":kind.table(),"closed_versions":old_pins,"unverified_closed_versions":unverified_versions,"versions":new_pins,"tx":tx});
        self.conn.execute("INSERT INTO events(type,ts,subject,group_id,tx_id,payload) VALUES(?1,?2,?3,NULL,?4,?5)",params![event,timestamp,name,tx,payload.to_string()])?;
        Ok(true)
    }

    /// Registry changes and every dependent mapping flag use one protected
    /// writer transaction. Registry-only changes need no graph batch, but
    /// still commit their version row and control event together.
    pub(super) fn mapping_registry_change(
        &mut self,
        kind: RegistryKind,
        name: &str,
        turtle: Option<&str>,
        timestamp: &str,
    ) -> Result<bool> {
        let actor = match crate::transaction_auth::current_identity() {
            Some(identity) if oxrdf::NamedNode::new(&identity.principal).is_ok() => {
                identity.principal
            }
            Some(identity) => format!(
                "urn:quipu:principal:{}",
                &crate::share::sha256(identity.principal.as_bytes())[7..]
            ),
            // Native database-file callers have no transport credential. This
            // names that execution boundary and asserts no human identity.
            None => "urn:quipu:principal:local-file-writer".into(),
        };
        let new_digest = turtle
            .map(crate::shape_mapping::canonical_shape_digest)
            .transpose()?;
        self.mapping_atomic(&actor, |store| {
            store.registry_change_preparation(kind, name, turtle, timestamp, new_digest.as_deref())
        }, |store,(old_pins,flagged,unverified_versions),now,tx| {
            let closed = if let Some(turtle) = turtle {
                store.load_versioned(kind.table(),name,turtle,timestamp)?;
                true
            } else { store.close_versioned(kind.table(),name,timestamp)? };
            let new_pins = if turtle.is_some() { store.current_shape_pins(kind,name)? } else { Vec::new() };
            let event = match (kind,turtle.is_some()) {
                (RegistryKind::Shapes,true)=>"shapes.loaded", (RegistryKind::Shapes,false)=>"shapes.removed",
                (RegistryKind::Ontologies,true)=>"ontology.loaded", (RegistryKind::Ontologies,false)=>"ontology.removed",
            };
            let payload = serde_json::json!({"name":name,"registry":kind.table(),"closed_versions":old_pins,"unverified_closed_versions":unverified_versions,"versions":new_pins,"tx":tx});
            store.conn.execute("INSERT INTO events(type,ts,subject,group_id,tx_id,payload) VALUES(?1,?2,?3,NULL,?4,?5)",params![event,timestamp,name,tx,payload.to_string()])?;
            let mut batches: std::collections::BTreeMap<i64,Vec<Datum>> = std::collections::BTreeMap::new();
            for (old,pin,graph,operations) in flagged {
                let authority = facts::decision_iri(&old.mapping,"flag",tx)?;
                let at = Settlement { actor:&actor,authority:&authority,timestamp:now,tx };
                let digest = old.mapping.digest()?;
                let (record,audit) = facts::record(&old.mapping,&at,Some(MappingStatus::Decided),MappingStatus::Flagged,Some(&digest),Some(&pin))?;
                let subject = store.intern(&old.mapping.id)?;
                let status = store.intern(&format!("{}mappingStatus",facts::NS))?;
                let data = batches.entry(graph).or_default();
                data.push(Datum { entity:subject,attribute:status,value:crate::Value::Str("decided".into()),valid_from:now.into(),valid_to:None,op:Op::Retract });
                data.push(Datum { entity:subject,attribute:status,value:crate::Value::Str("flagged".into()),valid_from:now.into(),valid_to:None,op:Op::Assert });
                let evidence = batches.entry(operations).or_default();
                evidence.extend(parse(store,&audit,now)?);
                evidence.extend(parse(store,&facts::decision(&old.mapping,&at,"flag")?,now)?);
                let state = State { mapping:old.mapping,operations:old.operations,status:MappingStatus::Flagged,
                    actor:actor.clone(),authority,timestamp:now.into(),tx,record };
                store.save_mapping_state(&state,now)?;
                let payload = serde_json::json!({"mapping":state.mapping.id,"old_pin":pin,"versions":new_pins,"record":state.record,"tx":tx});
                store.conn.execute("INSERT INTO events(type,ts,subject,group_id,tx_id,payload) VALUES('mapping.flagged',?1,?2,NULL,?3,?4)",params![now,state.mapping.id,tx,payload.to_string()])?;
            }
            Ok((closed,batches.into_iter().collect()))
        })
    }
}

#[cfg(test)]
#[path = "mapping_writes_tests.rs"]
mod tests;
