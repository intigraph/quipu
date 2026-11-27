//! B4 (aegis-n0wc.5): the versioned composition-cut token, refusal of a
//! foreign-store / foreign-epoch cursor, and the transaction scalar bound to
//! one file. Runs over the same two-file matrix: `REFS` lives in the legacy
//! file, `APP` and `PROV` in one routed set file.

use super::*;
use crate::store::composition_cut::{BoundScalar, CompositionCut};

/// Capture a cut over `graphs` and hand it, with the reads, to `check`.
fn with_cut<T>(
    m: &Matrix,
    graphs: &[&str],
    check: impl FnOnce(&[CapturedRead<'_>], CompositionCut) -> T,
) -> T {
    let mut out = None;
    m.dataset
        .read(graphs, deadline(), |reads| {
            let cut = CompositionCut::capture(reads)?;
            out = Some(check(reads, cut));
            Ok(())
        })
        .unwrap();
    out.unwrap()
}

fn refusal(result: Result<()>) -> String {
    result.expect_err("expected a refusal").to_string()
}

fn edit(cut: &CompositionCut, f: impl FnOnce(&mut serde_json::Value)) -> CompositionCut {
    let mut value = serde_json::to_value(cut).unwrap();
    f(&mut value);
    serde_json::from_value(value).unwrap()
}

#[test]
fn a_cut_names_every_selected_graph_with_its_file_coordinate() {
    let m = matrix();
    with_cut(&m, &[REFS, APP, PROV], |reads, cut| {
        assert_eq!(cut.version(), 1);
        let members = cut.members();
        assert_eq!(members.keys().map(String::as_str).collect::<Vec<_>>(), {
            let mut want = vec![REFS, APP, PROV];
            want.sort_unstable();
            want
        },);
        for read in reads {
            let member = &members[&read.route.graph];
            assert_eq!(member.store_id, read.route.store_id);
            assert_eq!(member.log_epoch, read.route.log_epoch);
            assert_eq!(member.local_tx, read.local_tx);
            assert_eq!(member.generation, read.route.generation);
            assert_eq!(member.policy, read.route.policy);
        }
        assert_eq!(cut.catalog_revision(), reads[0].catalog_revision);
        // APP and PROV share one file: same coordinate, two members.
        assert_eq!(members[APP].store_id, members[PROV].store_id);
        assert_ne!(members[APP].store_id, members[REFS].store_id);
    });
}

#[test]
fn the_token_round_trips_and_its_digest_is_canonical() {
    let m = matrix();
    let (token, digest) = with_cut(&m, &[REFS, APP], |reads, cut| {
        let token = cut.to_token();
        assert!(token.starts_with("qcut1."), "{token}");
        let back = CompositionCut::from_token(&token).unwrap();
        assert_eq!(back, cut);
        assert_eq!(back.digest(), cut.digest());
        back.admit(reads).unwrap();
        (token, cut.digest())
    });
    assert_eq!(digest.len(), 64, "sha256 hex: {digest}");
    // Selection order does not change the cut, its token or its digest.
    with_cut(&m, &[APP, REFS], |_, cut| {
        assert_eq!(cut.to_token(), token);
        assert_eq!(cut.digest(), digest);
    });
}

#[test]
fn a_cursor_from_a_foreign_store_or_epoch_refuses() {
    let m = matrix();
    with_cut(&m, &[REFS, APP], |reads, cut| {
        let store = edit(&cut, |v| {
            v["members"][APP]["store_id"] = "urn:other-store".into();
        });
        assert!(refusal(store.admit(reads)).contains("foreign-store"));
        let epoch = edit(&cut, |v| {
            v["members"][APP]["log_epoch"] = "urn:other-epoch".into();
        });
        assert!(refusal(epoch.admit(reads)).contains("foreign-epoch"));
        // The legacy member is checked the same way.
        let legacy = edit(&cut, |v| {
            v["members"][REFS]["log_epoch"] = "urn:other-epoch".into();
        });
        assert!(refusal(legacy.admit(reads)).contains("foreign-epoch"));
    });
}

#[test]
fn a_cut_for_a_different_selection_refuses() {
    let m = matrix();
    let wide = with_cut(&m, &[REFS, APP, PROV], |_, cut| cut);
    with_cut(&m, &[REFS, APP], |reads, narrow| {
        assert!(refusal(wide.admit(reads)).contains("extra-member"));
        let missing = edit(&narrow, |v| {
            v["members"].as_object_mut().unwrap().remove(APP);
        });
        assert!(refusal(missing.admit(reads)).contains("missing-member"));
    });
}

#[test]
fn a_changed_generation_policy_or_catalog_refuses() {
    let m = matrix();
    with_cut(&m, &[REFS, APP], |reads, cut| {
        let generation = edit(&cut, |v| v["members"][APP]["generation"] = 99.into());
        assert!(refusal(generation.admit(reads)).contains("generation-changed"));
        let policy = edit(&cut, |v| v["members"][APP]["policy"] = "policy-v2".into());
        assert!(refusal(policy.admit(reads)).contains("policy-changed"));
        let catalog = edit(&cut, |v| v["catalog_revision"] = 99.into());
        assert!(refusal(catalog.admit(reads)).contains("catalog-changed"));
    });
}

#[test]
fn the_canonical_bytes_are_pinned_with_sorted_keys() {
    // Keys deliberately out of order on input; the token must carry them
    // sorted at every level, independent of struct-field order.
    let cut: CompositionCut = serde_json::from_value(serde_json::json!({
        "version": 1,
        "members": {"urn:g:b": {
            "policy": "p1", "local_tx": 7, "store_id": "s1",
            "log_epoch": "e1", "generation": 2
        }},
        "catalog_revision": 3
    }))
    .unwrap();
    let want = r#"{"catalog_revision":3,"members":{"urn:g:b":{"generation":2,"local_tx":7,"log_epoch":"e1","policy":"p1","store_id":"s1"}},"version":1}"#;
    let token = cut.to_token();
    let body = base64::Engine::decode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        token.strip_prefix("qcut1.").unwrap(),
    )
    .unwrap();
    assert_eq!(String::from_utf8(body).unwrap(), want);
    assert_eq!(CompositionCut::from_token(&token).unwrap(), cut);
}

#[test]
fn a_cut_behind_head_is_admitted_but_not_at_head() {
    let m = matrix();
    with_cut(&m, &[REFS, APP], |reads, cut| {
        assert!(cut.is_at_head(reads));
        let tx = cut.members()[APP].local_tx;
        assert!(tx > 0, "matrix APP file has committed transactions");
        let behind = edit(&cut, |v| v["members"][APP]["local_tx"] = (tx - 1).into());
        behind.admit(reads).unwrap();
        assert!(
            !behind.is_at_head(reads),
            "reads answer at head, not at the cut"
        );
    });
}

#[test]
fn a_cut_ahead_of_a_files_head_refuses() {
    let m = matrix();
    with_cut(&m, &[REFS, APP], |reads, cut| {
        let ahead = edit(&cut, |v| {
            let tx = v["members"][APP]["local_tx"].as_i64().unwrap();
            v["members"][APP]["local_tx"] = (tx + 1).into();
        });
        assert!(refusal(ahead.admit(reads)).contains("ahead-of-head"));
    });
}

#[test]
fn an_unsupported_malformed_or_non_canonical_token_refuses() {
    let m = matrix();
    let cut = with_cut(&m, &[REFS, APP], |_, cut| cut);
    let token = cut.to_token();
    let json = |token: &str| {
        use base64::Engine as _;
        let body = token.strip_prefix("qcut1.").unwrap();
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(body)
            .unwrap()
    };
    let encode = |bytes: &[u8]| {
        use base64::Engine as _;
        format!(
            "qcut1.{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
        )
    };
    for (bad, why) in [
        ("qcut2.e30".to_owned(), "unsupported-version"),
        ("qcut1.!!".to_owned(), "malformed"),
        (encode(b"{\"version\":1}"), "malformed"),
        (
            encode(
                String::from_utf8(json(&token))
                    .unwrap()
                    .replace("\"version\":1", "\"version\":2")
                    .as_bytes(),
            ),
            "unsupported-version",
        ),
        (
            encode(
                String::from_utf8(json(&token))
                    .unwrap()
                    .replacen('{', "{\"extra\":1,", 1)
                    .as_bytes(),
            ),
            "malformed",
        ),
        // Same content, not canonical bytes: one cut has ONE token.
        (
            encode(&serde_json::to_vec_pretty(&cut).unwrap()),
            "non-canonical",
        ),
        (
            encode(&{
                let mut v = serde_json::to_value(&cut).unwrap();
                v["members"] = serde_json::json!({});
                serde_json::to_vec(&v).unwrap()
            }),
            "empty",
        ),
    ] {
        let err = CompositionCut::from_token(&bad).unwrap_err().to_string();
        assert!(err.contains(why), "{bad}: expected {why}, got {err}");
    }
}

#[test]
fn a_transaction_scalar_binds_to_one_file_and_refuses_across_files() {
    let m = matrix();
    // One file (the routed set file holds APP and PROV): a bound scalar admits.
    with_cut(&m, &[APP, PROV], |reads, cut| {
        let member = &cut.members()[APP];
        let scalar = BoundScalar {
            store_id: member.store_id.clone(),
            log_epoch: member.log_epoch.clone(),
            local_tx: member.local_tx,
        };
        scalar.admit(reads).unwrap();
        let foreign = BoundScalar {
            store_id: "urn:other-store".into(),
            ..scalar.clone()
        };
        assert!(refusal(foreign.admit(reads)).contains("foreign-store"));
        let epoch = BoundScalar {
            log_epoch: "urn:other-epoch".into(),
            ..scalar.clone()
        };
        assert!(refusal(epoch.admit(reads)).contains("foreign-epoch"));
        let ahead = BoundScalar {
            local_tx: scalar.local_tx + 1,
            ..scalar
        };
        assert!(refusal(ahead.admit(reads)).contains("ahead-of-head"));
    });
    // Two files: a scalar has no meaning, even one bound to a selected file.
    with_cut(&m, &[REFS, APP], |reads, cut| {
        let member = &cut.members()[APP];
        let scalar = BoundScalar {
            store_id: member.store_id.clone(),
            log_epoch: member.log_epoch.clone(),
            local_tx: member.local_tx,
        };
        let err = refusal(scalar.admit(reads));
        assert!(err.contains("scalar-spans-files"), "{err}");
        assert!(err.contains("composition cut"), "must ask for a cut: {err}");
    });
}

// PIN (existing behaviour, multi-db-composition.md §6): the graph-set
// composed store carries its attachments, so the store-level as_of_tx refusal
// applies to it. valid_at keeps composing (design: valid time is independent).
#[test]
fn as_of_tx_on_the_graph_set_composed_store_keeps_refusing_and_valid_at_composes() {
    let m = matrix();
    let query = format!(
        "SELECT ?v {} WHERE {{ GRAPH <{APP}> {{ <urn:right> <urn:value> ?v }} }}",
        named(&BOTH)
    );
    m.dataset
        .read_attached(&BOTH, deadline(), |reads, mut checkout| {
            let mut main_graphs = Vec::new();
            // No participant batch is pending, so no file has an S3
            // publication bound: every attached schema reads to its head.
            let mut attached: Vec<(String, Vec<i64>, Option<i64>)> = Vec::new();
            for read in reads {
                let id = if read.route.graph == ROOT_GRAPH_IRI {
                    0
                } else {
                    read.store.committed_graph_id(&read.route.graph)?.unwrap()
                };
                if read.legacy {
                    main_graphs.push(id);
                } else {
                    let schema = checkout
                        .schema_for(&read.route.store_id)
                        .unwrap()
                        .to_owned();
                    attached.push((schema, vec![id], None));
                }
            }
            checkout.compose(&main_graphs, &attached)?;
            let store = checkout.take_store();
            assert!(
                !store.attachments().is_empty(),
                "composed store lost its attachments"
            );
            let as_of = crate::sparql::TemporalContext {
                as_of_tx: Some(1),
                ..Default::default()
            };
            let err = crate::sparql::query_temporal(&store, &query, &as_of)
                .expect_err("as_of_tx answered across files")
                .to_string();
            assert!(
                err.contains("as_of_tx") && err.contains("file-local"),
                "{err}"
            );
            let valid = crate::sparql::TemporalContext {
                valid_at: Some("2100-01-01T00:00:00Z".into()),
                ..Default::default()
            };
            // valid_at is NOT a transaction scalar: across the two files it
            // composes, and it really filters the attached file's facts.
            let count = |ctx: &crate::sparql::TemporalContext| -> Result<usize> {
                match crate::sparql::query_temporal(&store, &query, ctx)? {
                    crate::sparql::QueryResult::Select { rows, .. } => Ok(rows.len()),
                    other => panic!("not a SELECT result: {other:?}"),
                }
            };
            assert_eq!(count(&Default::default())?, 1, "current read");
            assert_eq!(count(&valid)?, 1, "valid_at after the write composes");
            let before = crate::sparql::TemporalContext {
                valid_at: Some("1970-01-01T00:00:00Z".into()),
                ..Default::default()
            };
            assert_eq!(count(&before)?, 0, "valid_at before the write filters it");
            checkout.restore(store);
            Ok(())
        })
        .unwrap();
}
