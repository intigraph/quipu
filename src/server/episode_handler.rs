//! Episode HTTP admission, resolution and scoped commit.
use super::{
    AppError, JsonValue, SharedStore, State, episode_read_blocking_until, finish_deferred_embed,
    write_blocking_until,
};

#[cfg(test)]
#[path = "episode_strict_tests.rs"]
mod strict_tests;

/// `/episode` in two phases (aegis-xi0).
///
/// Non-strict entity resolution is advisory, and it is the expensive part of an
/// episode: one embedding and one label scan per node. Run under the writer, a
/// 57-node episode on a production-size store spent its whole budget holding
/// the single writer and was then refused. So resolution runs first on a READ
/// connection, and the writer is taken only to revalidate it against what
/// committed in between and to commit, exactly as [`rw_handler`] does.
///
/// One deadline, stamped here, covers both admissions. Where two phases do not
/// apply (strict resolution, no read pool, a reader whose frozen layers cannot
/// be synced) the request takes the single-phase path unchanged.
pub(crate) async fn episode(
    State(s): State<SharedStore>,
    axum::Json(i): axum::Json<JsonValue>,
) -> Result<axum::Json<JsonValue>, AppError> {
    let (started, deadline) = super::super::admission::episode_budget();
    let prepared = if !super::super::auth::request_identity()
        .is_some_and(|i| i.auth_class == "scoped_named_bearer")
        && s.resolves_off_writer()
    {
        let (reader_store, input) = (s.clone(), i.clone());
        episode_read_blocking_until(started, deadline, move || {
            let mut reader = reader_store.vector_read();
            // `read()` reports a failed frozen-pack sync and carries on with
            // the archives absent. Resolution must not: a duplicate in a
            // frozen layer would go unreported. Fall back to the writer.
            if reader.sync_frozen_attachments().is_err() {
                return Ok(None);
            }
            Ok(quipu::tool_episode_prepare(&reader, &input)?)
        })
        .await?
    } else {
        None
    };
    write_blocking_until(std::time::Instant::now(), deadline, move || {
        let (out, work) = {
            let mut st = s.write_lock()?;
            quipu::transaction_auth::refuse_if_refused()?;
            let targets = super::super::update::writer_targets::json(&i);
            let out = st.with_writer_scope(
                quipu::writer_scope::WriteOperation::Episode,
                &targets,
                |st| match prepared {
                    Some(prepared) => quipu::tool_episode_commit(st, prepared),
                    None => quipu::tool_episode(st, &i),
                },
            )?;
            (
                out,
                if quipu::writer_scope::is_scoped_request() {
                    let _ = st.take_deferred_embed();
                    None
                } else {
                    st.take_deferred_embed()
                },
            )
        };
        if let Some(work) = work {
            finish_deferred_embed(&s, &work)?;
        }
        Ok(axum::Json(super::super::input_fields::annotate(
            stringify!(quipu::tool_episode),
            &i,
            out,
        )))
    })
    .await
}
