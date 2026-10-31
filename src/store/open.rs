//! Store construction: open paths, schema init, and the attachment surface.
//!
//! Split from `mod.rs` (quipu-bu3). Everything here runs before or at the
//! moment a [`Store`] exists: the open variants, `init` and its migration
//! ordering, and the read-only accessors over what was mounted at open.

use ring::rand::{SecureRandom, SystemRandom};
use rusqlite::{Connection, OptionalExtension, params};

use super::{PoststateProfile, Store, TermCache, alias, attach, local_term_space, read_model};
use crate::config::{
    EmbeddingConfig, GovernanceConfig, OwlConfig, ResolutionConfig, SearchConfig, ShaclConfig,
};
use crate::error::{Error, Result};
use crate::vector::VECTORS_SQL;

impl Store {
    /// Apply a validated share of the aggregate service memory budget.
    /// This changes connection-local SQLite policy, never database facts.
    pub fn apply_memory_allocation(
        &self,
        allocation: crate::memory_budget::MemoryAllocation,
    ) -> Result<()> {
        let cache = i64::try_from(allocation.cache_kib)
            .map_err(|_| Error::InvalidValue("cache allocation overflow".into()))?;
        let mmap = i64::try_from(allocation.mmap_bytes)
            .map_err(|_| Error::InvalidValue("mmap allocation overflow".into()))?;
        self.conn.pragma_update(None, "cache_size", -cache)?;
        self.conn.pragma_update(None, "mmap_size", mmap)?;
        Ok(())
    }

    /// Open (or create) a Quipu store at the given path.
    pub fn open(path: &str) -> Result<Self> {
        Self::open_owned(path, &[])
    }

    /// Create an in-memory store (useful for tests).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    /// Open an existing store READ-ONLY, for a reader in a connection pool.
    ///
    /// WAL already permits N concurrent readers alongside one writer; before
    /// this, the server serialised every read behind the single writer
    /// connection's mutex, so effective parallelism was 1.0 at every
    /// concurrency. A pool needs N connections because
    /// `rusqlite::Connection` is `Send` but **not** `Sync` — it cannot be
    /// shared behind a read lock, only moved and owned exclusively.
    ///
    /// Two deliberate differences from [`Self::open`]:
    ///
    /// - `SQLITE_OPEN_READ_ONLY`, so a bug on a read path cannot write. This is
    ///   the mechanism, not the comment: a stray `INSERT` here fails at `SQLite`
    ///   rather than racing the writer.
    /// - **No DDL and no migration.** `init` runs `INIT_SQL`, `VECTORS_SQL` and
    ///   `migrate_named_graphs` on every open. Running schema setup N more times
    ///   at startup is at best redundant work against the writer's database and
    ///   at worst a concurrent migration; a read-only connection cannot do it
    ///   anyway. The writer owns schema, exclusively.
    ///
    /// The returned store carries DEFAULT configuration. Callers must copy the
    /// read-relevant policy across with [`Self::adopt_read_config_from`] — a
    /// pooled reader running different search guardrails than the writer would
    /// silently answer the same question two ways depending on which connection
    /// served it.
    pub fn open_read_only(path: &str) -> Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )?;
        // foreign_keys is a no-op for reads but keeps the connection's semantics
        // identical to the writer's; query_only makes the read-only intent true
        // at the SQL layer as well as the file-handle layer.
        //
        // `mmap_size` is NOT a micro-optimisation here, it is what makes the
        // pool a win at all — MEASURED, and it was measured because the first
        // version of this function omitted it and the acceptance curve caught
        // the result.
        //
        // Going from one connection to N trades a lock for N private page
        // caches. The single shared connection had ONE cache that every
        // serialised reader warmed for the next; eight connections each fault
        // the same pages in separately. With 8 concurrent readers on a 160k-fact
        // store that showed up as `wait` falling to 0.000s — the lock contention
        // genuinely gone — while `held` rose from 0.136s to 1.72s per query.
        // Parallelism was real and per-query cost had inflated 13x to pay for
        // it, so wall time barely moved. A pool can serialise silently; it can
        // also PARALLELISE silently and still buy nothing, and only the curve
        // tells them apart.
        //
        // Memory-mapped I/O restores the sharing: every connection maps the same
        // file pages through the OS page cache instead of copying them into a
        // private heap cache, so N readers cost roughly one reader's memory.
        conn.execute_batch(
            "PRAGMA foreign_keys=ON;
             PRAGMA query_only=ON;
             PRAGMA mmap_size=268435456;
             PRAGMA cache_size=-32000;",
        )?;
        Ok(Self::with_connection(conn))
    }

    /// Count this connection's SQLite statements for queries that arm
    /// [`crate::statement_profile`] (aegis-xacr). Off unless a server enables it.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn enable_statement_profile(&self) {
        crate::statement_profile::enable(&self.conn);
    }

    /// Set this connection's bounded wait for transient SQLite apply locks.
    /// This changes only connection-local policy, never persistent store state.
    pub fn set_busy_timeout(&self, timeout: std::time::Duration) -> Result<()> {
        self.conn.busy_timeout(timeout)?;
        Ok(())
    }

    /// Open a store FILE that must not change, such as a pack being verified
    /// (aegis-s8jra2).
    ///
    /// [`Self::open`] runs schema setup and migrations and commits them, so a
    /// verify through it rewrote the pack. [`Self::open_read_only`] cannot
    /// write, but on a WAL-mode file it still creates `-wal`/`-shm` siblings
    /// that it cannot remove, and a pack must stay one file. This opens with
    /// `immutable=1`: no locks, no siblings, bytes untouched. If a `-wal`
    /// sibling already exists it may hold committed pages that `immutable`
    /// would ignore, so that case falls back to a plain read-only open.
    ///
    /// Carries DEFAULT configuration and runs no DDL, like
    /// [`Self::open_read_only`].
    pub fn open_immutable(path: &str) -> Result<Self> {
        let conn = open_file_immutable(path)?;
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA query_only=ON;")?;
        Ok(Self::with_connection(conn))
    }

    /// Copy the READ-relevant configuration from another store.
    ///
    /// Reads are policy-bearing: `search_config` bounds result sets (hq-gkd),
    /// `base_ns` decides which IRIs a query is even about, and
    /// `owl_config` governs subclass inference on the read path. A pooled reader
    /// left on defaults would answer differently from the writer for reasons no
    /// caller could see — the same class of defect as a fact that is present and
    /// unretrievable.
    ///
    /// Write-path policy (`resolution`, `shacl`, `governance`, `signing`) is
    /// deliberately NOT copied: a read-only connection never reaches those
    /// gates, and copying them would imply a reader could write.
    ///
    /// Not copied either, and the reason is a real constraint rather than a
    /// preference: the vector backends (`vector_delegate`,
    /// `local_vector_backend`) are boxed trait objects and are not `Clone`, so
    /// vector-search handlers stay on the writer connection. See `ReadPool` in
    /// the server for which handlers are pooled.
    pub fn adopt_read_config_from(&mut self, other: &Self) {
        self.base_ns.clone_from(&other.base_ns);
        self.namespace_settings
            .clone_from(&other.namespace_settings);
        self.search_config.clone_from(&other.search_config);
        self.labels_config.clone_from(&other.labels_config);
        self.owl_config.clone_from(&other.owl_config);
        self.embedding_config.clone_from(&other.embedding_config);
        // A pooled reader resolves `/episode` nodes before the writer commits
        // them (aegis-xi0), and serves `/resolve`; both must use the served
        // `[quipu.resolution]` policy, not the defaults.
        self.resolution_config.clone_from(&other.resolution_config);
        // Readers answer projection aggregates (aegis-tg5 F2) and must know the
        // same declarations the writer installed.
        self.projection_defs.clone_from(&other.projection_defs);
        self.resolution_cache = other.resolution_cache.clone();
        self.embedding_provider
            .clone_from(&other.embedding_provider);
    }

    /// Open a store with read-only databases mounted alongside it (quipu #75).
    ///
    /// The attachments contribute NAMED graphs. No existing query's result
    /// changes: the default dataset stays main-ROOT-alone, so an attachment is
    /// only visible to a query that names one of its graphs.
    ///
    /// # Errors
    /// [`Error::Store`] for an invalid alias, a duplicate alias, or an
    /// attachment quipu refuses to compose (no `g` column, or a term space
    /// that collides — see `attach::verify_attached_schema`).
    pub fn open_with_attachments(path: &str, attachments: &[attach::Attachment]) -> Result<Self> {
        Self::open_owned(path, attachments)
    }

    /// Move what can be moved out of the WAL, without ever blocking a reader
    /// (aegis-raq1ok). Safe to call on a live serving store; see `store::wal`.
    ///
    /// Returns what SQLite actually achieved, so a caller can tell a checkpoint
    /// that RAN from one that RETIRED the log — the distinction this incident
    /// was made of.
    pub fn checkpoint_wal_passive(&self) -> Result<crate::store::wal::CheckpointOutcome> {
        crate::store::wal::checkpoint_passive(&self.conn)
    }

    /// Retire the WAL and RESET THE FILE. Blocks on readers — call only where
    /// none can exist, i.e. at startup before the read pool opens
    /// (aegis-raq1ok). See `store::wal::checkpoint_truncate`.
    pub fn checkpoint_wal_truncate(&self) -> Result<crate::store::wal::CheckpointOutcome> {
        crate::store::wal::checkpoint_truncate(&self.conn)
    }

    /// Size of the write-ahead log in bytes, or `None` when it cannot be read.
    ///
    /// `None` is deliberately distinct from `0`: an unreadable WAL and an empty
    /// one are opposite findings, and a gauge that reports the first as the
    /// second would say "healthy" about a store it could not see.
    #[must_use]
    pub fn wal_bytes(&self) -> Option<u64> {
        let path: String = self
            .conn
            .query_row("PRAGMA database_list", [], |r| r.get(2))
            .ok()?;
        if path.is_empty() {
            return None; // in-memory store: no WAL on disk, not a zero-byte one
        }
        std::fs::metadata(format!("{path}-wal"))
            .ok()
            .map(|m| m.len())
    }

    pub(crate) fn init(conn: Connection) -> Result<Self> {
        Self::init_with_attachments(conn, &[])
    }

    pub(super) fn init_with_attachments(
        conn: Connection,
        attachments: &[attach::Attachment],
    ) -> Result<Self> {
        // Every initializing constructor checks sticky ownership before DDL,
        // including deserialized stores; unsupported providers cannot downgrade.
        super::ownership_startup::require_unenrolled(&conn)?;
        let inspected = Self::with_connection(conn);
        inspected.ownership_state()?;
        let conn = inspected.conn;
        crate::lexical::register(&conn)?;
        // Per-row SPARQL scans reuse cached statements, one per text and
        // nesting depth (aegis-ek4); a multi-branch board query needs more
        // than rusqlite's default 16 before the LRU starts re-preparing.
        conn.set_prepared_statement_cache_capacity(256);
        // `wal_autocheckpoint` is stated EXPLICITLY rather than left to SQLite's
        // default (aegis-raq1ok). Read the honest caveat before changing it:
        // the deployed store reached a **1.06 GB** WAL against a 5.7 GB
        // database while the default (1000 pages) was in force, so this pragma
        // is NOT what was missing and setting it alone fixes nothing. SQLite
        // cannot checkpoint past the oldest live reader, and the server holds a
        // pool of long-lived read connections; a checkpoint that is attempted
        // and blocked leaves the WAL growing exactly as observed.
        //
        // It is set anyway for two reasons: intent stops being inferred from a
        // library default that can change under us, and the value becomes a
        // thing a reader can see next to `quipu_wal_bytes`. The load-bearing
        // parts of the fix are the passive checkpoints and that metric.
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA wal_autocheckpoint=1000;",
        )?;
        super::search_scope_policy::initialize_schema(&conn)?;
        Self::ensure_store_identity(&conn)?;
        Self::ensure_log_epoch(&conn)?;
        conn.execute_batch(VECTORS_SQL)?;
        // Frozen packs re-attach on every open (deep freeze): read the
        // registry BEFORE attach_all so the caller's attachments and the
        // frozen ones verify as one composition. The table is read via a
        // sqlite_master guard because migrations have not run yet — a store
        // that never froze anything has no table and skips this entirely.
        // A missing pack file is a HARD refusal naming the path: an archive
        // graph silently absent is the silent-zero-rows failure this stack
        // refuses everywhere.
        let mut attachments = attachments.to_vec();
        attachments.extend(super::freeze::frozen_attachments(&conn)?);
        let attachments = attachments.as_slice();
        // ATTACH here, after INIT_SQL and before the migrations, per
        // multi-db-composition.md §2. Safe because every migration below uses
        // unqualified table names, which SQLite binds to `main` — measured, not
        // assumed; the table is in `attach`'s module docs.
        attach::attach_all(&conn, attachments)?;
        Self::migrate_named_graphs(&conn)?;
        // AFTER migrate_named_graphs: idx_geav must exist before the index it
        // makes redundant is dropped (quipu-fcg).
        Self::migrate_drop_eavt(&conn)?;
        // Attribute-scoped current-fact index (aegis-svtdyn). Additive; no
        // ordering constraint beyond the `facts` table existing.
        Self::migrate_current_fact_index(&conn)?;
        // Entity-scoped current-fact index (aegis-wlf0). AFTER
        // migrate_named_graphs, which adds the `g` column it keys on.
        Self::migrate_current_entity_index(&conn)?;
        // Source-scoped retraction index (aegis-m6agjy); INIT_SQL owns the table.
        Self::migrate_transaction_source_index(&conn)?;
        // BEFORE `migrate_graph_labels`, which interns the meta-graph IRI: that
        // intern must know the store's space, or on a non-zero-space store it
        // allocates the reserved graph OUTSIDE the space that owns it.
        Self::migrate_term_spaces(&conn)?;
        Self::migrate_graph_labels(&conn)?;
        Self::migrate_datasets(&conn)?;
        Self::migrate_pendant_repos(&conn)?;
        // Order-independent: these tables reference nothing else.
        Self::migrate_session_attestation(&conn)?;
        Self::migrate_decision_nonces(&conn)?;
        #[cfg(not(target_arch = "wasm32"))]
        Self::migrate_attested_write(&conn)?;
        #[cfg(not(target_arch = "wasm32"))]
        Self::migrate_decision_deck(&conn)?;
        Self::migrate_registry_amendment_nonces(&conn)?;
        Self::migrate_bitemporal_registries(&conn)?;
        Self::migrate_query_registry(&conn)?;
        Self::migrate_pending_share_queries(&conn)?;
        Self::migrate_retraction_tx(&conn)?;
        Self::migrate_graph_events(&conn)?;
        // AFTER migrate_named_graphs: the fork registry references graphs(g).
        Self::migrate_forks(&conn)?;
        Self::migrate_frozen_packs(&conn)?;
        Self::migrate_graph_metadata_v1(&conn)?;
        attach::migrate_graph_source(&conn)?;

        // Verification runs AFTER the migrations, not at attach time, because
        // the space-collision check needs the local store's own space — which
        // `migrate_term_spaces` is what establishes. Nothing has read the
        // attachment before this point.
        if !attachments.is_empty() {
            let local_space = local_term_space(&conn)?;
            attach::verify_attached_schema(&conn, attachments, local_space)?;
            attach::register_attached_graphs(&conn, attachments)?;
        }
        let pack_manifests = attach::attached_pack_manifests(&conn, attachments)?;

        // AFTER verification and registration: the source it builds is only
        // meaningful for attachments quipu has agreed to compose, and it reads
        // the same meta-graph id registration filtered on.
        let facts_source = attach::build_facts_source(&conn, attachments)?;
        let resolve_sql = attach::build_resolve_sql(attachments);
        // quipu #76: the alias table is TEMP and derived, so it is built here
        // rather than migrated — see `alias`'s module docs for why it is not
        // persisted. Safe on an unattached store: it creates an empty table.
        alias::build_term_alias(&conn, attachments)?;

        let mut store = Self::with_connection(conn);
        store.attachments = attachments.to_vec();
        store.pack_manifests = pack_manifests;
        store.facts_source = facts_source;
        store.resolve_sql = resolve_sql;
        store.attached_graphs = attach::attached_graph_ids(&store.conn, attachments)?;
        // Installed after the migrations: from here on every outer commit on
        // this handle re-checks ownership (see `ownership_fence`).
        store.ownership_fence = super::ownership_fence::CommitFence::new(&store.conn);
        if let Some(fence) = store.ownership_fence.clone() {
            store.conn.commit_hook(Some(move || fence.refuses()))?;
        }
        Ok(store)
    }

    pub(super) fn ensure_store_identity(conn: &Connection) -> Result<()> {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM store_identity WHERE id = 1)",
            [],
            |row| row.get(0),
        )?;
        if !exists {
            conn.execute(
                "INSERT INTO store_identity (id, store_id) VALUES (1, ?1)",
                params![new_urn_uuid("store identity")?],
            )?;
        }
        Ok(())
    }

    /// A store opened for the first time, or first opened by a version that
    /// predates the epoch, gets one now; its earlier history had none.
    pub(super) fn ensure_log_epoch(conn: &Connection) -> Result<()> {
        conn.execute(
            "INSERT OR IGNORE INTO log_epoch (id, epoch, minted_at, reason) \
             VALUES (1, ?1, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), 'created')",
            params![new_urn_uuid("log epoch")?],
        )?;
        Ok(())
    }

    /// The SQL [`Self::resolve`] runs. Exposed for the test that pins it
    /// byte-identical on an unattached store.
    #[cfg(test)]
    pub(crate) fn resolve_sql(&self) -> &str {
        &self.resolve_sql
    }

    /// The table source a query reads `facts` from: `"facts"` for a store with
    /// no attachments, a `UNION ALL` over `main` and every attached layer
    /// otherwise (`multi-db-composition.md` §4).
    ///
    /// Interpolate this in place of the `facts` table name in a query that
    /// should see attached graphs. **Not for the write path or for local
    /// bookkeeping** — writes are `main`-only by design (§6), and a read that
    /// is deliberately ROOT-scoped cannot see an attached graph anyway, since
    /// attachments contribute only NAMED graphs.
    pub(crate) fn facts_source(&self) -> &str {
        if self.composition_suspended.get() {
            return "facts";
        }
        &self.facts_source
    }

    /// The databases mounted alongside this store.
    #[must_use]
    pub fn attachments(&self) -> &[attach::Attachment] {
        &self.attachments
    }

    /// Graph ids the attachments contribute (aegis-xacr).
    pub(crate) fn attached_graphs(&self) -> &[i64] {
        &self.attached_graphs
    }

    /// Answer as an unattached store (`composes == false`) or as the composed
    /// one until the guard drops, then restore the prior view (aegis-xacr).
    ///
    /// Only [`Self::has_attachments`] and [`Self::facts_source`] change:
    /// dictionary lookups, aliases and resolution stay composed, so an
    /// attached id that a local fact names still means what it meant. The
    /// caller proves the query can read no attached row and names no alias
    /// (see `sparql::attach_scope`); this only switches the view. Before this,
    /// ONE attached frozen pack turned off every fast path gated on
    /// [`Self::has_attachments`], for every query, whatever graph it read.
    pub(crate) fn scope_composition(&self, composes: bool) -> CompositionGuard<'_> {
        let prior = self.composition_suspended.replace(!composes);
        CompositionGuard { store: self, prior }
    }

    /// Knowledge-pack manifests contributed by attachments, paired with alias.
    #[must_use]
    pub fn pack_manifests(&self) -> &[(String, crate::pack::Manifest)] {
        &self.pack_manifests
    }

    /// Advisory embedding model/dimension mismatches for attached packs.
    #[must_use]
    pub fn pack_embedding_warnings(&self) -> Vec<String> {
        let consumer_model = self
            .embedding_config
            .model_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|p| p.to_string_lossy().to_string());
        self.pack_manifests.iter().filter_map(|(alias, manifest)| {
            let counts: serde_json::Value = serde_json::from_str(&manifest.counts).ok()?;
            let model = counts.get("embedding_model").and_then(|v| v.as_str()).map(str::to_string);
            let dim = counts.get("embedding_dimension").and_then(serde_json::Value::as_u64)
                .and_then(|n| usize::try_from(n).ok());
            if model != consumer_model || dim != Some(self.embedding_config.dimension) {
                Some(format!(
                    "attached pack {alias} embedding model/dimension {:?}/{:?} differs from consumer {:?}/{}; vectors are not converted",
                    model, dim, consumer_model, self.embedding_config.dimension
                ))
            } else { None }
        }).collect()
    }

    /// Recompute content hashes for every attached pack on explicit request.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn verify_attached_pack_hashes(&self) -> Result<Vec<(String, bool)>> {
        self.pack_manifests
            .iter()
            .map(|(alias, _)| {
                let attachment = self
                    .attachments
                    .iter()
                    .find(|a| &a.alias == alias)
                    .ok_or_else(|| {
                        Error::Store(format!("pack manifest has no attachment: {alias}"))
                    })?;
                let (_, _, ok) = crate::pack::verify(&attachment.path)?;
                Ok((alias.clone(), ok))
            })
            .collect()
    }

    /// Refuse a write aimed at a graph that lives in an attached database.
    ///
    /// **This is the only mechanism covering this case** — it reads like a
    /// third redundant layer and is not. `mode=ro` stops a write to the
    /// attached FILE, and unqualified table names route writes to `main`; but
    /// routing to `main` is precisely what makes this defect. Measured with
    /// this check removed: the write succeeds, one row lands in `main.facts`
    /// carrying the attached graph's id, the attached file is untouched, and
    /// every composed query then reads a local fact as though the layer had
    /// supplied it. Nothing errors anywhere.
    ///
    /// # Errors
    /// [`Error::Store`] naming the graph and the attachment that owns it.
    pub fn assert_graph_is_writable(&self, graph: i64) -> Result<()> {
        if graph == crate::schema::ROOT_GRAPH {
            return Ok(());
        }
        // Frozen first (deep freeze): a frozen graph's rows have been
        // relocated into a read-only pack, and a local write tagged with its
        // id would sit BESIDE the archive — read as if the archive supplied
        // it, exactly the failure the attached-source check below stops.
        // Checked unconditionally (not gated on attachments) so the refusal
        // holds even mid-freeze or with the pack temporarily absent.
        let row: Option<(Option<String>, Option<String>)> = self
            .conn
            .query_row(
                "SELECT lifecycle, source FROM main.graphs WHERE g = ?1",
                params![graph],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((lifecycle, source)) = row else {
            return Ok(());
        };
        if lifecycle.as_deref() == Some("frozen") {
            let iri = self.graph_display_name(graph);
            return Err(Error::Store(format!(
                "refusing to write to graph '{iri}': it is FROZEN — its rows \
                 live in a read-only archive pack. Thaw it first: \
                 `quipu graph thaw {iri}`."
            )));
        }
        if let Some(alias) = source {
            return Err(Error::Store(format!(
                "refusing to write to graph {graph}: it belongs to the attached \
                 database {alias:?}, which is mounted read-only and is another \
                 owner's artifact. Write to a local graph instead."
            )));
        }
        Ok(())
    }

    /// Wrap an already-opened connection in a default-configured `Store`,
    /// running no DDL. Split out of `init` so `open_read_only` can share the
    /// struct construction without sharing the schema setup — the two must not
    /// drift, or a pooled reader would be a different kind of Store than the
    /// writer.
    pub(super) fn with_connection(conn: Connection) -> Self {
        // Persistent lexical triggers can be present even when query mode is
        // off. Register their decoder on every writer and pooled reader.
        let _ = crate::lexical::register(&conn);
        // aegis-tl2q4j: the string-FILTER narrowing functions. A failed
        // registration cannot answer wrongly: a narrowed query then fails
        // loudly with "no such function".
        let _ = crate::sparql::string_pushdown::register(&conn);
        let _ = crate::sparql::status_pushdown::register(&conn);
        Self {
            conn,
            ownership_fence: None,
            signing: None,
            embedding_provider: None,
            embedding_config: EmbeddingConfig::default(),
            resolution_config: ResolutionConfig::default(),
            resolution_cache: Default::default(),
            search_config: SearchConfig::default(),
            labels_config: crate::config::LabelsConfig::default(),
            import_policy_config: crate::config::ImportPolicyConfig::default(),
            shacl_config: ShaclConfig::default(),
            projection_defs: Vec::new(),
            write_shapes: None,
            write_reference_graphs: None,
            poststate_profile: PoststateProfile::default(),
            current_g_index: std::cell::Cell::new(None),
            owl_config: OwlConfig::default(),
            #[cfg(feature = "owl")]
            owl_cache: None,
            governance_config: GovernanceConfig::default(),
            pending_write_events: std::cell::RefCell::new(Vec::new()),
            policy_registry: None,
            pending_verdicts: Vec::new(),
            pending_requests: Vec::new(),
            pending_refusal: None,
            pending_quarantine: None,
            replay_capture: None,
            gate_clock: None,
            principal_chain: Vec::new(),
            recording_verdicts: false,
            trust_root_bootstrap: None,
            base_ns: crate::namespace::DEFAULT_BASE_NS.to_string(),
            namespace_settings: crate::entity_namespaces::Settings::default(),
            vector_delegate: None,
            local_vector_backend: None,
            defer_auto_embed: false,
            pending_embed: None,
            attachments: Vec::new(),
            pack_manifests: Vec::new(),
            acquisition_anchors: std::cell::RefCell::new(std::collections::BTreeMap::new()),
            acquisition_incarnation: crate::acquisition::composed::new_incarnation(),
            facts_source: "facts".to_string(),
            resolve_sql: attach::RESOLVE_SQL_LOCAL.to_string(),
            term_cache: std::cell::RefCell::new(TermCache::default()),
            read_model: std::cell::RefCell::new(std::collections::HashMap::new()),
            projected_graph: std::cell::RefCell::new(None),
            read_model_enabled: std::cell::Cell::new(true),
            write_in_progress: std::cell::Cell::new(false),
            read_model_max_triples: std::cell::Cell::new(
                read_model::DEFAULT_READ_MODEL_MAX_TRIPLES,
            ),
            affordability_counts: std::cell::RefCell::new(std::collections::HashMap::new()),
            attached_graphs: Vec::new(),
            composition_suspended: std::cell::Cell::new(false),
            #[cfg(feature = "reactive-reasoner")]
            observers: Vec::new(),
        }
    }

    /// Whether any database is attached — the predicate that decides whether
    /// `facts_source()` is a plain table and `canonical_id` is the identity.
    #[must_use]
    pub fn has_attachments(&self) -> bool {
        !self.composition_suspended.get() && !self.attachments.is_empty()
    }
}

/// Restores the composed view when a suspended query ends. See
/// [`Store::scope_composition`].
pub(crate) struct CompositionGuard<'a> {
    store: &'a Store,
    prior: bool,
}

impl Drop for CompositionGuard<'_> {
    fn drop(&mut self) {
        self.store.composition_suspended.set(self.prior);
    }
}

/// See [`Store::open_immutable`]. A bare connection, for callers that read
/// raw tables (pack manifests, full-pack hashing).
pub(crate) fn open_file_immutable(path: &str) -> Result<Connection> {
    use rusqlite::OpenFlags;
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI;
    if std::path::Path::new(&format!("{path}-wal")).exists() {
        return Ok(Connection::open_with_flags(path, flags)?);
    }
    // Escape the characters a `file:` URI treats specially in a path.
    let escaped = path
        .replace('%', "%25")
        .replace('?', "%3f")
        .replace('#', "%23");
    Ok(Connection::open_with_flags(
        format!("file:{escaped}?immutable=1"),
        flags,
    )?)
}

#[cfg(test)]
#[path = "open_memory_tests.rs"]
mod memory_tests;

/// A random version-4 UUID as a `urn:uuid:` IRI. `what` names the failure.
pub(super) fn new_urn_uuid(what: &str) -> Result<String> {
    let mut bytes = [0_u8; 16];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| Error::Store(format!("cannot generate {what}")))?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = hex::encode(bytes);
    Ok(format!(
        "urn:uuid:{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}
