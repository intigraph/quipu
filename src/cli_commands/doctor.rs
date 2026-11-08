/// `quipu doctor labels` — recompute every graph's label from the meta-graph
/// facts and report where the `graphs` cache disagrees (quipu #65).
///
/// RDF is the source of truth. A non-empty report means the CACHE is wrong,
/// never the facts — so this reports rather than repairs, and says which side
/// is authoritative in its own output. Exits non-zero on drift so a cron or CI
/// caller can gate on it without parsing the text.
pub fn cmd_doctor(args: &[String], db_path: &str) {
    let sub = args.get(2).map_or("labels", String::as_str);
    if sub == "search" {
        use quipu::vector::KnowledgeVectorStore;
        let store = crate::cli_open::open_store(db_path);
        let progress = store.lexical_progress().unwrap_or_else(|error| {
            eprintln!("doctor search: {error}");
            std::process::exit(1);
        });
        let keyword_ready =
            store.search_config().keyword && progress.as_ref().is_some_and(|p| p.complete);
        let vector_rows = store.vector_count().unwrap_or_else(|error| {
            eprintln!("doctor search vector status: {error}");
            std::process::exit(1);
        });
        // Inspect the bootstrap selected by provider setup. Diagnostics must not
        // initialize ORT, acquire a GPU candidate, or change device selection.
        #[cfg(feature = "onnx")]
        let runtime = quipu::onnx_runtime::current().map_or_else(
            || {
                serde_json::json!({
                    "available": false, "linkage": null, "path": null,
                    "state": "not_initialized"
                })
            },
            |info| {
                serde_json::json!({
                    "available": info.available, "linkage": info.linkage,
                    "path": info.path, "state": "initialized"
                })
            },
        );
        #[cfg(not(feature = "onnx"))]
        let runtime = serde_json::json!({
            "available": false, "linkage": null, "path": null,
            "state": "not_compiled"
        });
        println!(
            "{}",
            serde_json::json!({
                "configured_mode": store.search_config().mode,
                "keyword_ready": keyword_ready,
                "keyword_index": progress,
                "keyword_remedy": if !store.search_config().keyword {
                    Some("set [quipu.search] keyword = true")
                } else if !keyword_ready {
                    Some("run quipu search-index backfill --batch-size 500 repeatedly until complete")
                } else { None },
                "semantic_provider_ready": store.has_embedding_provider(),
                "vector_rows": vector_rows,
                "semantic_ready": store.has_embedding_provider() && vector_rows > 0,
                "onnx_compiled": cfg!(feature = "onnx"),
                "onnx_runtime": runtime,
                "semantic_remedy": if !store.has_embedding_provider() {
                    Some("run quipu model install in a full build, or configure [quipu.embedding] model_path and tokenizer_path")
                } else if vector_rows == 0 {
                    Some("ingest new data or run bounded quipu model backfill for existing ROOT subjects")
                } else { None }
            })
        );
        return;
    }
    if sub != "labels" {
        eprintln!("usage: quipu doctor labels|search [--db <path>]");
        std::process::exit(1);
    }

    let store = crate::cli_open::open_store(db_path);

    // quipu #80: surface producers' RECOMMENDED floors here. They are advisory
    // — the store never applies them — so they are reported beside the drift
    // report rather than mixed into it, and the banner says so on every line.
    // (#75's attach path is the other intended print point; `RecommendedFloor::line`
    // is what it will call.)
    if let Ok(graphs) = store.all_named_graph_ids() {
        let mut shown = false;
        for g in graphs {
            let Ok(iri) = store.resolve(g) else { continue };
            let Ok(rec) = store.recommended_floor(&iri) else {
                continue;
            };
            if !rec.is_empty() {
                if !shown {
                    println!("recommended floors (advisory — NOT enforced):");
                    shown = true;
                }
                println!("  {}", rec.line(&iri));
            }
        }
        if shown {
            println!();
        }
    }

    match store.graph_label_drift() {
        Ok(drift) if drift.is_empty() => {
            println!("labels: no drift — every cached label agrees with the meta-graph");
        }
        Ok(drift) => {
            println!(
                "labels: {} disagreement(s) between the meta-graph (authoritative) \
                 and the graphs cache\n",
                drift.len()
            );
            for d in &drift {
                println!("  {}", d.graph_iri);
                println!("    axis:   {}", d.axis);
                println!("    rdf:    {}   <- authoritative", d.rdf);
                println!("    cached: {}", d.cached);
            }
            println!(
                "\nThe cache is derived; the facts are the truth. Re-declare these \
                 graphs' labels with set_graph_label to rebuild the cache."
            );
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("doctor error: {e}");
            std::process::exit(1);
        }
    }
}
