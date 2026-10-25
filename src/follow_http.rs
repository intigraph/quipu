//! The primary over HTTP, for [`crate::follow`].
//!
//! Every request carries `X-Quipu-Client: replica-follower` and follows no
//! redirects. A page read is bounded in time; the stream's read timeout is three
//! heartbeats, so a silent connection is dropped and reopened from
//! the cursor rather than held forever.

use std::io::{BufRead, BufReader};
use std::time::Duration;

use crate::follow::{CLIENT, Feed};

/// The stream's heartbeat is 15 s; three missed beats mean the connection is gone.
const STREAM_IDLE: Duration = Duration::from_secs(45);
const PAGE_TIMEOUT: Duration = Duration::from_secs(60);

/// A primary at `base` (no trailing slash needed).
pub struct Http {
    base: String,
    agent: ureq::Agent,
    stream_agent: ureq::Agent,
}

impl Http {
    /// A feed from the primary at `base`.
    #[must_use]
    pub fn new(base: &str) -> Self {
        let build = |read: Duration| {
            ureq::AgentBuilder::new()
                .redirects(0)
                .timeout_connect(Duration::from_secs(5))
                .timeout_read(read)
                .build()
        };
        Self {
            base: base.trim_end_matches('/').to_string(),
            agent: build(PAGE_TIMEOUT),
            stream_agent: build(STREAM_IDLE),
        }
    }
}

impl Http {
    /// The primary's graphs frozen into packs. The change feed does not carry
    /// them, so a follower does not hold them; the run states how many.
    ///
    /// # Errors
    /// A transport failure or an unreadable response: the count is UNKNOWN.
    pub fn frozen_graphs(&self) -> std::io::Result<Vec<String>> {
        let body = self
            .agent
            .get(&format!("{}/graphs", self.base))
            .set("X-Quipu-Client", CLIENT)
            .call()
            .map_err(transport)?
            .into_string()?;
        let graphs: serde_json::Value =
            serde_json::from_str(&body).map_err(std::io::Error::other)?;
        let list = graphs.get("graphs").unwrap_or(&graphs);
        let list = list
            .as_array()
            .ok_or_else(|| std::io::Error::other("/graphs is not a list"))?;
        Ok(list
            .iter()
            .filter(|g| g["lifecycle"] == "frozen")
            .filter_map(|g| g["iri"].as_str().map(str::to_string))
            .collect())
    }
}

fn transport(error: ureq::Error) -> std::io::Error {
    std::io::Error::other(error.to_string())
}

impl Feed for Http {
    fn page(&mut self, since: i64, limit: usize) -> std::io::Result<String> {
        self.agent
            .get(&format!("{}/changes", self.base))
            .query("since", &since.to_string())
            .query("limit", &limit.to_string())
            .query("capture", "old_and_new_values")
            .set("X-Quipu-Client", CLIENT)
            .call()
            .map_err(transport)?
            .into_string()
    }

    fn stream(&mut self, since: i64) -> std::io::Result<Box<dyn BufRead>> {
        let response = self
            .stream_agent
            .get(&format!("{}/changes/stream", self.base))
            .query("since", &since.to_string())
            .set("Accept", "text/event-stream")
            .set("X-Quipu-Client", CLIENT)
            .call()
            .map_err(transport)?;
        Ok(Box::new(BufReader::new(response.into_reader())))
    }
}
