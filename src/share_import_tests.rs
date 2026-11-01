#[cfg(test)]
mod tests {
    use super::*;

    const TS: &str = "2026-08-29T00:00:00Z";

    fn request(store: &Store) -> (tempfile::TempDir, ShareImportRequest) {
        let dir = tempfile::tempdir().unwrap();
        let share_dir = dir.path().join("share");
        crate::share::share(
            store,
            share_dir.to_str().unwrap(),
            &crate::share::ShareOptions {
                no_shapes: true,
                ..Default::default()
            },
        )
        .unwrap();
        let read = |name: &str| std::fs::read_to_string(share_dir.join(name)).unwrap();
        let request = ShareImportRequest {
            manifest: serde_json::from_str(&read("manifest.json")).unwrap(),
            export_ntriples: read("export.nt"),
            shapes_turtle: read("shapes.ttl"),
            alignments_turtle: None,
            queries_turtle: None,
            query_namespace: None,
            replace_queries: false,
            source: "https://example.org/alice/share".into(),
            actor: Some("alice".into()),
            accept_exact: false,
            destination: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            attestation: None,
        };
        (dir, request)
    }

    #[test]
    fn foreign_share_preserves_receivers_implicit_search_policy() {
        for (sender_policy, receiver_policy, receiver_scope) in [
            ("all", "legacy-root", "root"),
            ("legacy-root", "all", "all"),
        ] {
            let mut sender = Store::open_in_memory().unwrap();
            crate::share_scrub::seed_test_catalogue(&mut sender);
            sender
                .conn
                .execute(
                    "UPDATE search_default_policy SET policy=?1 WHERE id=1",
                    [sender_policy],
                )
                .unwrap();
            crate::rdf::ingest_rdf(
                &mut sender,
                &b"<urn:scope-transfer:positive> <urn:scope-transfer:p> \"carried\" .\n"[..],
                RdfFormat::NTriples,
                None,
                TS,
                None,
                None,
            )
            .unwrap();
            let (_dir, incoming) = request(&sender);
            let mut receiver = Store::open_in_memory().unwrap();
            crate::share_scrub::seed_test_catalogue(&mut receiver);
            receiver
                .conn
                .execute(
                    "UPDATE search_default_policy SET policy=?1 WHERE id=1",
                    [receiver_policy],
                )
                .unwrap();
            import_share(&mut receiver, &incoming, TS, None).unwrap();
            assert!(
                receiver
                    .lookup("urn:scope-transfer:positive")
                    .unwrap()
                    .is_some()
            );
            assert_eq!(
                receiver.default_search_graph_scope().unwrap().0,
                receiver_scope
            );
        }
    }

    /// aegis-i48b9w: an exact canonical-name match must be PROPOSED, not applied.
    ///
    /// Both arms in one test on purpose. A test that only proved the new default could
    /// not tell a working flag from a flag that is ignored — and the flag is the whole
    /// reversibility argument for landing this.
    #[test]
    fn exact_name_match_is_proposed_by_default_and_applied_only_on_request() {
        // Two stores that independently named an entity "Alice" — different IRIs, same
        // label. This is the collision the rewrite used to resolve silently.
        let plant = |iri: &str| {
            let mut st = Store::open_in_memory().unwrap();
            crate::share_scrub::seed_test_catalogue(&mut st);
            let nt = format!("<{iri}> <http://www.w3.org/2000/01/rdf-schema#label> \"Alice\" .\n");
            crate::rdf::ingest_rdf(
                &mut st,
                nt.as_bytes(),
                RdfFormat::NTriples,
                None,
                TS,
                None,
                Some("test"),
            )
            .unwrap();
            st
        };
        let source = plant("https://example.org/alice");
        let (_dir, mut request) = request(&source);

        // ARM 1 — default. The foreign IRI SURVIVES and the match is reported.
        let mut target = plant("https://local.example/alice");
        let staged = import_share(&mut target, &request, TS, Some("legacy-shared-bearer")).unwrap();
        let exact = &staged.resolution.exact_merges;
        assert_eq!(exact.len(), 1, "the exact match must still be REPORTED");
        assert_eq!(exact[0].foreign, "https://example.org/alice");
        assert_eq!(
            exact[0].score,
            Some(1.0),
            "reported with its score, for bulk-accept"
        );
        assert_eq!(exact[0].matched_on.as_deref(), Some("canonical_name:exact"));
        // `lookup` answers with the TERM ID for an IRI, so "is this IRI known to the
        // store" is exactly `is_some()`. (My first spelling formatted the returned i64
        // and searched it for the IRI — an assertion that could never pass, and would
        // have read as the fix not working.)
        assert!(
            target
                .lookup("https://example.org/alice")
                .unwrap()
                .is_some(),
            "the FOREIGN IRI must survive import — rewriting it destroys the identity \
             before anything can record it (aegis-i48b9w)"
        );

        // ARM 2 — opt in. Today's fast path is still reachable, explicitly.
        request.accept_exact = true;
        let mut target2 = plant("https://local.example/alice");
        import_share(&mut target2, &request, TS, Some("legacy-shared-bearer")).unwrap();
        assert!(
            target2
                .lookup("https://example.org/alice")
                .unwrap()
                .is_none(),
            "with accept_exact the foreign IRI IS rewritten away — otherwise the flag is \
             inert and arm 1 proves nothing"
        );
    }

    #[test]
    fn verified_share_stages_then_promotes_explicitly() {
        let mut source = Store::open_in_memory().unwrap();
        crate::share_scrub::seed_test_catalogue(&mut source);
        crate::rdf::ingest_rdf(
            &mut source,
            &b"<https://example.org/alice> <http://www.w3.org/2000/01/rdf-schema#label> \"Alice\" .\n"[..],
            RdfFormat::NTriples,
            None,
            TS,
            None,
            Some("test"),
        ).unwrap();
        let (_dir, request) = request(&source);
        let mut target = Store::open_in_memory().unwrap();
        crate::share_scrub::seed_test_catalogue(&mut target);
        let staged = import_share(&mut target, &request, TS, Some("legacy-shared-bearer")).unwrap();
        assert_eq!(staged.outcome, "staged");
        assert!(staged.promotion.eligible);
        assert!(target.lookup(&staged.staging_graph).unwrap().is_some());

        let promoted = promote_import(
            &mut target,
            &PromoteImportRequest {
                share_id: staged.share_id,
                actor: Some("reviewer".into()),
                target_graph: None,
                retire_from: None,
                dry_run: false,
            },
            TS,
            Some("legacy-shared-bearer"),
        )
        .unwrap();
        assert_eq!(promoted.outcome, "promoted");
        assert_eq!(promoted.triples, 1);
        let transactions = target.list_transactions().unwrap();
        let import_tx = transactions
            .iter()
            .find(|tx| {
                tx.source
                    .as_deref()
                    .is_some_and(|s| s.starts_with("share-import:"))
            })
            .unwrap();
        assert_eq!(import_tx.actor.as_deref(), Some("legacy-shared-bearer"));
        assert!(
            import_tx
                .source
                .as_deref()
                .unwrap()
                .contains("claimed-actor=alice")
        );
        let promotion_tx = transactions
            .iter()
            .find(|tx| {
                tx.source
                    .as_deref()
                    .is_some_and(|s| s.starts_with("share-promotion:"))
            })
            .unwrap();
        assert_eq!(promotion_tx.actor.as_deref(), Some("legacy-shared-bearer"));
        assert!(
            promotion_tx
                .source
                .as_deref()
                .unwrap()
                .contains("claimed-actor=reviewer")
        );
    }

    #[test]
    fn hash_mismatch_writes_nothing() {
        let mut source = Store::open_in_memory().unwrap();
        crate::share_scrub::seed_test_catalogue(&mut source);
        let (_dir, mut request) = request(&source);
        request.export_ntriples.push_str("# tampered\n");
        let mut target = Store::open_in_memory().unwrap();
        crate::share_scrub::seed_test_catalogue(&mut target);
        assert!(import_share(&mut target, &request, TS, None).is_err());
        assert!(
            target
                .lookup(&staging_graph(&request.manifest.share_id, false).unwrap())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn off_vocabulary_share_is_quarantined_and_cannot_promote() {
        let mut source = Store::open_in_memory().unwrap();
        crate::share_scrub::seed_test_catalogue(&mut source);
        crate::rdf::ingest_rdf(
            &mut source,
            &b"<https://example.org/alice> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://foreign.example/Unknown> .\n"[..],
            RdfFormat::NTriples,
            None,
            TS,
            None,
            Some("test"),
        ).unwrap();
        let (_dir, request) = request(&source);
        let mut target = Store::open_in_memory().unwrap();
        crate::share_scrub::seed_test_catalogue(&mut target);
        let result = import_share(&mut target, &request, TS, None).unwrap();
        assert_eq!(result.outcome, "quarantined");
        assert_eq!(result.triples.quarantined, 1);
        assert_eq!(result.promotion.blockers, vec!["off_vocabulary"]);
        assert!(
            promote_import(
                &mut target,
                &PromoteImportRequest {
                    share_id: result.share_id,
                    actor: None,
                    target_graph: None,
                    retire_from: None,
                    dry_run: false,
                },
                TS,
                None,
            )
            .is_err()
        );
    }

    /// A store that both carries the block-tier rule and violates it.
    fn leaky_store() -> Store {
        let mut store = Store::open_in_memory().unwrap();
        crate::rdf::ingest_rdf(
            &mut store,
            &br#"@prefix aegis: <http://aegis.gastown.local/ontology/> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
aegis:private-host-rule a aegis:InternalIdentifierPattern ;
    rdfs:label "private host" ;
    aegis:regex "private[.]example" ;
    aegis:enforcementTier "block" .
<urn:leak> <urn:p> "private.example" .
"#[..],
            oxrdfio::RdfFormat::Turtle,
            None,
            TS,
            None,
            None,
        )
        .unwrap();
        store
    }

    /// Build an import request from a share of `source`, produced internally.
    fn internal_request(source: &Store) -> (tempfile::TempDir, ShareImportRequest) {
        let dir = tempfile::tempdir().unwrap();
        let share_dir = dir.path().join("share");
        crate::share::share(
            source,
            share_dir.to_str().unwrap(),
            &crate::share::ShareOptions {
                no_shapes: true,
                destination: crate::share::ShareDestination::Internal,
                ..Default::default()
            },
        )
        .unwrap();
        let request = crate::share_transport::read_local(share_dir.to_str().unwrap()).unwrap();
        assert_eq!(
            request.manifest.destination,
            Some(crate::share::ShareDestination::Internal),
            "fixture must produce a stamped share, or the gate under test is untested"
        );
        (dir, request)
    }

    /// An untyped store holding only the leaking literal, so a receiver needs
    /// no vocabulary to stage it (and could promote it).
    fn leak_only_store() -> Store {
        let mut store = Store::open_in_memory().unwrap();
        crate::rdf::ingest_rdf(
            &mut store,
            &b"<urn:leak> <urn:p> \"private.example\" .\n"[..],
            RdfFormat::NTriples,
            None,
            TS,
            None,
            None,
        )
        .unwrap();
        store
    }

    /// A store carrying the block-tier rule and nothing that violates it.
    fn catalogue_only_store() -> Store {
        let mut store = Store::open_in_memory().unwrap();
        crate::rdf::ingest_rdf(
            &mut store,
            &br#"@prefix aegis: <http://aegis.gastown.local/ontology/> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
aegis:private-host-rule a aegis:InternalIdentifierPattern ;
    rdfs:label "private host" ;
    aegis:regex "private[.]example" ;
    aegis:enforcementTier "block" .
"#[..],
            oxrdfio::RdfFormat::Turtle,
            None,
            TS,
            None,
            None,
        )
        .unwrap();
        store
    }

    /// aegis-zev: the receiver reads the stamp. An internal share carrying an
    /// identifier the outward scrub would block imports with NO operator flag,
    /// and the import records that it was internal, on the result and on the
    /// staged review record.
    #[test]
    fn an_internal_share_imports_without_a_flag_and_records_its_stamp() {
        let (_dir, request) = internal_request(&leak_only_store());
        let mut store = catalogue_only_store();

        let result = import_share(&mut store, &request, TS, None).unwrap();
        assert_eq!(result.share_id, request.manifest.share_id);
        assert_eq!(result.outcome, "staged");
        assert_eq!(
            result.destination,
            Some(crate::share::ShareDestination::Internal),
            "the import must say it took an internal share"
        );
        let staged = store
            .events_after(0, 100, Some(&["import.staged".to_string()]), None)
            .unwrap();
        assert_eq!(staged.len(), 1);
        let payload: serde_json::Value = serde_json::from_str(&staged[0].payload).unwrap();
        assert_eq!(payload["destination"], "internal", "{payload}");
    }

    /// The marking still protects the way back out: once that internal data is
    /// promoted, an OUTWARD share of the receiving store runs the scrub and
    /// refuses. This is the guarantee the old import-side refusal stood in for.
    #[test]
    fn an_outward_share_of_imported_internal_data_is_still_scrubbed() {
        let (_dir, request) = internal_request(&leak_only_store());
        let mut store = catalogue_only_store();
        let out = tempfile::tempdir().unwrap();
        // CONTROL: before the import, this receiver shares outward cleanly, so
        // the refusal below is caused by the imported data.
        crate::share::share(
            &store,
            out.path().join("before").to_str().unwrap(),
            &crate::share::ShareOptions {
                no_shapes: true,
                ..Default::default()
            },
        )
        .unwrap();
        let staged = import_share(&mut store, &request, TS, None).unwrap();
        let promoted = promote_import(
            &mut store,
            &PromoteImportRequest {
                share_id: staged.share_id,
                actor: Some("reviewer".into()),
                target_graph: None,
                retire_from: None,
                dry_run: false,
            },
            TS,
            None,
        )
        .unwrap();
        assert_eq!(promoted.outcome, "promoted");

        let error = crate::share::share(
            &store,
            out.path().join("onward").to_str().unwrap(),
            &crate::share::ShareOptions {
                no_shapes: true,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("private host"), "{error}");

        // CONTROL: the same store shared internally is written, so the refusal
        // above is the scrub and not some other failure.
        crate::share::share(
            &store,
            out.path().join("internal").to_str().unwrap(),
            &crate::share::ShareOptions {
                no_shapes: true,
                destination: crate::share::ShareDestination::Internal,
                ..Default::default()
            },
        )
        .unwrap();
    }

    /// `[quipu.import] refuse_internal = true` refuses a stamped share before
    /// anything is staged, and names the setting.
    #[test]
    fn refuse_internal_config_refuses_a_stamped_share_before_staging() {
        let (_dir, request) = internal_request(&leak_only_store());
        let mut store = catalogue_only_store();
        store.import_policy_config_mut().refuse_internal = true;

        let error = import_share(&mut store, &request, TS, None).unwrap_err();
        assert!(error.to_string().contains("refuse_internal"), "{error}");
        assert!(error.to_string().contains("destination=internal"), "{error}");
        assert!(
            store.lookup(&request.manifest.share_id).unwrap().is_none(),
            "a refused import staged a graph"
        );
    }

    /// A clean internal share imports and is still recorded as internal: the
    /// stamp is the producer's statement, kept whatever the payload holds.
    #[test]
    fn an_internal_share_whose_payload_is_clean_imports_without_the_flag() {
        let mut clean = Store::open_in_memory().unwrap();
        crate::rdf::ingest_rdf(
            &mut clean,
            &b"<urn:ok> <urn:p> \"nothing internal here\" .\n"[..],
            RdfFormat::NTriples,
            None,
            TS,
            None,
            None,
        )
        .unwrap();
        let (_dir, request) = internal_request(&clean);

        let mut store = leaky_store();
        let result = import_share(&mut store, &request, TS, None).unwrap();
        assert_eq!(result.share_id, request.manifest.share_id);
        assert_eq!(result.destination, Some(crate::share::ShareDestination::Internal));
    }

    /// An UNSTAMPED share imports and records no stamp, even when it carries
    /// the pattern: import never scrubs. Stripping the stamp gains a producer
    /// nothing, because the outward scrub runs on the way back out regardless
    /// (see `an_outward_share_of_imported_internal_data_is_still_scrubbed`).
    #[test]
    fn an_unstamped_share_is_not_checked_even_when_it_carries_the_pattern() {
        let source = leaky_store();
        let dir = tempfile::tempdir().unwrap();
        let share_dir = dir.path().join("share");
        // Produced outward would be refused, so write the payload by hand with
        // no marker: this is precisely the pre-auw0o7 share, and the shape a
        // laundering attempt would take.
        let payload = crate::share::share_payload(
            &source,
            &crate::share::ShareOptions {
                no_shapes: true,
                destination: crate::share::ShareDestination::Internal,
                ..Default::default()
            },
            crate::share::SHARE_PAYLOAD_MAX_BYTES,
        )
        .unwrap();
        std::fs::create_dir_all(&share_dir).unwrap();
        let mut manifest = payload.manifest.clone();
        manifest.destination = None;
        // Re-derive the id, as a laundering producer would have to: the marker
        // is inside the hash, so it cannot merely be deleted.
        manifest.share_id =
            crate::share::sha256(&crate::share::manifest_bytes(&manifest, false).unwrap());
        std::fs::write(
            share_dir.join("manifest.json"),
            crate::share::manifest_bytes(&manifest, true).unwrap(),
        )
        .unwrap();
        for name in ["export.nt", "shapes.ttl"] {
            std::fs::write(share_dir.join(name), &payload.files[name]).unwrap();
        }

        let request = crate::share_transport::read_local(share_dir.to_str().unwrap()).unwrap();
        assert_eq!(request.manifest.destination, None);
        let mut store = leaky_store();
        let result = import_share(&mut store, &request, TS, None).unwrap();
        assert_eq!(result.destination, None);
    }

    /// aegis-1mv0to: a Warning-severity result must NOT quarantine a share.
    ///
    /// v0.10.0's repository share was quarantined whole (86,038 triples) by
    /// two untraced directives, because import gated on "any result" and an
    /// emit shape without `sh:severity` defaults to `sh:Violation`. Both arms in
    /// one test: the same untraced directive under the same shape is STAGED
    /// at `sh:Warning` and QUARANTINED at `sh:Violation`. The second arm is the
    /// control: it proves the import gate still blocks, so the first arm
    /// cannot pass because validation was skipped.
    ///
    /// The fixture copies the real shape's STRUCTURE (a node-level `sh:or`), on
    /// purpose: `sh:severity` on a node shape does not reach nested
    /// `sh:property` shapes, so a bare property fixture reports a violation
    /// either way.
    #[cfg(feature = "shacl")]
    #[test]
    fn warning_severity_does_not_quarantine_but_violation_still_does() {
        let shapes = |severity: &str| {
            format!(
                r#"@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix aegis: <http://aegis.gastown.local/ontology/> .
aegis:TraceShape a sh:NodeShape ;
    sh:targetClass aegis:Directive ;
    sh:severity sh:{severity} ;
    sh:or ( [ sh:property [ sh:path aegis:trackedBy ; sh:minCount 1 ] ] ) .
"#
            )
        };
        let mut source = Store::open_in_memory().unwrap();
        crate::share_scrub::seed_test_catalogue(&mut source);
        crate::rdf::ingest_rdf(
            &mut source,
            &b"<http://aegis.gastown.local/ontology/untraced> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://aegis.gastown.local/ontology/Directive> .\n"[..],
            RdfFormat::NTriples,
            None,
            TS,
            None,
            Some("test"),
        )
        .unwrap();
        let (_dir, request) = request(&source);
        let import_under = |severity: &str| {
            let mut target = Store::open_in_memory().unwrap();
            crate::share_scrub::seed_test_catalogue(&mut target);
            target.load_shapes("trace", &shapes(severity), TS).unwrap();
            import_share(&mut target, &request, TS, None).unwrap()
        };

        let warned = import_under("Warning");
        assert_eq!(
            warned.outcome, "staged",
            "a warning must stage: blockers {:?} report {}",
            warned.promotion.blockers, warned.validation.report
        );
        assert!(warned.promotion.blockers.is_empty());
        assert!(!warned.validation.blocking);
        assert!(!warned.validation.conforms, "sh:conforms stays spec-exact");
        let report = serde_json::to_string(&warned.validation.report).unwrap();
        assert!(
            report.contains("Warning"),
            "the warning must still be REPORTED, not dropped: {report}"
        );

        let violated = import_under("Violation");
        assert_eq!(violated.outcome, "quarantined");
        assert_eq!(violated.promotion.blockers, vec!["shacl_nonconforming"]);
    }
}
