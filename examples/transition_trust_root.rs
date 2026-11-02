//! Private in-memory transition trust probe. No live store, registration or key.
use quipu::{
    governance::{
        transition::transition_message,
        trust_root::{bootstrap, bootstrap_message, endorse, endorse_challenge},
    },
    namespace::{DEFAULT_BASE_NS as NS, PROV, RDF_TYPE},
    signing::{public_key_hex, sign_hex},
    store::{Datum, Store},
    types::{Op, Value},
};
use ring::{rand::SystemRandom, signature::Ed25519KeyPair};

const AT: &str = "2026-10-28T00:00:00Z";

fn key() -> Ed25519KeyPair {
    let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap()
}

fn datum(store: &Store, entity: &str, attribute: &str, value: Value) -> Datum {
    Datum {
        entity: store.intern(entity).unwrap(),
        attribute: store.intern(attribute).unwrap(),
        value,
        valid_from: AT.into(),
        valid_to: None,
        op: Op::Assert,
    }
}

fn run(label: &str, named: bool, governed: bool, enforce: bool) {
    let mut store = Store::open_in_memory().unwrap();
    store.governance_config_mut().verify_transitions = enforce;
    let agent = key();
    let class = store.intern(&format!("{NS}VerifierRegistration")).unwrap();
    let registration = vec![
        datum(&store, "urn:demo:registration", RDF_TYPE, Value::Ref(class)),
        datum(
            &store,
            "urn:demo:registration",
            &format!("{NS}verifier"),
            Value::Str("alice".into()),
        ),
        datum(
            &store,
            "urn:demo:registration",
            &format!("{NS}publicKey"),
            Value::Str(public_key_hex(&agent)),
        ),
    ];
    let graph = if named {
        store.graph_create("urn:demo:identity").unwrap()
    } else {
        0
    };
    store
        .transact_to_graph(&registration, AT, None, None, graph)
        .unwrap();
    if governed {
        let root = key();
        let public = public_key_hex(&root);
        let proof = sign_hex(
            &root,
            &bootstrap_message(&store.store_id().unwrap(), "demo-human", &public),
        );
        bootstrap(&mut store, "demo-human", &public, &[], &proof, AT).unwrap();
        let challenge = endorse_challenge(&store, "urn:demo:registration", "agent").unwrap();
        endorse(
            &mut store,
            "urn:demo:registration",
            "agent",
            "demo-human",
            &challenge.nonce,
            &sign_hex(&root, &challenge.message),
            "1970-01-01T00:00:00Z",
        )
        .unwrap();
    }
    let signature = sign_hex(
        &agent,
        &transition_message("urn:demo:run", "review", "open", "reviewed", AT, "alice"),
    );
    let class = store.intern(&format!("{NS}TransitionEvent")).unwrap();
    let mut transition = vec![datum(&store, "urn:demo:event", RDF_TYPE, Value::Ref(class))];
    for (field, value) in [
        ("inRun", "urn:demo:run"),
        ("atStep", "urn:demo:step/review"),
        ("fromState", "open"),
        ("toState", "reviewed"),
        ("performedBy", "urn:shuttle:agent:alice"),
        ("signature", signature.as_str()),
    ] {
        transition.push(datum(
            &store,
            "urn:demo:event",
            &format!("{NS}{field}"),
            Value::Str(value.into()),
        ));
    }
    transition.push(datum(
        &store,
        "urn:demo:event",
        &format!("{PROV}endedAtTime"),
        Value::Str(AT.into()),
    ));
    match store.transact(&transition, AT, None, None) {
        Ok(_) => println!("{label}=ADMITTED"),
        Err(quipu::Error::PolicyDenied(_)) => println!("{label}=REFUSED"),
        Err(error) => panic!("unexpected fixture error: {error}"),
    }
}

fn main() {
    println!("fixture=in-memory keys=ephemeral live_operations=NONE");
    run("unendorsed_root", false, false, true);
    run("unendorsed_named_identity", true, false, true);
    run("human_root_endorsed", false, true, true);
    run("legacy_flag_off", false, false, false);
}
