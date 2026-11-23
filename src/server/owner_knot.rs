//! Inert server boundary for one already admitted pilot. No startup, HTTP
//! advertisement, file discovery or legacy fallback is installed here.
#![cfg_attr(not(test), allow(dead_code))]
use crate::{
    admission::owner::{OwnerWriteAdmission, read_now},
    base::AppError,
};
use quipu::{graph_catalog::WriteRequest, store::graph_router::Router};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// Trusted request pins, not authentication or writer authority.
#[derive(Clone)]
pub(crate) struct Pin {
    pub catalog_revision: u64,
    pub generation: u64,
    pub policy: String,
    pub signed: bool,
    pub cross_graph_context: bool,
}

/// Consume the sole admitted Router so this boundary owns its queue budget.
/// A future initializer must retain one shared Arc, never duplicate queues.
pub(crate) struct PreparedOwner {
    router: Router,
    graph: String,
    writes: Arc<OwnerWriteAdmission>,
    readers: Arc<Semaphore>,
}

fn refuse(message: &str) -> AppError {
    quipu::Error::PolicyDenied(format!("owner transport: {message}")).into()
}

impl PreparedOwner {
    pub(crate) fn new(router: Router, graph: String) -> Result<Arc<Self>, AppError> {
        if router.single_routed_graph() != Some(graph.as_str()) {
            return Err(refuse("one explicitly selected admitted graph required"));
        }
        let owner = router
            .owner(&graph)
            .ok_or_else(|| refuse("owner missing"))?;
        let writes = Arc::new(OwnerWriteAdmission::new(owner.queued_per_owner())?);
        let readers = Arc::new(Semaphore::new(owner.read_pool_size()));
        Ok(Arc::new(Self {
            router,
            graph,
            writes,
            readers,
        }))
    }

    fn request<'a>(&'a self, pin: &'a Pin) -> WriteRequest<'a> {
        WriteRequest {
            graph: Some(&self.graph),
            catalog_revision: pin.catalog_revision,
            owner_generation: Some(pin.generation),
            policy: &pin.policy,
            signed: pin.signed,
            cross_graph_context: pin.cross_graph_context,
        }
    }

    fn preflight(&self, pin: &Pin, input: &Value, id: &str) -> Result<(), AppError> {
        let route = self
            .router
            .owner(&self.graph)
            .ok_or_else(|| refuse("owner missing"))?
            .catalog_route();
        if pin.catalog_revision != self.router.catalog_revision()
            || pin.generation != route.generation
            || pin.policy != route.policy
            || pin.signed
            || pin.cross_graph_context
            || input.get("graph").and_then(Value::as_str) != Some(self.graph.as_str())
        {
            return Err(refuse("request is outside the admitted owner contract"));
        }
        if id.len() != 64
            || !id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(refuse("one lowercase SHA256-sized write ID required"));
        }
        let identity = crate::auth::request_identity()
            .ok_or_else(|| refuse("authenticated owner required"))?;
        if identity.principal.is_empty()
            || identity.credential_id.as_ref().is_none_or(String::is_empty)
            || identity.auth_class.is_empty()
            || identity.auth_class == "scoped_named_bearer"
        {
            return Err(refuse("stable unscoped durable credential required"));
        }
        Ok(())
    }

    pub(crate) async fn knot(
        self: &Arc<Self>,
        pin: Pin,
        input: Value,
        id: String,
    ) -> Result<Value, AppError> {
        self.preflight(&pin, &input, &id)?;
        let owner = self.clone();
        self.writes
            .write(move || {
                Ok(owner
                    .router
                    .owner(&owner.graph)
                    .ok_or_else(|| refuse("owner missing"))?
                    .knot_durable(&owner.request(&pin), &input, &id)?)
            })
            .await
    }

    pub(crate) async fn status(
        self: &Arc<Self>,
        pin: Pin,
        input: Value,
        id: String,
    ) -> Result<Value, AppError> {
        self.preflight(&pin, &input, &id)?;
        let owner = self.clone();
        read_now(self.readers.clone(), move || {
            Ok(owner
                .router
                .owner(&owner.graph)
                .ok_or_else(|| refuse("owner missing"))?
                .knot_attempt_status(&owner.request(&pin), &input, &id)?)
        })
        .await
    }

    pub(crate) async fn seal(
        self: &Arc<Self>,
        pin: Pin,
        input: Value,
        id: String,
    ) -> Result<Value, AppError> {
        self.preflight(&pin, &input, &id)?;
        let owner = self.clone();
        self.writes
            .write(move || {
                Ok(owner
                    .router
                    .owner(&owner.graph)
                    .ok_or_else(|| refuse("owner missing"))?
                    .seal_knot_binding(&owner.request(&pin), &input, &id)?)
            })
            .await
    }
}

#[cfg(test)]
#[path = "owner_knot_tests.rs"]
mod tests;
