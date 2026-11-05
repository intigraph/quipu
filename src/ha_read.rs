//! Inactive Linux/SQLite-only HA source profile (aegis-s34).
//!
//! No listener, config loader, CLI, default/full feature or deployment uses it.
//! The owner pins a complete cut and stream key through a trusted channel. One
//! shared applier owns the replica; only signed scoped frames may tail the cut.
//! Every query copies an authorized view under the SAME SQLite transaction as
//! its source/cursor checks. It never exports a reusable readiness capability.
//!
//! This first profile permits zero *verified* lag only: an answered challenge
//! covers the source head it observed. It cannot bound transactions committed
//! later; that uncertainty is bounded only in time, by the explicit age limit.
//! Cross-version migration and non-SQLite providers are refused, not inferred.

use rusqlite::OptionalExtension as _;
use serde::Serialize;

use crate::applier::{Applier, Outcome};
use crate::asof_share::{Manifest, manifest_bytes};
use crate::read_authorization::{OwnerReadPolicy, ReadIdentity, ReadOperation};
use crate::{Error, Result, Store};

fn refuse(reason: &str) -> Error {
    Error::CannotVerify(format!("HA read profile: {reason}"))
}

/// Owner-supplied pins, never learned from an arriving frame. Merely compiling
/// or constructing this value enables nothing; `enabled` must be explicit.
#[derive(Clone)]
pub struct Config {
    /// Explicit owner opt-in; false is refused without profile writes.
    pub enabled: bool,
    /// Exact cut pin from an independently trusted acquisition channel.
    pub manifest: Manifest,
    /// Out-of-band pinned Ed25519 source key, lowercase hex.
    pub stream_public_key: String,
    /// Required finite upper bound including transport, suspend and query time.
    pub max_age_ms: u64,
    /// Only zero verified lag is supported by this conservative first profile.
    pub max_lag_txs: u64,
}

impl Config {
    fn validate(&self) -> Result<String> {
        let m = &self.manifest;
        if !self.enabled {
            return Err(refuse("disabled"));
        }
        if self.max_age_ms == 0 || self.max_age_ms == u64::MAX || self.max_lag_txs != 0 {
            return Err(refuse(
                "explicit finite age and zero verified lag are required",
            ));
        }
        if m.schema != crate::asof_share::SCHEMA
            || m.producer.version != env!("CARGO_PKG_VERSION")
            || m.producer.git_sha != env!("QUIPU_GIT_SHA")
            || m.producer.git_dirty
            || m.watermark_tx < 0
            || m.source_head_tx < m.watermark_tx
            || !m.completeness.valid_time
            || m.completeness.bytes != "lossless"
            || m.completeness.graphs.values().any(|g| g.frozen.is_some())
            || m.source.store_id.is_empty()
            || m.source.log_epoch.is_empty()
        {
            return Err(refuse("unsupported cut/provider/migration contract"));
        }
        let (scope, context) = crate::asof_share::scope_lists(&m.scope, &m.context_graphs)
            .map_err(|_| refuse("invalid scope"))?;
        if m.scope_digest != crate::asof_share::scope_digest(&scope, &context)
            || scope
                .iter()
                .chain(&context)
                .collect::<std::collections::BTreeSet<_>>()
                != m.completeness.graphs.keys().collect()
            || self.stream_public_key.len() != 64
            || hex::decode(&self.stream_public_key).is_err()
        {
            return Err(refuse("incomplete scope or invalid pinned key"));
        }
        Ok(crate::share::sha256(&manifest_bytes(m)?))
    }
}

/// Data and the exact source position of its serving snapshot. This is a query
/// result, not an eligibility token usable for a later/unbound read.
#[derive(Debug, Serialize)]
pub struct Response {
    /// Source metadata requires BOTH query and replicate authority.
    pub source_store_id: String,
    pub log_epoch: String,
    pub scope_digest: String,
    pub applied_tx: i64,
    pub verified_lag_txs: u64,
    pub staleness_bound_ms: u64,
    pub media_type: &'static str,
    pub body: Vec<u8>,
}

/// Owns the only applier and its exact store. No raw Store accessor exists.
pub struct Profile {
    store: Store,
    applier: Applier,
    config: Config,
    manifest_hash: String,
    complete: bool,
    halted: bool,
    draining: bool,
    clock: fn() -> Result<u64>,
    last_clock: u64,
    #[cfg(test)]
    after_pin: Option<Box<dyn FnOnce() + Send>>,
}

const MARKER: &str = "CREATE TABLE IF NOT EXISTS ha_read_cut (
 id INTEGER PRIMARY KEY CHECK(id=1), manifest_hash TEXT NOT NULL, halted INTEGER NOT NULL DEFAULT 0)";

impl Profile {
    /// Explicit owner-local construction; disabled and unsupported settings are
    /// refused BEFORE opening an applier or creating profile metadata.
    pub fn open(store: Store, config: Config) -> Result<Self> {
        let manifest_hash = config.validate()?;
        unsupported(&store)?;
        let clock = boot_ms()?;
        let has_marker: bool = store.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='ha_read_cut' AND type='table')",
            [],
            |r| r.get(0),
        )?;
        let marker = if has_marker {
            store
                .conn
                .query_row(
                    "SELECT manifest_hash,halted FROM ha_read_cut WHERE id=1",
                    [],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)),
                )
                .optional()?
        } else {
            None
        };
        if marker.as_ref().is_some_and(|(m, _)| m != &manifest_hash) {
            return Err(refuse("persisted cut pin differs"));
        }
        // A feed-only replica is NOT a complete-cut bootstrap, even if ready200.
        let has_progress: bool = store.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='replica_progress' AND type='table')",
            [], |r| r.get(0),
        )?;
        if has_progress
            && marker.is_none()
            && store
                .conn
                .query_row("SELECT EXISTS(SELECT 1 FROM replica_progress)", [], |r| {
                    r.get::<_, bool>(0)
                })?
        {
            return Err(refuse("replica has no qualified complete-cut marker"));
        }
        if Applier::new_replica_refusal(&store)?.is_some() {
            return Err(refuse("new replica contains unqualified local facts"));
        }
        let complete = marker.is_some();
        let mut applier = Applier::open(&store)?;
        applier.trust_stream(&config.stream_public_key, &config.manifest.scope_digest);
        store.conn.execute_batch(MARKER)?;
        Ok(Self {
            store,
            applier,
            config,
            manifest_hash,
            complete,
            halted: marker.is_some_and(|(_, halted)| halted),
            draining: false,
            clock: boot_ms,
            last_clock: clock,
            #[cfg(test)]
            after_pin: None,
        })
    }

    fn now(&mut self) -> Result<u64> {
        let now = (self.clock)()?;
        if now < self.last_clock {
            self.applier.clock_discontinuity();
            return Err(refuse("clock discontinuity"));
        }
        self.last_clock = now;
        Ok(now)
    }

    /// Frames are transported by an authorized external adapter; this method
    /// adds no network, credential, policy, follower or service activation.
    pub fn apply_frame(&mut self, event: &str, data: &str) -> Result<Outcome> {
        if self.halted {
            return Err(refuse("stopped; rebuild required"));
        }
        let now = self.now()?;
        if event == "quipu.asof" || event == "quipu.asof.begin" {
            let frame: serde_json::Value =
                serde_json::from_str(data).map_err(|_| refuse("invalid cut frame"))?;
            let manifest: Manifest = serde_json::from_value(frame["manifest"].clone())
                .map_err(|_| refuse("invalid cut manifest"))?;
            if crate::share::sha256(&manifest_bytes(&manifest)?) != self.manifest_hash {
                return Err(refuse("cut differs from owner pin"));
            }
        } else if !event.starts_with("quipu.asof.") && !event.starts_with("quipu.replicate.") {
            return Err(refuse(
                "only complete cut and scoped stream frames are supported",
            ));
        }
        let outcome = self
            .applier
            .apply_frame(&mut self.store, event, data, now)?;
        match &outcome {
            Outcome::Bootstrapped { .. } => {
                self.store.conn.execute(
                    "INSERT INTO ha_read_cut(id,manifest_hash) VALUES(1,?1)",
                    [&self.manifest_hash],
                )?;
                self.complete = true;
            }
            Outcome::Rebootstrap { .. }
            | Outcome::Stopped { .. }
            | Outcome::Revoked { .. }
            | Outcome::Refused { .. } => {
                self.halted = true;
                self.store
                    .conn
                    .execute("UPDATE ha_read_cut SET halted=1 WHERE id=1", [])?;
            }
            _ => {}
        }
        Ok(outcome)
    }

    /// Issue before sending to the source. Nonces and clock readings cannot be
    /// supplied by an HTTP caller; signed responses are checked by ONE applier.
    pub fn challenge(&mut self) -> Result<[u8; 16]> {
        let now = self.now()?;
        let session = self
            .applier
            .session()
            .ok_or_else(|| refuse("no signed session"))?
            .to_owned();
        let mut nonce = [0; 16];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut nonce)
            .map_err(|_| refuse("nonce generation failed"))?;
        self.applier
            .challenge(&session, nonce, now)
            .map_err(|_| refuse("challenge refused"))?;
        Ok(nonce)
    }

    /// Draining immediately invalidates proofs. Rejoining requires a new proof.
    pub fn set_draining(&mut self, draining: bool) {
        self.draining = draining;
        self.applier.clock_discontinuity();
    }

    /// A closed transport cannot keep serving a cached freshness indication.
    pub fn end_session(&mut self) {
        self.applier.end_session();
    }

    /// Execute an authorized query, including COUNT, on the same observed
    /// snapshot as eligibility/minTx. No unbound readiness precheck is exposed.
    pub fn query(
        &mut self,
        policy: &OwnerReadPolicy,
        identity: &ReadIdentity,
        text: &str,
        min_tx: i64,
        now_epoch: u64,
    ) -> Result<Response> {
        let before = self.now()?;
        let m = &self.config.manifest;
        // Existing authorization remains conjunctive and cannot be forged from
        // readiness, a declared principal, or a write-class credential.
        let allowed: std::collections::BTreeSet<String> = m
            .scope
            .iter()
            .chain(&m.context_graphs)
            .map(|g| {
                if g == "ROOT" {
                    crate::schema::ROOT_GRAPH_IRI.to_owned()
                } else {
                    g.clone()
                }
            })
            .collect();
        let authority = policy.snapshot()?;
        let replication = authority.authorize(
            identity,
            ReadOperation::Replicate,
            Some(&allowed),
            now_epoch,
        )?;
        let auth =
            authority.authorize(identity, ReadOperation::Query, Some(&allowed), now_epoch)?;
        if !self.complete || self.halted || self.draining || min_tx < 0 {
            return Err(refuse("incomplete, stopped, draining or invalid minTx"));
        }
        let bound = self
            .applier
            .freshness(before)
            .staleness_bound_ms
            .filter(|b| *b <= self.config.max_age_ms)
            .ok_or_else(|| refuse("freshness unknown or stale"))?;
        unsupported(&self.store)?;
        let read = self.store.conn.unchecked_transaction()?;
        // FIRST read pins the serving snapshot. Its progress AND signed coverage
        // must match this exact applier, never another Store or a cached bit.
        let (source, epoch, progress): (String, String, i64) = self.store.conn.query_row(
            "SELECT store_id,log_epoch,source_tx FROM replica_progress WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let (cs, ce, coverage): (String, String, i64) = self.store.conn.query_row(
            "SELECT store_id,log_epoch,watermark FROM replica_coverage WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let (marker, halted): (String, bool) = self.store.conn.query_row(
            "SELECT manifest_hash,halted FROM ha_read_cut WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let ended: bool = self.store.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM replica_stop) OR EXISTS(SELECT 1 FROM replica_source_end WHERE store_id=?1 AND log_epoch=?2)",
            [&source, &epoch], |r| r.get(0))?;
        if marker != self.manifest_hash || halted || ended {
            return Err(refuse("cut marker/stop state mismatch"));
        }
        let held = progress.max(coverage);
        let p = self.applier.progress().ok_or_else(|| refuse("no cursor"))?;
        if source != m.source.store_id
            || epoch != m.source.log_epoch
            || cs != source
            || ce != epoch
            || p.source.store_id != source
            || p.source.log_epoch != epoch
            || p.source_tx != progress
            || self.applier.coverage() != Some(coverage)
            || held < min_tx
        {
            return Err(refuse("snapshot lineage/cursor/minTx mismatch"));
        }
        #[cfg(test)]
        if let Some(after_pin) = self.after_pin.take() {
            after_pin();
        }
        let view = auth.open_in_snapshot(&self.store, now_epoch)?;
        let (media_type, body) = view.query_response(text, false, now_epoch)?;
        let scope_digest = m.scope_digest.clone();
        read.rollback()?;
        let publication_policy = policy.snapshot()?;
        // Query/copy time counts in the same bound; never publish a stale result.
        let after = self.now()?;
        let elapsed = after
            .checked_sub(before)
            .and_then(|d| d.checked_add(1))
            .ok_or_else(|| refuse("clock regressed or overflowed"))?;
        // Ceil the observation clock: two truncated millisecond samples must
        // never understate actual age at a boundary by almost one millisecond.
        let bound = bound
            .checked_add(elapsed)
            .filter(|b| *b <= self.config.max_age_ms)
            .ok_or_else(|| refuse("query outlived freshness"))?;
        let publication_epoch = now_epoch
            .checked_add(elapsed.div_ceil(1000))
            .ok_or_else(|| refuse("publication clock overflow"))?;
        view.recheck(&publication_policy, identity, publication_epoch)?;
        replication.recheck(&publication_policy, identity, publication_epoch)?;
        Ok(Response {
            source_store_id: source,
            log_epoch: epoch,
            scope_digest,
            applied_tx: held,
            verified_lag_txs: 0,
            staleness_bound_ms: bound,
            media_type,
            body,
        })
    }
}

fn unsupported(store: &Store) -> Result<()> {
    if store.has_attachments()
        || store.vector_delegate.is_some()
        || store.local_vector_backend.is_some()
        || store.embedding_provider.is_some()
    {
        return Err(refuse(
            "attachments/index/model/provider coverage unsupported",
        ));
    }
    Ok(())
}

// Linux BOOTTIME includes suspend. Unsupported clocks return UNKNOWN; no wall
// clock or saturating-sub fallback may turn a clock fault into fresh evidence.
fn boot_ms() -> Result<u64> {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::Boottime);
    u64::try_from(t.tv_sec)
        .ok()
        .and_then(|s| s.checked_mul(1000))
        .and_then(|s| s.checked_add(u64::try_from(t.tv_nsec).ok()? / 1_000_000))
        .ok_or_else(|| refuse("invalid BOOTTIME"))
}

#[cfg(test)]
#[path = "ha_read_tests.rs"]
mod tests;
