//! Private in-memory profile comparison. No endpoint, credential file or listener.
use quipu::{
    Store,
    read_authorization::{OwnerReadPolicy, ReadGrant, ReadIdentity, ReadOperation},
};
use serde_json::json;
use sha2::{Digest, Sha256};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut source = Store::open_in_memory()?;
    for (graph, value) in [
        ("urn:graph:a", "allowed"),
        ("urn:graph:b", "fixture-secret"),
    ] {
        let g = source.graph_create(graph)?;
        let text = format!("<urn:entity:{value}> <urn:p> \"{value}\" .");
        quipu::rdf::ingest_rdf_to_graph(
            &mut source,
            text.as_bytes(),
            oxrdfio::RdfFormat::NTriples,
            None,
            "2026-01-01T00:00:00Z",
            None,
            None,
            g,
        )?;
    }
    let mut owner = OwnerReadPolicy::open(":memory:", "urn:owner")?;
    let grant = ReadGrant {
        version: 1,
        id: "urn:grant:p".into(),
        principal: "urn:principal:p".into(),
        graphs: ["urn:graph:a".into()].into(),
        operations: [ReadOperation::Query].into(),
        valid_from: 1,
        valid_until: 100,
        max_offline_seconds: 300,
        issuer: "urn:owner".into(),
        revision: 1,
        revoked: false,
    };
    owner.apply(&grant, "private demo grant")?;
    let token = "private-fixture-only";
    let registry = quipu::crew_credentials::CredentialRegistry::parse(
        &serde_json::to_vec(&json!({"version":1,"credentials":[{
            "credential_id":"fixture","principal":"urn:principal:p","audience":"quipu",
            "token_sha256":format!("{:x}",Sha256::digest(token.as_bytes()))
        }]}))?,
        "quipu",
    )?;
    let identity = ReadIdentity::authenticate_named(&registry, Some(&format!("Bearer {token}")))?;
    let snapshot = owner.snapshot()?;
    let view = snapshot
        .authorize(&identity, ReadOperation::Query, None, 2)?
        .open(&source, 2)?;
    let denied = "SELECT ?value WHERE { GRAPH <urn:graph:b> { ?entity <urn:p> ?value } }";
    println!(
        "Target: private in-memory fixture; unchanged legacy API and new scoped API on the candidate."
    );
    println!("P has query permission for A only. Same explicit B query:");
    println!(
        "legacy: {}",
        quipu::tool_query(&source, &json!({"query":denied}))?
    );
    let refusal = view.query(denied, false, 2).expect_err("B must refuse");
    println!("scoped: {refusal}");
    println!(
        "Allowed A control: {}",
        String::from_utf8(view.query(
            "ASK { <urn:entity:allowed> <urn:p> \"allowed\" }",
            true,
            2
        )?)?
    );
    let mut revoked = grant;
    revoked.revoked = true;
    revoked.revision = 2;
    owner.apply(&revoked, "private demo revocation")?;
    println!(
        "After revocation, buffered publication recheck: {}",
        view.recheck(&owner.snapshot()?, &identity, 2)
            .expect_err("revoked view must refuse")
    );
    println!("No production access or bootstrap/lease/runtime acceptance is demonstrated.");
    Ok(())
}
