//! Scratch-only share-reconnect adapter (`share_merge::{merge}` and the
//! decisions file `emit` / `apply`). Input recipes contain no expected answers.
//!
//! Two private stores: a source that publishes the base share and, after its
//! own edits, the incoming share; and ROOT, which holds the base and then the
//! ours edits. The merge runs against ROOT exactly as `quipu share merge` does.
use super::*;
use quipu::share::{ShareOptions, share};
use quipu::share_merge::merge;
use quipu::share_merge_decisions::{Decision, Side, apply, emit};
use quipu::share_scrub::ShareDestination;

fn options(parent: Option<String>) -> ShareOptions {
    ShareOptions {
        shapes: vec!["oracle-input".into()],
        // Run-scoped scratch bundles; they never leave this process.
        destination: ShareDestination::Internal,
        parent_share: parent,
        ..Default::default()
    }
}

/// The operator's decision for the single conflicting row, from explicit input.
fn decision(request: &Request) -> Res<Option<Decision>> {
    let choose = |choose| Ok(Some(Decision::Choose { choose }));
    match request.decision.as_deref() {
        None | Some("defer") => {
            if request.resolution.is_some() {
                return Err("share recipe: resolution needs decision \"value\"".into());
            }
            Ok(None)
        }
        Some("continue") => choose(Side::Continue),
        Some("ours") => choose(Side::Ours),
        Some("theirs") => choose(Side::Theirs),
        Some("base") => choose(Side::Base),
        Some("both") => choose(Side::Both),
        Some("value") => {
            let values = request
                .resolution
                .as_ref()
                .ok_or("share recipe: decision \"value\" needs resolution values")?;
            Ok(Some(Decision::Values {
                values: values
                    .iter()
                    .map(|v| oxrdf::Literal::new_simple_literal(v.clone()).to_string())
                    .collect(),
            }))
        }
        Some(_) => Err("share recipe: unknown decision".into()),
    }
}

pub(super) fn run(request: &Request) -> Res<serde_json::Value> {
    let chosen = decision(request)?;
    let scratch = tempfile::tempdir()?;
    let shapes = shapes_ttl(request.max_count);

    let shares = scratch.path().join("shares");
    let incoming = shares.join("incoming");
    {
        let source_db = scratch.path().join("source.db");
        let mut source = Store::open(source_db.to_str().ok_or("scratch path encoding")?)?;
        source.load_shapes("oracle-input", &shapes, &request.base.at)?;
        let (e, a) = (source.intern(SUBJECT)?, source.intern(PREDICATE)?);
        write(&mut source, &request.base, 0, e, a)?;
        let base = share(
            &source,
            shares
                .join("base")
                .to_str()
                .ok_or("scratch path encoding")?,
            &options(None),
        )?;
        for step in &request.theirs {
            write(&mut source, step, 0, e, a)?;
        }
        share(
            &source,
            incoming.to_str().ok_or("scratch path encoding")?,
            &options(Some(base.share_id)),
        )?;
    }

    let db = scratch.path().join("fixture.db");
    let mut store = Store::open(db.to_str().ok_or("scratch path encoding")?)?;
    store.load_shapes("oracle-input", &shapes, &request.base.at)?;
    let (e, a) = (store.intern(SUBJECT)?, store.intern(PREDICATE)?);
    write(&mut store, &request.base, 0, e, a)?;
    let base_tx = store.latest_tx_id()?;
    for step in &request.ours {
        write(&mut store, step, 0, e, a)?;
    }
    let before = store.latest_tx_id()?;

    let mut file = emit(&store, &incoming)?;
    let conflicts: Vec<_> = file.rows.iter().map(|r| r.record.clone()).collect();
    let native = if file.rows.is_empty() {
        if chosen.is_some() {
            return Err("explicit decision needs exactly one native conflict".into());
        }
        match merge(
            &mut store,
            &incoming,
            &request.decision_at,
            Some("oracle-input"),
        ) {
            Ok(r) => {
                json!({"outcome":if r.outcome == "merged" {"promoted"} else {r.outcome.as_str()},
                "native_outcome":r.outcome,"tx":r.tx_id,"asserted":r.asserted,"retracted":r.retracted})
            }
            Err(error) => json!({"outcome":"error","error":error.to_string()}),
        }
    } else if file.rows.len() != 1 {
        return Err("share recipe: expected at most one conflicting slot".into());
    } else if let Some(choice) = chosen {
        file.rows[0].decision = Some(choice);
        let bytes = serde_json::to_vec(&file)?;
        match apply(
            &mut store,
            &incoming,
            &file,
            &bytes,
            "oracle-input",
            &request.decision_at,
            Some("oracle-input"),
        ) {
            Ok(d) => json!({"outcome":"promoted","native_outcome":d.merge.outcome,
                "tx":d.merge.tx_id,"asserted":d.merge.asserted,"retracted":d.merge.retracted}),
            Err(error) => json!({"outcome":"error","error":error.to_string()}),
        }
    } else {
        // Undecided: the decisions file is the held state; nothing is applied.
        json!({"outcome":"conflicts","held":file.rows.len()})
    };
    let after = store.latest_tx_id()?;
    let mut observation = observe::capture(
        &store,
        &db,
        e,
        a,
        before,
        after,
        base_tx,
        &request.probes,
        observe::Writer::Share,
    )?;
    let decided = request.resolution.clone().or_else(|| {
        let record = conflicts.first()?;
        let unquote = |v: &[String]| {
            v.iter()
                .map(|s| s.trim_matches('"').to_string())
                .collect::<Vec<_>>()
        };
        match request.decision.as_deref()? {
            "ours" => Some(unquote(&record.ours)),
            "theirs" => Some(unquote(&record.theirs)),
            "base" => Some(unquote(&record.base)),
            _ => None,
        }
    });
    observation["report"] = report(&conflicts, &native, decided.as_ref());
    let capture = json!({"schema":"quipu-native-capture/v1","case":request.case,
        "path":"share","native":native,"conflicts":conflicts,
        "before_tx":before,"after_tx":after,"capture":observation});
    drop(store);
    scratch.close()?;
    Ok(capture)
}
