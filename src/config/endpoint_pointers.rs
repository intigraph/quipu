//! Endpoint declaration resolution. Caboodle owns all endpoint trust policy.
use super::{FederationConfig, RemoteEndpoint};
use crate::{Error, Result};

impl FederationConfig {
    /// Resolve only the exact requested endpoint. Verification and current
    /// Caboodle consent precede every use; no endpoint or pack is fetched here.
    pub(crate) fn pointer_endpoint(&self, endpoint: &str) -> Result<Option<RemoteEndpoint>> {
        self.pointer_endpoint_with(endpoint, check_endpoint)
    }

    fn pointer_endpoint_with(
        &self,
        endpoint: &str,
        mut trust: impl FnMut(&str, &str) -> Result<()>,
    ) -> Result<Option<RemoteEndpoint>> {
        for pendant in &self.pendants {
            let context = |e| {
                Error::InvalidValue(format!(
                    "SERVICE pointer {endpoint} declared by {} in {}: {e}",
                    pendant.repo_remote,
                    pendant.path.display()
                ))
            };
            let path = pendant
                .path
                .to_str()
                .ok_or_else(|| context("non-UTF-8 local path".into()))?;
            let request =
                crate::share_transport::read_local(path).map_err(|e| context(e.to_string()))?;
            crate::share_import::verify_share(&request).map_err(|e| context(e.to_string()))?;
            for pointer in &request.manifest.pointers {
                match &pointer.target {
                    crate::share::PointerTarget::Endpoint { endpoint: declared }
                        if declared == endpoint =>
                    {
                        trust(&pendant.repo_remote, declared)
                            .map_err(|e| context(e.to_string()))?;
                        // No auth token or self-declared trust label crosses this boundary.
                        return Ok(Some(RemoteEndpoint::new(&pointer.graph, declared)));
                    }
                    _ => {}
                }
            }
        }
        Ok(None)
    }
}

fn check_endpoint(remote: &str, endpoint: &str) -> Result<()> {
    use std::process::{Command, Stdio};
    let error = |reason: &str| {
        Error::InvalidValue(format!(
            "caboodle endpoint trust {reason}: {endpoint} declared by {remote}"
        ))
    };
    let mut child = Command::new("caboodle")
        .args(["packs", "trust", "check-endpoint", remote, endpoint])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| error("unavailable"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return match status.code() {
                    Some(0) => Ok(()),
                    Some(3) => Err(error("refused")),
                    _ => Err(error("unavailable (check failed)")),
                };
            }
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error("did not complete"));
            }
        }
    }
}

#[cfg(test)]
mod tests;
