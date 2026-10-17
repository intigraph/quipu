use super::*;
use crate::config::EndpointPendant;
use crate::share::{PointerTarget, ShareDestination, ShareOptions, SharePointer};

fn declaration() -> (tempfile::TempDir, FederationConfig) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = crate::Store::open_in_memory().unwrap();
    crate::share_scrub::seed_test_catalogue(&mut store);
    let path = dir.path().join("declaration.pendant");
    crate::share::share(
        &store,
        path.to_str().unwrap(),
        &ShareOptions {
            no_shapes: true,
            destination: ShareDestination::Internal,
            pointers: vec![
                SharePointer {
                    graph: "urn:remote".into(),
                    target: PointerTarget::Endpoint {
                        endpoint: "https://example.org/sparql/".into(),
                    },
                },
                SharePointer {
                    graph: "urn:pack".into(),
                    target: PointerTarget::Pack {
                        url: "https://example.org/pack".into(),
                        sha256: "a".repeat(64),
                    },
                },
            ],
            ..Default::default()
        },
    )
    .unwrap();
    let cfg = FederationConfig {
        pendants: vec![EndpointPendant {
            path,
            repo_remote: "https://github.com/foreign/project.git".into(),
        }],
        ..Default::default()
    };
    (dir, cfg)
}

#[test]
fn exact_verified_endpoint_requires_current_consent_and_has_no_authority() {
    let (_dir, cfg) = declaration();
    let mut calls = 0;
    let remote = cfg
        .pointer_endpoint_with("https://example.org/sparql/", |repo, endpoint| {
            assert_eq!(repo, "https://github.com/foreign/project.git");
            assert_eq!(endpoint, "https://example.org/sparql/");
            calls += 1;
            Ok(())
        })
        .unwrap()
        .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(remote.name, "urn:remote");
    assert_eq!(remote.url, "https://example.org/sparql/");
    assert!(remote.auth_token.is_none());
    assert!(remote.trust.is_none());
    assert!(remote.freshness.is_none());
    let err = cfg
        .pointer_endpoint_with("https://example.org/sparql/", |_, _| {
            Err(Error::InvalidValue("revoked".into()))
        })
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("revoked")
            && err.contains("github.com/foreign/project")
            && err.contains("https://example.org/sparql/"),
        "{err}"
    );
}

#[test]
fn url_neighbours_and_pack_pointers_never_call_policy_or_fetch() {
    let (_dir, cfg) = declaration();
    for url in [
        "https://example.org/sparql",
        "https://example.org/sparql/query",
        "https://example.org/pack",
        "https://other.example/query",
    ] {
        assert!(
            cfg.pointer_endpoint_with(url, |_, _| panic!("not declared"))
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn forged_declaration_is_refused_before_trust() {
    let (_dir, cfg) = declaration();
    let path = cfg.pendants[0].path.join("manifest.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["pointers"][0]["endpoint"] = "https://attacker.example/query".into();
    std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    let err = cfg
        .pointer_endpoint_with("https://attacker.example/query", |_, _| {
            panic!("unverified")
        })
        .unwrap_err()
        .to_string();
    assert!(err.contains("share id mismatch"), "{err}");
}

#[test]
fn config_defaults_to_no_pointer_declarations() {
    let cfg: FederationConfig = toml::from_str("").unwrap();
    assert!(cfg.pendants.is_empty());
    let cfg: FederationConfig = toml::from_str("[[pendants]]\npath='local.pendant'\nrepo_remote='https://github.com/foreign/project.git'\n").unwrap();
    assert_eq!(cfg.pendants.len(), 1);
    assert!(
        toml::from_str::<FederationConfig>(
            "[[pendants]]\npath='local.pendant'\nrepo_remote='repo'\ntrust=true"
        )
        .is_err()
    );
}
