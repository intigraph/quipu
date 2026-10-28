//! `GET /diff` and `POST /patch`: the store-wide change-set routes (aegis-4mk).
//!
//! Both speak `quipu-changeset` v1 (`quipu::changeset`, spec in
//! `docs/design/changeset-format.md`). `/patch` is idempotent: a change-set
//! whose post-state already holds writes nothing and says so; one whose
//! pre-state holds commits through the same gates as `/update`; anything else
//! is a 409 that writes nothing.

use std::collections::HashMap;

use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use quipu::changeset::{self, ChangeSet, Statement};
use quipu::changeset_store::{self, DiffSide, PatchState};

use super::{
    SharedStore,
    base::{AppError, blocking},
    update::{Applied, Attribution, UpdatePath, update_commit},
};

const DEFAULT_SOURCE: &str = "changeset-patch";

/// `GET /diff?from=&to=` — the change-set that turns `from` into `to`, as
/// SPARQL Update or (by `Accept`) RDF Patch.
pub(crate) async fn diff_get(
    State(store): State<SharedStore>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, AppError> {
    let side = |name: &str| -> Result<DiffSide, AppError> {
        let value = params.get(name).ok_or_else(|| {
            quipu::Error::InvalidValue(format!(
                "/diff requires {name}= (tx:N, main, or fork:<name>)"
            ))
        })?;
        Ok(DiffSide::parse(value)?)
    };
    let (from, to) = (side("from")?, side("to")?);
    let media = changeset::negotiate(headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()));
    let graph = params.get("graph").cloned();
    let limit = super::update::update_full::max_facts();
    blocking(move || {
        let store = store.read();
        Ok(diff_response(
            changeset_store::diff_bounded(&store, &from, &to, graph.as_deref(), limit)?,
            media,
        ))
    })
    .await
}

/// The `/diff` response: the change-set, or a 422 naming the side over the
/// ceiling (`server.update_full_copy_max_facts`, the same whole-store-copy
/// ceiling `/update` enforces), written before either side was read.
pub(crate) fn diff_response(
    outcome: Result<ChangeSet, changeset_store::DiffTooLarge>,
    media: &'static str,
) -> Response {
    match outcome {
        Ok(cs) => ([(header::CONTENT_TYPE, media)], cs.render(media)).into_response(),
        Err(too_large) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            axum::Json(serde_json::json!({
                "error": "diff_too_large",
                "side": too_large.side,
                "statements": too_large.statements,
                "limit": too_large.limit,
                "message": format!(
                    "side {} holds {} statements, over the server's limit of {}; nothing was \
                     read. Narrow it with graph=, diff a closer pair of transactions, or raise \
                     server.update_full_copy_max_facts",
                    too_large.side, too_large.statements, too_large.limit
                ),
            })),
        )
            .into_response(),
    }
}

fn nquads(statements: &[Statement]) -> Vec<String> {
    statements.iter().map(Statement::nquad).collect()
}

/// `POST /patch` — apply a change-set through the write gates, idempotently.
pub(crate) async fn patch_post(
    State(store): State<SharedStore>,
    headers: HeaderMap,
    Query(params): Query<Vec<(String, String)>>,
    body: Bytes,
) -> Result<Response, AppError> {
    let media = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map_or("", str::trim)
        .to_ascii_lowercase();
    if media != changeset::SPARQL_UPDATE && media != changeset::RDF_PATCH {
        return Ok((
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Content-Type must be application/sparql-update or application/rdf-patch",
        )
            .into_response());
    }
    let text = std::str::from_utf8(&body)
        .map_err(|e| quipu::Error::InvalidValue(format!("change-set body is not UTF-8: {e}")))?;
    let cs = ChangeSet::parse(text, &media)?;
    let attribution = match attribution(&params, &cs) {
        Ok(a) => a,
        Err(message) => return Ok((StatusCode::BAD_REQUEST, message).into_response()),
    };
    blocking(move || apply(&store, &cs, &attribution)).await
}

/// Declared `actor`/`source` query parameters, as `/update` takes them; an
/// absent `source` falls back to the change-set header's, then to
/// `changeset-patch`.
pub(crate) fn attribution(
    params: &[(String, String)],
    cs: &ChangeSet,
) -> Result<Attribution, String> {
    let mut out = Attribution::from_fields(params.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;
    if !params.iter().any(|(k, _)| k == "source") {
        out.source = cs
            .header
            .source
            .clone()
            .unwrap_or_else(|| DEFAULT_SOURCE.to_string());
    }
    Ok(out)
}

/// The locked half of `/patch`: classify, then apply or explain.
pub(crate) fn apply(
    shared: &SharedStore,
    cs: &ChangeSet,
    attribution: &Attribution,
) -> Result<Response, AppError> {
    let mut store = shared.lock();
    let digest = format!("sha256:{}", cs.digest());
    let head = store.latest_tx_id()?;
    let shapes_match = match &cs.header.shapes_hash {
        Some(declared) => Some(*declared == changeset_store::shapes_hash(&store, head)?),
        None => None,
    };
    match changeset_store::classify(&store, cs)? {
        PatchState::AlreadyApplied => Ok(axum::Json(serde_json::json!({
            "applied": false, "already_applied": true, "digest": digest,
            "tx": null, "asserted": 0, "retracted": 0, "graphs": [],
            "shapes_hash_matches": shapes_match,
        }))
        .into_response()),
        PatchState::Conflict {
            already_deleted,
            already_inserted,
        } => Ok((
            StatusCode::CONFLICT,
            axum::Json(serde_json::json!({
                "error": "conflict",
                "message": "the target matches neither the change-set's pre-state nor its \
                            post-state; nothing was written",
                "digest": digest,
                "base_tx": cs.header.base_tx,
                "already_deleted": nquads(&already_deleted),
                "already_inserted": nquads(&already_inserted),
            })),
        )
            .into_response()),
        PatchState::Applicable => {
            let to_quads = |side: &[Statement]| {
                side.iter()
                    .map(Statement::to_quad)
                    .collect::<quipu::Result<Vec<_>>>()
            };
            let (removed, asserted) = (to_quads(&cs.deletes)?, to_quads(&cs.inserts)?);
            let (changes, txs) = update_commit::commit_untimed(
                &mut store,
                removed.iter(),
                asserted.iter(),
                attribution,
                cs.header.valid_time.as_deref(),
            )?;
            let applied = Applied {
                path: UpdatePath::Sliced,
                changes,
                txs,
            };
            let mut report = applied.report(&store)?;
            let object = report.as_object_mut().expect("report is an object");
            object.insert("applied".into(), true.into());
            object.insert("already_applied".into(), false.into());
            object.insert("digest".into(), digest.into());
            object.insert(
                "shapes_hash_matches".into(),
                serde_json::json!(shapes_match),
            );
            Ok(axum::Json(report).into_response())
        }
    }
}

#[cfg(test)]
#[path = "patch_tests.rs"]
mod tests;
