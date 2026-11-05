//! The governance vocabulary resolves under the store's own `base_ns`, and a
//! store with none fails closed (gju5 S3b). Wired from `governance/mod.rs`.

use crate::error::Error;
use crate::namespace::{DEFAULT_BASE_NS, RDF_TYPE};
use crate::store::{Datum, Store};
use crate::types::{Op, Value};

const TS: &str = "2026-01-01T00:00:00Z";
const OWN: &str = "http://example.org/kb/";
const DOC: &str = "http://ex/Doc";
const LABEL: &str = "http://www.w3.org/2000/01/rdf-schema#label";
const REQUIRE_LABEL: &str = "ASK { $target <http://www.w3.org/2000/01/rdf-schema#label> ?l }";

fn datum(store: &Store, s: &str, p: &str, v: Value) -> Datum {
    Datum {
        entity: store.intern(s).unwrap(),
        attribute: store.intern(p).unwrap(),
        value: v,
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    }
}

fn iri(store: &Store, i: &str) -> Value {
    Value::Ref(store.intern(i).unwrap())
}

/// A deny policy "every Doc has a label", written under `ns`.
fn define_policy(store: &mut Store, ns: &str) {
    let p = "http://ex/policy/label";
    let datums = vec![
        datum(store, p, RDF_TYPE, iri(store, &format!("{ns}Policy"))),
        datum(store, p, &format!("{ns}targets"), Value::Str(DOC.into())),
        datum(
            store,
            p,
            &format!("{ns}claim"),
            Value::Str(REQUIRE_LABEL.into()),
        ),
        datum(
            store,
            p,
            &format!("{ns}boundary"),
            Value::Str("action".into()),
        ),
        datum(store, p, &format!("{ns}effect"), Value::Str("deny".into())),
    ];
    store.transact(&datums, TS, None, None).unwrap();
}

fn unlabeled_doc(store: &mut Store, s: &str) -> crate::error::Result<()> {
    let w = vec![datum(store, s, RDF_TYPE, iri(store, DOC))];
    store.transact(&w, TS, None, None).map(|_| ())
}

fn unavailable(r: crate::error::Result<()>) -> bool {
    matches!(r, Err(e) if e.to_string().contains("governance is unavailable"))
}

#[test]
fn a_store_is_governed_by_policies_in_its_own_namespace() {
    let mut store = Store::open_in_memory().unwrap();
    store.set_base_ns(OWN);
    store.governance_config_mut().enforce_on_write = true;
    define_policy(&mut store, OWN);
    let r = unlabeled_doc(&mut store, "http://ex/d1");
    assert!(matches!(r, Err(Error::PolicyDenied(_))), "{r:?}");
    let ok = vec![
        datum(&store, "http://ex/d2", RDF_TYPE, iri(&store, DOC)),
        datum(&store, "http://ex/d2", LABEL, Value::Str("hi".into())),
    ];
    store
        .transact(&ok, TS, None, None)
        .expect("a compliant write passes");
}

#[test]
fn the_legacy_namespace_keeps_identical_iris() {
    // A store pinned to the old constant reads and writes exactly the
    // IRIs it did before.
    let mut store = Store::open_in_memory().unwrap();
    store.set_base_ns(DEFAULT_BASE_NS);
    assert_eq!(store.governance_ns().unwrap(), DEFAULT_BASE_NS);
    store.governance_config_mut().enforce_on_write = true;
    define_policy(&mut store, DEFAULT_BASE_NS);
    assert!(matches!(
        unlabeled_doc(&mut store, "http://ex/d1"),
        Err(Error::PolicyDenied(_))
    ));
}

#[test]
fn enforcement_without_a_namespace_refuses_instead_of_finding_no_policies() {
    let mut store = Store::open_in_memory().unwrap();
    assert!(store.governance_ns().is_err());
    store.governance_config_mut().enforce_on_write = true;
    assert!(unavailable(unlabeled_doc(&mut store, "http://ex/d1")));
}

#[test]
fn without_a_namespace_plain_writes_pass_and_governance_shaped_ones_refuse() {
    let mut store = Store::open_in_memory().unwrap();
    // Plain data: no always-on gate has anything to check.
    unlabeled_doc(&mut store, "http://ex/d1").expect("plain write");
    // Governance terms, by local name in ANY namespace, are refused: the
    // gates that would judge them cannot read their rules.
    let scheme = vec![datum(
        &store,
        "http://ex/r",
        "http://elsewhere.example/v#signatureScheme",
        Value::Str("tpm".into()),
    )];
    assert!(unavailable(
        store.transact(&scheme, TS, None, None).map(|_| ())
    ));
    let typed = vec![datum(
        &store,
        "http://ex/p",
        RDF_TYPE,
        iri(&store, "https://elsewhere.example/ns#Policy"),
    )];
    assert!(unavailable(
        store.transact(&typed, TS, None, None).map(|_| ())
    ));
    let transition = vec![datum(
        &store,
        "http://ex/t",
        RDF_TYPE,
        iri(&store, &format!("{DEFAULT_BASE_NS}TransitionEvent")),
    )];
    assert!(unavailable(
        store.transact(&transition, TS, None, None).map(|_| ())
    ));
}

#[test]
fn without_a_namespace_generic_words_on_plain_entities_still_pass() {
    // `predicate`, `effect`, `claim` are governance words only on a
    // governance class; a reified fact or an ordinary record uses them too.
    let mut store = Store::open_in_memory().unwrap();
    let w = vec![
        datum(
            &store,
            "http://ex/f",
            "http://ex/v#predicate",
            iri(&store, "http://ex/p"),
        ),
        datum(
            &store,
            "http://ex/f",
            "http://ex/v#effect",
            Value::Str("x".into()),
        ),
        datum(
            &store,
            "http://ex/f",
            "http://ex/v#claim",
            Value::Str("y".into()),
        ),
    ];
    store.transact(&w, TS, None, None).expect("plain record");
    // Amending an entity that IS typed with a governance class is refused.
    let mut legacy = Store::open_in_memory_test_ns().unwrap();
    define_policy(&mut legacy, DEFAULT_BASE_NS);
    legacy.set_base_ns("");
    let amend = vec![datum(
        &legacy,
        "http://ex/policy/label",
        "http://ex/v#note",
        Value::Str("z".into()),
    )];
    assert!(unavailable(
        legacy.transact(&amend, TS, None, None).map(|_| ())
    ));
}

#[test]
fn policies_stranded_under_another_namespace_refuse_instead_of_being_ignored() {
    // An upgrade sets a new base_ns over policies written under the old one:
    // the guard would load none of them and deny nothing (malcolm, #1011).
    let mut store = Store::open_in_memory_test_ns().unwrap();
    define_policy(&mut store, DEFAULT_BASE_NS);
    store.set_base_ns(OWN);
    store.governance_config_mut().enforce_on_write = true;
    let r = unlabeled_doc(&mut store, "http://ex/d1");
    assert!(
        matches!(&r, Err(e) if e.to_string().contains("silently ignored")),
        "{r:?}"
    );
    // A class merely named Policy elsewhere, without that namespace's claim,
    // is not a stranded policy.
    let mut other = Store::open_in_memory().unwrap();
    other.set_base_ns(OWN);
    other.governance_config_mut().enforce_on_write = true;
    let w = vec![datum(
        &other,
        "http://ex/insurance",
        RDF_TYPE,
        iri(&other, "http://vocab.example/ins#Policy"),
    )];
    other.transact(&w, TS, None, None).expect("unrelated class");
    unlabeled_doc(&mut other, "http://ex/d2").expect("no stranded policy");
}

#[test]
fn reads_without_a_namespace_refuse_rather_than_answer_empty() {
    let store = Store::open_in_memory().unwrap();
    assert!(crate::governance::guard::PolicyRegistry::build(&store).is_err());
    assert!(crate::governance::inventory::load(&store).is_err());
}
