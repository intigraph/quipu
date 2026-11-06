//! Authentication view for validated internal reader processes.
use super::ProcessPool;
use std::borrow::Cow;

impl ProcessPool {
    /// A validated internal reader serves reads, never primary replication transport.
    /// Keep replica/read-only identity and every other auth setting unchanged.
    pub(crate) fn auth_config<'a>(
        &self,
        config: &'a quipu::config::ServerConfig,
    ) -> Cow<'a, quipu::config::ServerConfig> {
        if self.internal_worker && config.replication_credentials_file.is_some() {
            let mut worker = config.clone();
            worker.replication_credentials_file = None;
            Cow::Owned(worker)
        } else {
            Cow::Borrowed(config)
        }
    }
}
