//! Bounded, process-local evidence that one ordinary knot worker has stopped.
//! This is not an idempotency service: a missing/evicted receipt is UNKNOWN.
use super::{SharedStore, StoreHandle, base::AppError};
use axum::{Json, extract::State, http::HeaderMap};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock, Weak},
    time::{Duration, Instant},
};

pub(crate) const CONTRACT: &str = "quipu.knot-completion/v1";
const HEADER: &str = "x-quipu-write-id";
const MAX_RECEIPTS: usize = 512;
const MAX_AGE: Duration = Duration::from_secs(3600);
type Key = (usize, String);
// Attestation nonces are per HTTP request, not the credential's stable owner.
type Owner = Option<(String, Option<String>, String)>;

fn owner() -> Owner {
    super::auth::request_identity().map(|i| (i.principal, i.credential_id, i.auth_class))
}

#[derive(Clone)]
enum Phase {
    Queued,
    Running,
    NotStarted,
    Stopped,
    Finished(Value),
}

struct Receipt {
    store: Weak<StoreHandle>,
    owner: Owner,
    hash: String,
    created: Instant,
    phase: Mutex<Phase>,
}

struct Registry {
    entries: HashMap<Key, Arc<Receipt>>,
    maximum: usize,
    age: Duration,
}

impl Registry {
    fn new(maximum: usize, age: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            maximum,
            age,
        }
    }

    fn prune(&mut self, now: Instant) {
        self.entries.retain(|_, receipt| {
            receipt.store.upgrade().is_some()
                && now.saturating_duration_since(receipt.created) < self.age
        });
    }

    fn insert(&mut self, key: Key, receipt: Arc<Receipt>, now: Instant) -> Result<(), AppError> {
        self.prune(now);
        if self.entries.contains_key(&key) {
            return Err(quipu::Error::InvalidValue(
                "write ID already retained; do not replay".into(),
            )
            .into());
        }
        if self.maximum == 0 {
            return Err(
                quipu::Error::InvalidValue("completion receipt capacity is zero".into()).into(),
            );
        }
        if self.entries.len() >= self.maximum {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, r)| r.created)
                .map(|(k, _)| k.clone());
            if let Some(oldest) = oldest {
                self.entries.remove(&oldest);
            }
        }
        self.entries.insert(key, receipt);
        Ok(())
    }

    fn lookup(&mut self, key: &Key, owner: &Owner, hash: &str, now: Instant) -> Option<Phase> {
        self.prune(now);
        let receipt = self.entries.get(key)?;
        if &receipt.owner != owner || receipt.hash != hash {
            return None;
        }
        Some(receipt.phase.lock().expect("completion phase lock").clone())
    }
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::new(MAX_RECEIPTS, MAX_AGE)))
}

fn key(store: &SharedStore, id: &str) -> Key {
    (Arc::as_ptr(store) as usize, id.to_owned())
}

fn valid_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn hash(request: &Value) -> Result<String, AppError> {
    if !request.is_object() {
        return Err(quipu::Error::InvalidValue(
            "completion binding needs the original JSON object".into(),
        )
        .into());
    }
    let bytes =
        serde_json::to_vec(request).map_err(|e| quipu::Error::Serialization(e.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

/// Owned by the actual blocking closure, including its queue and unwind.
pub(crate) struct Worker {
    receipt: Arc<Receipt>,
    started: bool,
    finished: bool,
}

impl Worker {
    pub(crate) fn running(&mut self) {
        self.started = true;
        *self.receipt.phase.lock().expect("completion phase lock") = Phase::Running;
    }

    /// Call only after ALL synchronous write and deferred-embedding work ended.
    pub(crate) fn finish(&mut self, result: Option<&Value>) {
        let receipt = result.map(|result| {
            let mut receipt = serde_json::Map::new();
            for name in ["tx_id", "count", "conforms", "replaced"] {
                if let Some(value) = result.get(name) {
                    receipt.insert(name.into(), value.clone());
                }
            }
            Value::Object(receipt)
        });
        *self.receipt.phase.lock().expect("completion phase lock") =
            receipt.map_or(Phase::Stopped, Phase::Finished);
        self.finished = true;
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if !self.finished {
            // Dropping an uncalled FnOnce proves it cannot later start. Once
            // entered, even a panic/error can follow a commit: read back first.
            *self.receipt.phase.lock().expect("completion phase lock") = if self.started {
                Phase::Stopped
            } else {
                Phase::NotStarted
            };
        }
        // Never reinsert: an evicted in-flight worker must remain UNKNOWN.
    }
}

pub(crate) fn begin(
    store: &SharedStore,
    headers: &HeaderMap,
    request: &Value,
) -> Result<Option<Worker>, AppError> {
    let Some(raw) = headers.get(HEADER) else {
        return Ok(None);
    };
    let id = raw
        .to_str()
        .ok()
        .filter(|id| valid_id(id))
        .filter(|_| headers.get_all(HEADER).iter().count() == 1)
        .ok_or_else(|| {
            quipu::Error::InvalidValue(
                "x-quipu-write-id must be one fresh 64-character lowercase hex ID".into(),
            )
        })?;
    let now = Instant::now();
    let receipt = Arc::new(Receipt {
        store: Arc::downgrade(store),
        owner: owner(),
        hash: hash(request)?,
        created: now,
        phase: Mutex::new(Phase::Queued),
    });
    registry()
        .lock()
        .expect("completion registry lock")
        .insert(key(store, id), receipt.clone(), now)?;
    Ok(Some(Worker {
        receipt,
        started: false,
        finished: false,
    }))
}

/// A lock-free-with-respect-to-the-store lookup; never guesses from current facts.
pub(crate) async fn status(
    State(store): State<SharedStore>,
    Json(input): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let id = input
        .get("write_id")
        .and_then(Value::as_str)
        .filter(|id| valid_id(id))
        .ok_or_else(|| quipu::Error::InvalidValue("missing or malformed write_id".into()))?;
    let request = input
        .get("request")
        .ok_or_else(|| quipu::Error::InvalidValue("missing original request binding".into()))?;
    let phase = registry().lock().expect("completion registry lock").lookup(
        &key(&store, id),
        &owner(),
        &hash(request)?,
        Instant::now(),
    );
    Ok(Json(response(id, phase)))
}

fn response(id: &str, phase: Option<Phase>) -> Value {
    let (state, receipt) = match phase {
        None => ("unknown", None),
        Some(Phase::Queued) => ("queued", None),
        Some(Phase::Running) => ("running", None),
        Some(Phase::NotStarted) => ("not_started", None),
        Some(Phase::Stopped) => ("stopped", None),
        Some(Phase::Finished(receipt)) => ("finished", Some(receipt)),
    };
    let mut response = json!({"contract":CONTRACT,"write_id":id,"state":state});
    if let Some(receipt) = receipt {
        response["receipt"] = receipt;
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(store: &SharedStore, now: Instant) -> Arc<Receipt> {
        Arc::new(Receipt {
            store: Arc::downgrade(store),
            owner: None,
            hash: "hash".into(),
            created: now,
            phase: Mutex::new(Phase::Queued),
        })
    }

    #[test]
    fn count_eviction_under_load_is_unknown_even_after_old_worker_finishes() {
        let store = Arc::new(StoreHandle::writer_only(
            quipu::Store::open_in_memory().unwrap(),
        ));
        let mut registry = Registry::new(3, Duration::from_secs(30));
        let now = Instant::now();
        let old = receipt(&store, now);
        registry
            .insert(key(&store, "old"), old.clone(), now)
            .unwrap();
        let mut worker = Worker {
            receipt: old,
            started: false,
            finished: false,
        };
        worker.running();
        for index in 1..20 {
            let at = now + Duration::from_millis(index);
            registry
                .insert(key(&store, &index.to_string()), receipt(&store, at), at)
                .unwrap();
            assert!(registry.entries.len() <= 3);
        }
        assert!(
            registry
                .lookup(&key(&store, "old"), &None, "hash", now)
                .is_none()
        );
        worker.finish(Some(&json!({"tx_id":42,"count":1})));
        drop(worker);
        let observed = response(
            "old",
            registry.lookup(&key(&store, "old"), &None, "hash", now),
        );
        assert_eq!(
            observed["state"], "unknown",
            "late worker must not resurrect an evicted receipt"
        );
        assert!(observed.get("receipt").is_none());
    }

    #[test]
    fn age_expiry_and_foreign_request_bindings_are_unknown() {
        let store = Arc::new(StoreHandle::writer_only(
            quipu::Store::open_in_memory().unwrap(),
        ));
        let mut registry = Registry::new(3, Duration::from_secs(30));
        let now = Instant::now();
        let id = key(&store, "age");
        registry
            .insert(id.clone(), receipt(&store, now), now)
            .unwrap();
        assert!(registry.lookup(&id, &None, "different", now).is_none());
        let foreign = Some(("foreign".into(), None, "named_bearer".into()));
        assert!(registry.lookup(&id, &foreign, "hash", now).is_none());
        assert!(matches!(
            registry.lookup(&id, &None, "hash", now),
            Some(Phase::Queued)
        ));
        assert!(
            registry
                .lookup(&id, &None, "hash", now + Duration::from_secs(30))
                .is_none()
        );
        assert!(registry.entries.is_empty());
    }

    #[test]
    fn only_an_explicit_uncalled_worker_drop_proves_not_started() {
        let store = Arc::new(StoreHandle::writer_only(
            quipu::Store::open_in_memory().unwrap(),
        ));
        let mut registry = Registry::new(3, Duration::from_secs(30));
        let now = Instant::now();
        let id = key(&store, "not-started");
        assert!(registry.lookup(&id, &None, "hash", now).is_none());
        let entry = receipt(&store, now);
        registry.insert(id.clone(), entry.clone(), now).unwrap();
        drop(Worker {
            receipt: entry,
            started: false,
            finished: false,
        });
        assert!(matches!(
            registry.lookup(&id, &None, "hash", now),
            Some(Phase::NotStarted)
        ));
        let entry = receipt(&store, now);
        let mut started = Worker {
            receipt: entry.clone(),
            started: false,
            finished: false,
        };
        started.running();
        drop(started);
        assert!(
            matches!(*entry.phase.lock().unwrap(), Phase::Stopped),
            "started worker must never become not_started"
        );
    }

    #[test]
    fn disabled_or_expired_request_deadline_is_not_a_completion_proof() {
        let store = Arc::new(StoreHandle::writer_only(
            quipu::Store::open_in_memory().unwrap(),
        ));
        for deadline in [None, Some(quipu::time::Deadline::after_millis(0))] {
            let _deadline = quipu::time::set_request_deadline(deadline);
            let entry = receipt(&store, Instant::now());
            let mut worker = Worker {
                receipt: entry.clone(),
                started: false,
                finished: false,
            };
            worker.running();
            assert!(matches!(*entry.phase.lock().unwrap(), Phase::Running));
            drop(worker);
            assert!(matches!(*entry.phase.lock().unwrap(), Phase::Stopped));
        }
    }
}
