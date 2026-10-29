//! The MCP adapter owns the terminal outcome after body parsing/projection.
use super::{CallToolResult, ErrorData, HeaderMap, Value};
use crate::search_outcome::{
    Outcome,
    observation::{Call, Surface, emit, for_route},
};
use std::sync::Arc;

impl super::McpServer {
    pub(super) async fn dispatch(
        &self,
        name: &str,
        input: Value,
        headers: &HeaderMap,
    ) -> Result<CallToolResult, ErrorData> {
        let mut observation = Observation::new(name, &input, headers);
        let result = self
            .dispatch_observed(name, input, headers, &observation)
            .await;
        observation.finish(
            result
                .as_ref()
                .map_or(true, |value| value.is_error == Some(true)),
        );
        result
    }
}

pub(super) struct Observation {
    pub call: Option<Arc<Call>>,
    done: bool,
}

impl Observation {
    pub fn new(name: &str, input: &serde_json::Value, headers: &axum::http::HeaderMap) -> Self {
        let (_, route) = super::endpoint(name);
        let call = for_route(&route).map(|operation| Call::new(Surface::Mcp, operation));
        if let Some(call) = &call {
            call.input(input);
            let header = |name| headers.get(name).and_then(|value| value.to_str().ok());
            call.declare(&[
                ("agent", header("x-quipu-agent")),
                ("session", header("x-quipu-session")),
                ("harness", header("x-quipu-harness")),
                ("host", header("x-quipu-host")),
                ("model", header("x-quipu-model")),
                ("task", header("x-quipu-task")),
                ("client", header("x-quipu-client")),
                ("purpose", header("x-quipu-purpose")),
            ]);
        }
        Self { call, done: false }
    }
    pub fn finish(&mut self, failed: bool) {
        self.done = true;
        if let Some(call) = &self.call
            && let Some(event) = call.finish_mcp(failed.then_some(Outcome::Error))
        {
            emit(&event);
        }
    }
}

impl Drop for Observation {
    fn drop(&mut self) {
        if !self.done
            && let Some(call) = &self.call
            && let Some(event) = call.finish_mcp(Some(Outcome::Unknown))
        {
            emit(&event);
        }
    }
}
