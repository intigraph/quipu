//! Registry changes that settle C1 mapping flags: the shared preparation, the
//! atomic settlement used by `load_shapes`/`load_ontology`, and the enclosed
//! path used inside a schema proposal's migration savepoint. Split from
//! `mapping_writes.rs`, whose private lifecycle helpers it uses.

use rusqlite::params;

use crate::shape_mapping::facts::{self, MappingStatus, Settlement};
use crate::shape_mapping::{RegistryKind, ShapePin};
use crate::{Op, Result};

use super::super::mapping_atomic::Preparation;
use super::super::{Datum, Store};
use super::{State, invalid, parse};

/// A decided mapping a registry change must flag: its state, the closed pin,
/// and its mapping and operations graph ids.
type FlaggedPin = (State, ShapePin, i64, i64);

/// The pins a registry change closes, the decided mappings it flags, and how
/// many closed versions predate digests.
type RegistryPlan = (Vec<ShapePin>, Vec<FlaggedPin>, usize);

impl Store {
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
    pub(in crate::store) fn mapping_registry_change_enclosed(
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
    pub(in crate::store) fn mapping_registry_change(
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
