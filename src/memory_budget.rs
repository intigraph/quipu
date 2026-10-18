//! A conservative SQLite allocation from one aggregate service budget.
//!
//! Half is reserved for evaluator heaps, models and runtime overhead. Cache and
//! mmap receive a quarter each, divided across every connection in every process.
//! These are sizing ceilings, not a bound on total RSS; the encompassing service
//! cgroup must enforce the same aggregate budget.

use crate::{Error, Result, config::ServerConfig};

/// Per-connection allocation, with the aggregate limit for service managers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryAllocation {
    /// Aggregate bytes, including the writer and all workers.
    pub total_bytes: u64,
    /// Number of SQLite connections across the complete service.
    pub connections: u64,
    /// Per-connection private SQLite cache ceiling in KiB.
    pub cache_kib: u64,
    /// Per-connection mmap address-space ceiling in bytes.
    pub mmap_bytes: u64,
}

impl MemoryAllocation {
    /// Validate before opening a database or spawning any process.
    pub fn for_server(config: &ServerConfig) -> Result<Option<Self>> {
        let Some(mib) = config.memory_budget_mib else {
            if config.read_processes != 0 {
                return Err(Error::InvalidValue(
                    "read_processes requires memory_budget_mib".into(),
                ));
            }
            return Ok(None);
        };
        if config.read_processes > 32 || config.read_pool_size > 256 {
            return Err(Error::InvalidValue(
                "reader count exceeds supported bound".into(),
            ));
        }
        let readers = if config.read_processes == 0 {
            config.read_pool_size
        } else {
            config.read_processes
        };
        let connections = u64::try_from(readers)
            .ok()
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| Error::InvalidValue("connection count overflow".into()))?;
        let total_bytes = mib
            .checked_mul(1_048_576)
            .filter(|n| i64::try_from(*n).is_ok())
            .ok_or_else(|| Error::InvalidValue("memory budget overflow".into()))?;
        if mib < connections * 64 {
            return Err(Error::InvalidValue(
                "memory budget requires at least 64 MiB per connection".into(),
            ));
        }
        let quarter = total_bytes / 4 / connections;
        Ok(Some(Self {
            total_bytes,
            connections,
            cache_kib: quarter / 1024,
            mmap_bytes: quarter,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_budget_partitions_aggregate_not_each_process() {
        for processes in [0, 1, 4, 32] {
            let config = ServerConfig {
                memory_budget_mib: Some(4096),
                read_processes: processes,
                ..Default::default()
            };
            let plan = MemoryAllocation::for_server(&config).unwrap().unwrap();
            assert!(
                plan.cache_kib * 1024 * plan.connections + plan.mmap_bytes * plan.connections
                    <= plan.total_bytes / 2
            );
            assert_eq!(
                plan.connections,
                if processes == 0 {
                    5
                } else {
                    processes as u64 + 1
                }
            );
        }
    }

    #[test]
    fn memory_budget_refuses_missing_tiny_and_overflow() {
        assert_eq!(
            MemoryAllocation::for_server(&ServerConfig::default()).unwrap(),
            None
        );
        for (budget, processes) in [
            (None, 1),
            (Some(0), 0),
            (Some(63), 1),
            (Some(u64::MAX), 1),
            (Some(4096), 33),
        ] {
            assert!(
                MemoryAllocation::for_server(&ServerConfig {
                    memory_budget_mib: budget,
                    read_processes: processes,
                    ..Default::default()
                })
                .is_err()
            );
        }
        assert!(
            MemoryAllocation::for_server(&ServerConfig {
                memory_budget_mib: Some(320),
                ..Default::default()
            })
            .unwrap()
            .is_some()
        );
    }
}
