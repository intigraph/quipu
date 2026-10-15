//! Audit the real REST handlers and typed wire inputs behind native workflow tools.
use std::collections::BTreeSet;
use std::path::Path;

pub(super) fn handler(name: &str) -> Option<(&'static str, Option<(&'static str, &'static str)>)> {
    Some(match name {
        "quipu_share" => (
            "share_payload",
            Some(("src/share.rs", "SharePayloadRequest")),
        ),
        "quipu_import" => (
            "import_share",
            Some(("src/share_import.rs", "ShareImportRequest")),
        ),
        "quipu_import_promote" => (
            "promote_import",
            Some(("src/share_import.rs", "PromoteImportRequest")),
        ),
        "quipu_acquire" => ("acquire", Some(("src/acquisition.rs", "Request"))),
        "quipu_acquire_composed" => (
            "acquire_composed",
            Some(("src/acquisition/composed.rs", "Request")),
        ),
        "quipu_events" => ("events_get", Some(("src/server/feed.rs", "EventParams"))),
        "quipu_changes" => ("changes_get", Some(("src/server/feed.rs", "ChangeParams"))),
        "quipu_transactions" => (
            "transactions",
            Some(("src/server/feed.rs", "TransactionParams")),
        ),
        "quipu_entity" => (
            "entity_query_conneg",
            Some(("src/server/entity.rs", "EntityParams")),
        ),
        "quipu_fragments" => (
            "fragments_handler",
            Some(("src/server/entity.rs", "FragmentParams")),
        ),
        "quipu_knot_stage" => ("stage_snapshot_part", None),
        "quipu_knot_promote" => ("promote_snapshot_upload", None),
        "quipu_spotlight" => ("spotlight_handler", None),
        "quipu_reconcile" => ("reconcile_handler", None),
        "quipu_reason" => ("reason", None),
        "quipu_update" => ("update_post", None),
        "quipu_events_commit" => ("events_commit", None),
        "quipu_entity_history" => ("entity_history", None),
        "quipu_embed_backfill" => ("embed_backfill", None),
        "quipu_embed_backfill_graph" => ("embed_backfill_graph", None),
        "quipu_stats" => ("stats", None),
        "quipu_health" => ("health", None),
        "quipu_version" => ("version", None),
        "quipu_workflow_capabilities" => ("workflow_capabilities", None),
        _ => return None,
    })
}

pub(super) fn wire_fields(file: &str, name: &str) -> BTreeSet<String> {
    let source = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(file)).unwrap();
    let ast = syn::parse_file(&source).unwrap();
    let fields = ast
        .items
        .into_iter()
        .find_map(|item| match item {
            syn::Item::Struct(s) if s.ident == name => Some(s.fields),
            _ => None,
        })
        .expect("mapped wire struct exists");
    fields
        .into_iter()
        .filter_map(|field| {
            let mut name = field.ident.expect("named wire field").to_string();
            let mut skip = false;
            for attr in field.attrs.iter().filter(|a| a.path().is_ident("serde")) {
                let metas = attr
                    .parse_args_with(
                        syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
                    )
                    .unwrap();
                for meta in metas {
                    match meta {
                        syn::Meta::Path(p)
                            if p.is_ident("skip") || p.is_ident("skip_deserializing") =>
                        {
                            skip = true
                        }
                        syn::Meta::NameValue(n) if n.path.is_ident("rename") => {
                            if let syn::Expr::Lit(syn::ExprLit {
                                lit: syn::Lit::Str(s),
                                ..
                            }) = n.value
                            {
                                name = s.value();
                            }
                        }
                        _ => {}
                    }
                }
            }
            (!skip).then_some(name)
        })
        .collect()
}

#[test]
fn wire_audit_keeps_serde_names_and_excludes_non_wire_authority_fields() {
    let fragments = wire_fields("src/server/entity.rs", "FragmentParams");
    assert!(fragments.contains("pageSize"));
    assert!(!fragments.contains("page_size"));
    let import = wire_fields("src/share_import.rs", "ShareImportRequest");
    assert!(import.contains("manifest") && import.contains("source"));
    assert!(!import.contains("destination"));
}
