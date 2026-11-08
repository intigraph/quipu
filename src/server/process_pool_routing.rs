//! Two bounded forwarding lanes, with one service-wide owner-row executor.
use super::{Ordering, Routing, Worker};
use tokio::sync::OwnedSemaphorePermit;

pub(super) fn hop_header(name: &str) -> bool {
    matches!(
        name,
        "host"
            | "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "content-length"
    )
}

#[cfg(test)]
#[path = "process_pool_routing_tests.rs"]
mod tests;

impl Routing {
    pub(super) fn heavy(&self, path: &str, operation: Option<&str>) -> bool {
        self.heavy_permits > 0 && path == "/query" && operation == Some("owner-rows")
    }

    pub(super) fn select(&self, heavy: bool) -> Option<(&Worker, OwnedSemaphorePermit)> {
        if heavy {
            // Never multiply K by the worker count. The first worker's existing
            // task-owned heavy cap enforces K even after a proxy timeout while
            // its blocking query is still alive. No failover or writer fallback.
            let worker = self.workers.first()?;
            return worker
                .heavy_available
                .clone()
                .try_acquire_owned()
                .ok()
                .map(|permit| (worker, permit));
        }
        // Heavy HTTP waiters never reserve this lane. Its single forwarded
        // ordinary request can use the reader left free by K < worker readers.
        let start = self.next.fetch_add(1, Ordering::Relaxed);
        (0..self.workers.len()).find_map(|offset| {
            let worker = &self.workers[start.wrapping_add(offset) % self.workers.len()];
            worker
                .available
                .clone()
                .try_acquire_owned()
                .ok()
                .map(|permit| (worker, permit))
        })
    }
}
