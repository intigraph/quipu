//! w3id follow-through for the companion tag (aegis-m5s.3): a fresh store
//! writes only the canonical camayoc-core `sourceKind`, and a store tagged
//! under the former `aegis.gastown.local` spelling still reads as tagged.

use super::*;
use crate::reasoner::{RULE_NS, evaluate, parse_rules};

const TS: &str = "2026-01-01T00:00:00Z";
const OLD_HOSTS: [&str; 2] = ["aegis.gastown.local", "scbrown.github.io/quechua"];

fn companion_terms(store: &Store) -> Vec<String> {
    let companion = store.lookup(ROOT_INFERRED_GRAPH_IRI).unwrap().unwrap();
    let mut terms = Vec::new();
    for f in store.current_facts_in_graph(companion).unwrap() {
        terms.push(store.resolve(f.entity).unwrap());
        terms.push(store.resolve(f.attribute).unwrap());
        if let Value::Ref(id) = f.value {
            terms.push(store.resolve(id).unwrap());
        }
    }
    terms
}

/// Negative control: a fresh install's reasoner run puts zero old-namespace
/// terms in the companion, and the tag it does write is the canonical one.
#[test]
fn fresh_companion_writes_no_old_namespace_terms() {
    let ttl = format!(
        "@prefix rule: <{RULE_NS}> .\n\
         <http://ex/r> a rule:Rule ; rule:id \"R1\" ;\n\
         rule:head \"<http://ex/h>(?x, ?y)\" ; rule:body \"<http://ex/p>(?x, ?y)\" .\n"
    );
    let mut store = Store::open_in_memory().unwrap();
    let datum = Datum {
        entity: store.intern("http://ex/a").unwrap(),
        attribute: store.intern("http://ex/p").unwrap(),
        value: Value::Ref(store.intern("http://ex/b").unwrap()),
        valid_from: TS.to_string(),
        valid_to: None,
        op: Op::Assert,
    };
    store
        .transact(&[datum], TS, Some("test"), Some("base"))
        .unwrap();
    evaluate(&mut store, &parse_rules(&ttl, None).unwrap(), TS).unwrap();

    let terms = companion_terms(&store);
    assert!(terms.iter().any(|t| t == SOURCE_KIND), "{terms:?}");
    for host in OLD_HOSTS {
        assert!(
            !terms.iter().any(|t| t.contains(host)),
            "{host} in {terms:?}"
        );
    }
}

/// Recall control: a companion tagged under the former spelling is
/// recognised as tagged — not re-tagged, and the old tag is not rewritten.
#[test]
fn legacy_tagged_companion_is_still_recognised() {
    let mut store = Store::open_in_memory().unwrap();
    let companion = store.intern(ROOT_INFERRED_GRAPH_IRI).unwrap();
    let legacy = Datum {
        entity: companion,
        attribute: store.intern(LEGACY_SOURCE_KIND).unwrap(),
        value: Value::Str("inferred".into()),
        valid_from: TS.to_string(),
        valid_to: None,
        op: Op::Assert,
    };
    store
        .transact_to_graph(&[legacy], TS, Some("quipu"), Some(PLANE_SOURCE), companion)
        .unwrap();

    assert_eq!(
        store.ensure_companion_inferred_graph(0, TS).unwrap(),
        companion
    );

    let terms = companion_terms(&store);
    assert!(terms.iter().any(|t| t == LEGACY_SOURCE_KIND), "{terms:?}");
    assert!(!terms.iter().any(|t| t == SOURCE_KIND), "{terms:?}");
}
