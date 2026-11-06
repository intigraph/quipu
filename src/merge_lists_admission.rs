//! Explicit list-mode admission bound to the exact shapes document.
//! This profile is caller input, not inferred RDF vocabulary or a write grant.
//! Adapters must bind its bytes into their merge identity before using it.
use super::{Mode, Slot, invalid};
use crate::error::{Error, Result};
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
}
