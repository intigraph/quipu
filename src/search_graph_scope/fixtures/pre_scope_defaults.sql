-- Frozen accepted 0a20 schema statements, comments removed.
CREATE TABLE IF NOT EXISTS terms (
    id  INTEGER PRIMARY KEY,
    iri TEXT    NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS transactions (
    id        INTEGER PRIMARY KEY,
    timestamp TEXT    NOT NULL,
    actor     TEXT,
    source    TEXT
);
CREATE TABLE IF NOT EXISTS transaction_auth (
    tx INTEGER PRIMARY KEY REFERENCES transactions(id),
    principal TEXT NOT NULL,
    credential_id TEXT,
    auth_class TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS store_identity (
    id       INTEGER PRIMARY KEY CHECK (id = 1),
    store_id TEXT NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS facts (
    e         INTEGER NOT NULL,
    a         INTEGER NOT NULL,
    v         BLOB    NOT NULL,
    g         INTEGER NOT NULL DEFAULT 0,
    tx        INTEGER NOT NULL REFERENCES transactions(id),
    valid_from TEXT   NOT NULL,
    valid_to   TEXT,
    op        INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (e, a, v, tx)
);
CREATE INDEX IF NOT EXISTS idx_aevt ON facts(a, e, v, valid_from);
CREATE INDEX IF NOT EXISTS idx_vaet ON facts(v, a, e, valid_from);
CREATE INDEX IF NOT EXISTS idx_tx   ON facts(tx);
CREATE TABLE IF NOT EXISTS graphs (
    g             INTEGER PRIMARY KEY,
    class         TEXT    NOT NULL CHECK (class IN ('committed','overlay')),
    parent_branch INTEGER REFERENCES graphs(g),
    created_at    TEXT    NOT NULL
);
INSERT OR IGNORE INTO graphs (g, class, parent_branch, created_at)
    VALUES (0, 'committed', NULL, '1970-01-01T00:00:00Z');
CREATE TABLE IF NOT EXISTS shapes (
    name      TEXT PRIMARY KEY,
    turtle    TEXT NOT NULL,
    loaded_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS proposals (
    id            INTEGER PRIMARY KEY,
    kind          TEXT NOT NULL,
    target        TEXT NOT NULL,
    diff          TEXT NOT NULL,
    rationale     TEXT,
    proposer      TEXT NOT NULL,
    trigger_ref   TEXT,
    status        TEXT NOT NULL DEFAULT 'pending',
    decided_by    TEXT,
    decided_at    TEXT,
    decision_note TEXT,
    created_at    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_proposals_status ON proposals(status, created_at);
CREATE TABLE IF NOT EXISTS ontologies (
    name      TEXT PRIMARY KEY,
    turtle    TEXT NOT NULL,
    loaded_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS events (
    "offset"  INTEGER PRIMARY KEY AUTOINCREMENT,
    type      TEXT    NOT NULL,
    ts        TEXT    NOT NULL,
    subject   TEXT,
    group_id  TEXT,
    tx_id     INTEGER NOT NULL,
    payload   TEXT    NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS idx_events_type  ON events(type, "offset");
CREATE INDEX IF NOT EXISTS idx_events_group ON events(group_id, "offset");
CREATE TABLE IF NOT EXISTS import_reviews (
    share_id TEXT PRIMARY KEY,
    state TEXT NOT NULL,
    first_seen TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    notice_policy TEXT,
    payload TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_import_reviews_pending ON import_reviews(state, first_seen);
CREATE TABLE IF NOT EXISTS consumers (
    consumer_id      TEXT PRIMARY KEY,
    committed_offset INTEGER NOT NULL DEFAULT 0,
    filter           TEXT,
    updated_at       TEXT
);
CREATE TABLE IF NOT EXISTS snapshot_uploads (
    upload_id    TEXT PRIMARY KEY,
    snapshot     TEXT    NOT NULL,
    content_hash TEXT    NOT NULL,
    total_parts  INTEGER NOT NULL,
    total_bytes  INTEGER NOT NULL,
    timestamp    TEXT    NOT NULL,
    actor        TEXT,
    source       TEXT,
    graph        TEXT,
    created_at   INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS snapshot_upload_parts (
    upload_id   TEXT    NOT NULL REFERENCES snapshot_uploads(upload_id) ON DELETE CASCADE,
    part_number INTEGER NOT NULL,
    part_hash   TEXT    NOT NULL,
    byte_count  INTEGER NOT NULL,
    payload     BLOB    NOT NULL,
    PRIMARY KEY (upload_id, part_number)
);
CREATE INDEX IF NOT EXISTS idx_snapshot_uploads_expiry ON snapshot_uploads(expires_at);
CREATE TABLE IF NOT EXISTS subscriptions (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    consumer_id   TEXT    NOT NULL UNIQUE,
    types         TEXT,
    sparql_ask    TEXT,
    mode          TEXT    NOT NULL DEFAULT 'realtime',
    webhook_url   TEXT    NOT NULL,
    batch_size    INTEGER NOT NULL DEFAULT 50,
    batch_window_s INTEGER NOT NULL DEFAULT 30,
    created_at    TEXT    NOT NULL
);
CREATE TABLE IF NOT EXISTS schema_terms (
    term         TEXT NOT NULL,
    kind         TEXT NOT NULL CHECK (kind IN ('type','predicate')),
    first_offset INTEGER NOT NULL,
    PRIMARY KEY (term, kind)
);
CREATE TABLE IF NOT EXISTS denial_quarantine (
    id           INTEGER PRIMARY KEY,
    verdict      TEXT    NOT NULL,
    attempt      TEXT    NOT NULL,
    graph        TEXT    NOT NULL,
    base_tx      INTEGER NOT NULL,
    at           TEXT    NOT NULL,
    actor        TEXT,
    source       TEXT,
    chain        TEXT,
    gate_now     INTEGER NOT NULL,
    rules_digest TEXT    NOT NULL,
    post_digest  TEXT    NOT NULL,
    retention    TEXT    NOT NULL CHECK (retention IN ('digest','full')),
    verifier     TEXT    NOT NULL,
    seal         TEXT    NOT NULL,
    purged_at    TEXT,
    UNIQUE (verdict, attempt, base_tx)
);
CREATE INDEX IF NOT EXISTS idx_quarantine_verdict ON denial_quarantine(verdict);
CREATE INDEX IF NOT EXISTS idx_quarantine_graph ON denial_quarantine(graph, at);
CREATE TABLE IF NOT EXISTS quarantine_deltas (
    attempt TEXT PRIMARY KEY,
    delta   TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS term_spaces (
    space INTEGER PRIMARY KEY,
    db    TEXT    NOT NULL,
    local INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_term_spaces_local
    ON term_spaces(local) WHERE local = 1;
