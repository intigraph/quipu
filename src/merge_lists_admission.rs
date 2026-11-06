//! Explicit list-mode admission bound to the exact shapes document.
//! This profile is caller input, not inferred RDF vocabulary or a write grant.
//! Adapters must bind its bytes into their merge identity before using it.
use super::{Mode, Slot, Value, invalid};
use crate::error::{Error, Result};
use crate::share_merge::Graph;
use serde::Deserialize;
use std::collections::BTreeMap;

const SCHEMA: &str = "quipu-list-profile/v1";
const MAX_SLOTS: usize = 4096;
const MAX_BYTES: usize = 1_048_576;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    shapes_sha256: String,
    slots: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    subject: String,
    predicate: String,
    mode: Mode,
}

/// A validated explicit profile. No adapter may infer additional list slots.
pub(crate) struct Admitted {
    /// Hash of the original profile bytes, for the caller's merge identity.
    pub(crate) profile_sha256: String,
    pub(crate) slots: BTreeMap<Slot, Mode>,
}

/// Refuse stale shapes, ambiguous entries and unknown profile fields.
/// The explicit profile declares list modes; SHACL cardinality or labels do not.
pub(crate) fn admit(bytes: &[u8], shapes: &str) -> Result<Admitted> {
    if bytes.len() > MAX_BYTES {
        return Err(invalid("profile byte budget exceeded"));
    }
    let profile: Profile = serde_json::from_slice(bytes)
        .map_err(|e| Error::InvalidValue(format!("list profile: {e}")))?;
    if profile.schema != SCHEMA {
        return Err(invalid("unknown list profile schema"));
    }
    if profile.shapes_sha256 != crate::share::sha256(shapes.as_bytes()) {
        return Err(invalid("list profile shapes digest mismatch"));
    }
    // Validate Turtle even if there are no cardinality constraints.
    crate::merge3::ShapeBounds::parse(shapes)?;
    if profile.slots.len() > MAX_SLOTS {
        return Err(invalid("profile slot budget exceeded"));
    }
    let mut slots = BTreeMap::new();
    for entry in profile.slots {
        let subject = crate::fork_merge_decisions::parse_term(&entry.subject)?;
        if matches!(subject, oxrdf::Term::Literal(_)) {
            return Err(invalid("list profile subject must be a resource"));
        }
        let predicate = oxrdf::NamedNode::new(entry.predicate)
            .map_err(|_| invalid("list profile predicate must be an absolute IRI"))?;
        let slot = (subject.to_string(), predicate.as_str().to_owned());
        if slots.insert(slot, entry.mode).is_some() {
            return Err(invalid("duplicate list profile slot"));
        }
    }
    Ok(Admitted {
        profile_sha256: crate::share::sha256(bytes),
        slots,
    })
}

/// All three structurally validated values for one explicitly admitted slot.
pub(crate) struct PreparedSlot {
    pub(crate) mode: Mode,
    pub(crate) base: Option<Value>,
    pub(crate) ours: Option<Value>,
    pub(crate) theirs: Option<Value>,
}

/// Pure input preparation; no operation has been planned or written yet.
pub(crate) struct Prepared {
    /// Adapters must include this binding in their merge/replay identity.
    pub(crate) binding: String,
    pub(crate) slots: BTreeMap<Slot, PreparedSlot>,
}

/// Validate every side before any planner can consume a profile.
/// The binding is sensitive to profile bytes, shapes bytes, all graph content
/// and side order, but independent of each graph's hash-table iteration order.
pub(crate) fn prepare(
    bytes: &[u8],
    shapes: &str,
    base: &Graph,
    ours: &Graph,
    theirs: &Graph,
) -> Result<Prepared> {
    let profile = admit(bytes, shapes)?;
    let mut b = super::read(base, &profile.slots)?;
    let mut o = super::read(ours, &profile.slots)?;
    let mut t = super::read(theirs, &profile.slots)?;
    let digest = |graph: &Graph| -> Result<String> {
        let mut triples: Vec<_> = graph.iter().map(ToString::to_string).collect();
        triples.sort();
        let bytes =
            serde_json::to_vec(&triples).map_err(|e| Error::Serialization(e.to_string()))?;
        Ok(crate::share::sha256(&bytes))
    };
    let binding = serde_json::to_vec(&(
        "quipu-list-inputs/v1",
        &profile.profile_sha256,
        crate::share::sha256(shapes.as_bytes()),
        digest(base)?,
        digest(ours)?,
        digest(theirs)?,
    ))
    .map_err(|e| Error::Serialization(e.to_string()))?;
    let slots = profile
        .slots
        .into_iter()
        .map(|(slot, mode)| {
            let values = PreparedSlot {
                mode,
                base: b.remove(&slot).flatten(),
                ours: o.remove(&slot).flatten(),
                theirs: t.remove(&slot).flatten(),
            };
            (slot, values)
        })
        .collect();
    Ok(Prepared {
        binding: crate::share::sha256(&binding),
        slots,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn profile() -> serde_json::Value {
        json!({"schema": SCHEMA, "shapes_sha256": crate::share::sha256(b""),
            "slots": [{"subject":"<urn:owner>", "predicate":"urn:list", "mode":"whole"}]})
    }

    #[test]
    fn explicit_modes_bind_exact_shapes_and_profile_bytes() {
        let bytes = serde_json::to_vec(&profile()).unwrap();
        let admitted = admit(&bytes, "").unwrap();
        assert_eq!(admitted.profile_sha256, crate::share::sha256(&bytes));
        assert_eq!(admitted.slots.len(), 1);
        assert_eq!(
            admitted.slots[&("<urn:owner>".into(), "urn:list".into())],
            Mode::Whole
        );
        assert!(admit(&bytes, "# changed shapes bytes\n").is_err());
        let mut sequence = profile();
        sequence["slots"][0]["mode"] = json!("sequence");
        assert_eq!(
            admit(&serde_json::to_vec(&sequence).unwrap(), "")
                .unwrap()
                .slots
                .values()
                .next(),
            Some(&Mode::Sequence)
        );
    }

    #[test]
    fn ambiguous_or_unknown_profile_inputs_refuse() {
        let mut variants = Vec::new();
        let mut duplicate = profile();
        let entry = duplicate["slots"][0].clone();
        duplicate["slots"].as_array_mut().unwrap().push(entry);
        variants.push(duplicate);
        for (field, value) in [
            ("subject", "\"literal\""),
            ("predicate", "relative"),
            ("mode", "guess"),
        ] {
            let mut bad = profile();
            bad["slots"][0][field] = json!(value);
            variants.push(bad);
        }
        let mut unknown = profile();
        unknown["guess_modes"] = json!(true);
        variants.push(unknown);
        for variant in variants {
            assert!(admit(&serde_json::to_vec(&variant).unwrap(), "").is_err());
        }
    }

    #[test]
    fn normalized_duplicate_slots_and_invalid_shapes_refuse() {
        let mut duplicate = profile();
        let mut entry = duplicate["slots"][0].clone();
        entry["subject"] = json!("  <urn:owner>  ");
        duplicate["slots"].as_array_mut().unwrap().push(entry);
        assert!(admit(&serde_json::to_vec(&duplicate).unwrap(), "").is_err());

        // A matching digest is necessary but does not make invalid Turtle valid.
        let shapes = "this is not Turtle";
        let mut matching = profile();
        matching["shapes_sha256"] = json!(crate::share::sha256(shapes.as_bytes()));
        assert!(admit(&serde_json::to_vec(&matching).unwrap(), shapes).is_err());

        // The profile is explicit: a parsed shape alone admits no list slots.
        let shapes = "<urn:s> <http://www.w3.org/ns/shacl#path> <urn:list> .";
        let mut empty = profile();
        empty["shapes_sha256"] = json!(crate::share::sha256(shapes.as_bytes()));
        empty["slots"] = json!([]);
        assert!(
            admit(&serde_json::to_vec(&empty).unwrap(), shapes)
                .unwrap()
                .slots
                .is_empty()
        );
    }

    #[test]
    fn prepared_inputs_bind_sides_profile_and_nonlist_context() {
        let bytes = serde_json::to_vec(&profile()).unwrap();
        let slot = ("<urn:owner>".into(), "urn:list".into());
        let empty = Graph::new();
        let graph = super::super::lower(&slot, &Some(vec!["\"x\"".into()])).unwrap();
        let prepared = prepare(&bytes, "", &empty, &empty, &graph).unwrap();
        let value = &prepared.slots[&slot];
        assert_eq!(value.mode, Mode::Whole);
        assert!(value.base.is_none() && value.ours.is_none());
        assert_eq!(value.theirs.as_ref().unwrap().elements, vec!["\"x\""]);
        assert_eq!(
            prepared.binding,
            prepare(&bytes, "", &empty, &empty, &graph).unwrap().binding
        );
        assert_ne!(
            prepared.binding,
            prepare(&bytes, "", &empty, &graph, &empty).unwrap().binding
        );
        let mut context = graph.clone();
        context.extend(
            crate::share_merge::parse_graph("<urn:context> <urn:p> \"changed\" .", "context")
                .unwrap(),
        );
        assert_ne!(
            prepared.binding,
            prepare(&bytes, "", &empty, &empty, &context)
                .unwrap()
                .binding
        );
        let spaced = serde_json::to_vec_pretty(&profile()).unwrap();
        assert_ne!(
            prepared.binding,
            prepare(&spaced, "", &empty, &empty, &graph)
                .unwrap()
                .binding
        );
    }

    #[test]
    fn preparing_refuses_a_malformed_side_before_returning_inputs() {
        let bytes = serde_json::to_vec(&profile()).unwrap();
        let empty = Graph::new();
        let bad = crate::share_merge::parse_graph(
            "<urn:owner> <urn:list> _:a .\n_:a <http://www.w3.org/1999/02/22-rdf-syntax-ns#first> \"x\" .\n",
            "missing rest",
        ).unwrap();
        for (b, o, t) in [
            (&bad, &empty, &empty),
            (&empty, &bad, &empty),
            (&empty, &empty, &bad),
        ] {
            assert!(prepare(&bytes, "", b, o, t).is_err());
        }
    }
}
