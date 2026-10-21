//! Thin RPC adapters over the same core used by the local repository CLI.
use super::{
    SharedStore,
    base::{AppError, blocking},
};
use axum::{Extension, Json, extract::State};
use quipu::{config::MemoryFilesConfig, crew_credentials::CrewPrincipal, memory_files as memory};
use serde_json::{Value, json};

fn graph(store: &quipu::Store, config: &MemoryFilesConfig) -> quipu::Result<i64> {
    store
        .registered_graph_id(&config.graph)?
        .ok_or_else(|| quipu::Error::CannotVerify("file memory graph is not registered".into()))
}

pub(crate) async fn read(
    State(store): State<SharedStore>,
    Extension(config): Extension<MemoryFilesConfig>,
    Json(owner): Json<memory::Owner>,
) -> Result<Json<Value>, AppError> {
    config.check(&owner)?;
    blocking(move || {
        let store = store.read();
        let snapshot = memory::read(&store, graph(&store, &config)?, &owner)?;
        Ok(Json(
            json!({"coverage":"this_repository_owner_records","snapshot":snapshot}),
        ))
    })
    .await
}

pub(crate) async fn export(
    State(store): State<SharedStore>,
    Extension(config): Extension<MemoryFilesConfig>,
    Json(owner): Json<memory::Owner>,
) -> Result<Json<Value>, AppError> {
    config.check(&owner)?;
    blocking(move || {
        let store = store.read();
        Ok(Json(
            serde_json::to_value(memory::export(&store, graph(&store, &config)?, &owner)?)
                .map_err(|_| quipu::Error::Serialization("file memory export".into()))?,
        ))
    })
    .await
}

/// This response can feed own native staging; ordinary export stays shared.
pub(crate) async fn projection(
    State(store): State<SharedStore>,
    Extension(config): Extension<MemoryFilesConfig>,
    principal: Option<Extension<CrewPrincipal>>,
    Json(owner): Json<memory::Owner>,
) -> Result<Json<Value>, AppError> {
    config.check(&owner)?;
    let principal = principal
        .ok_or_else(|| {
            quipu::Error::PolicyDenied("named file projection identity required".into())
        })?
        .0;
    if principal.iri != owner.crew {
        return Err(
            quipu::Error::PolicyDenied("projection identity differs from owner".into()).into(),
        );
    }
    blocking(move || {
        let store = store.read();
        let export = memory::export(&store, graph(&store, &config)?, &owner)?;
        Ok(Json(
            json!({"authenticated_owner":principal.iri,"export":export}),
        ))
    })
    .await
}

pub(crate) async fn receipt(
    State(store): State<SharedStore>,
    Extension(config): Extension<MemoryFilesConfig>,
    principal: Option<Extension<CrewPrincipal>>,
    Json(input): Json<memory::Commit>,
) -> Result<Json<Value>, AppError> {
    config.check(&input.owner)?;
    let principal = principal
        .ok_or_else(|| quipu::Error::PolicyDenied("named file receipt identity required".into()))?
        .0;
    if principal.iri != input.owner.crew {
        return Err(
            quipu::Error::PolicyDenied("receipt identity differs from owner".into()).into(),
        );
    }
    blocking(move || {
        let store = store.read();
        let receipt = memory::receipt(
            &store,
            graph(&store, &config)?,
            &input,
            memory::Authority::Named(&principal),
        )?;
        Ok(Json(json!({"receipt":receipt})))
    })
    .await
}

pub(crate) async fn commit(
    State(store): State<SharedStore>,
    Extension(config): Extension<MemoryFilesConfig>,
    principal: Option<Extension<CrewPrincipal>>,
    Json(input): Json<memory::Commit>,
) -> Result<Json<Value>, AppError> {
    config.check(&input.owner)?;
    // Created by existing registry verification, never a caller-supplied actor.
    let principal = principal
        .ok_or_else(|| quipu::Error::PolicyDenied("named file memory writer required".into()))?
        .0;
    if principal.iri != input.owner.crew {
        return Err(
            quipu::Error::PolicyDenied("file memory writer differs from owner".into()).into(),
        );
    }
    blocking(move || {
        let mut store = store.lock();
        let graph = graph(&store, &config)?;
        let receipt = memory::commit(
            &mut store,
            graph,
            &input,
            memory::Authority::Named(&principal),
            &quipu::time::now_iso(),
        )?;
        memory::revision(&store, graph, &receipt)?;
        Ok(Json(json!({"receipt":receipt})))
    })
    .await
}

#[cfg(test)]
#[path = "memory_files/tests.rs"]
mod tests;
