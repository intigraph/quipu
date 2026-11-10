//! Request-local strict admission. This tightens validation, never write authority.
use crate::{
    Store,
    episode::IngestResolutionOpts,
    error::{Error, Result},
};
use serde_json::Value;

pub(super) fn options(store: &Store, input: &Value) -> Result<IngestResolutionOpts> {
    let mut opts = IngestResolutionOpts::from_config(store.resolution_config());
    let strict = match input.get("strict_new_entities") {
        None => false,
        Some(value) => value
            .as_bool()
            .ok_or_else(|| Error::InvalidValue("strict_new_entities must be a boolean".into()))?,
    };
    if !strict {
        return Ok(opts);
    }
    if !crate::transaction_auth::current_identity().is_some_and(|identity| {
        !identity.principal.trim().is_empty()
            && matches!(
                identity.auth_class.as_str(),
                "named_bearer"
                    | "scoped_named_bearer"
                    | "legacy_shared_bearer"
                    | "attested_session"
            )
    }) {
        return Err(Error::InvalidValue(
            "strict_new_entities requires an authenticated request principal".into(),
        ));
    }
    let graph = input
        .get("graph")
        .and_then(Value::as_str)
        .filter(|graph| !graph.is_empty() && *graph != crate::schema::ROOT_GRAPH_IRI)
        .ok_or_else(|| {
            Error::InvalidValue(
                "strict_new_entities requires an explicit registered named graph".into(),
            )
        })?;
    let metadata = store.graph_metadata(graph)?.ok_or_else(|| {
        Error::InvalidValue(
            "strict_new_entities requires an existing registered committed graph".into(),
        )
    })?;
    if metadata["class"] != "committed"
        || !metadata["source"].is_null()
        || metadata["lifecycle"] == "frozen"
        || metadata["g"].as_i64().unwrap_or(0) <= 0
    {
        return Err(Error::InvalidValue(
            "strict_new_entities requires a local writable committed named graph".into(),
        ));
    }
    // Do not change Store configuration: another request may remain advisory.
    opts.enabled = true;
    opts.strict_mode = true;
    Ok(opts)
}
