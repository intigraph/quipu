//! Differential controls for cached scan statements and the memoized
//! `idx_current_g` check (aegis-ek4): randomized seeds-shaped boards answer
//! every board aggregate exactly as the uncached evaluator does, on a cold and
//! a warm cache, after writes, and across schema changes.
use crate::Store;
use crate::sparql::{Bindings, QueryResult, query};
use oxrdfio::RdfFormat;

const TS: &str = "2026-10-27T00:00:00Z";
const EX: &str = "http://example.org/";
const BOARD: &str = "http://example.org/board";
const EPHEMERAL: &str = "http://example.org/board#ephemeral";

/// xorshift64*: deterministic, dependency-free.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn ttl(rng: &mut Rng, items: usize, offset: usize) -> String {
    const STATUS: [&str; 7] = [
        "\"open\"",
        "\"in_progress\"",
        "\"closed\"",
        "\"tombstone\"",
        "\"blocked\"",
        "\"closed\"@en",
        "<http://example.org/odd>",
    ];
    const PRIORITY: [&str; 7] = ["0", "1", "2", "3", "\"4\"^^xsd:integer", "\"x\"", "1.5"];
    let mut out = String::new();
    for k in 0..items {
        let i = offset + k;
        // Duplicate identifiers and identifier-less subjects both occur.
        let id = rng.below((items as u64) * 3 / 4 + 1);
        out += &format!("ex:i{i} a ex:Issue ; ex:id \"s-{id}\" ");
        for _ in 0..rng.below(3) {
            out += &format!("; ex:status {} ", STATUS[rng.below(7) as usize]);
        }
        for _ in 0..rng.below(3) {
            out += &format!("; ex:priority {} ", PRIORITY[rng.below(7) as usize]);
        }
        match rng.below(4) {
            0 => {}
            1 => {
                out += &format!(
                    "; ex:agent <https://seeds.local/principal/a{}> ",
                    rng.below(5)
                );
            }
            2 => out += &format!("; ex:agent \"a{}\" ", rng.below(5)),
            _ => out += "; ex:agent <http://example.org/stranger> ",
        }
        for _ in 0..rng.below(4) {
            out += &format!("; ex:label \"l{}\" ", rng.below(4));
        }
        if rng.below(3) == 0 {
            out += &format!(
                "; ex:closed \"2026-10-{:02}T10:00:00Z\"^^xsd:dateTime ",
                1 + rng.below(9)
            );
        }
        out += ".\n";
    }
    out
}

fn ingest(store: &mut Store, graph: i64, body: &str) {
    crate::rdf::ingest_rdf_to_graph(
        store,
        format!("@prefix ex: <{EX}> . @prefix xsd: <http://www.w3.org/2001/XMLSchema#> . {body}")
            .as_bytes(),
        RdfFormat::Turtle,
        None,
        TS,
        None,
        None,
        graph,
    )
    .unwrap();
}

fn board(seed: u64) -> Store {
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut store = Store::open_in_memory().unwrap();
    let main = store.graph_create(BOARD).unwrap();
    let eph = store.graph_create(EPHEMERAL).unwrap();
    let n = 20 + rng.below(40) as usize;
    let body = ttl(&mut rng, n, 0);
    ingest(&mut store, main, &body);
    let body = ttl(&mut rng, n / 4, n);
    ingest(&mut store, eph, &body);
    store
}

// Seeds' effective-cohort shapes (summary.rs), over the test vocabulary.
fn effective(fields: &str) -> String {
    let base = "?s a ex:Issue ; ex:id ?id .";
    format!(
        "{{ GRAPH <{BOARD}> {{ {base} {fields} FILTER(isLiteral(?id) && sameTerm(?id, STR(?id))) }} \
         FILTER NOT EXISTS {{ GRAPH <{EPHEMERAL}> {{ ?shadow a ex:Issue ; ex:id ?id }} }} }} \
         UNION {{ GRAPH <{EPHEMERAL}> {{ {base} {fields} FILTER(isLiteral(?id) && sameTerm(?id, STR(?id))) }} }}"
    )
}

fn normalized(fields: &str) -> String {
    let status =
        "?s ex:status ?status . FILTER(isLiteral(?status) && sameTerm(?status, STR(?status)))";
    let missing = "FILTER NOT EXISTS { ?s ex:status ?raw_status FILTER(isLiteral(?raw_status) && sameTerm(?raw_status, STR(?raw_status))) } BIND(\"open\" AS ?status)";
    format!(
        "{{ {} }} UNION {{ {} }}",
        effective(&format!("{status} {fields}")),
        effective(&format!("{missing} {fields}"))
    )
}

fn queries() -> Vec<String> {
    let g = |body: &str| format!("GRAPH <{BOARD}> {{ {body} }}");
    let item = "?s a ex:Issue ; ex:id ?id ; ex:status ?status .";
    let active = "FILTER(?status != \"tombstone\")";
    let principal = "(isIRI(?raw_key) && STRSTARTS(STR(?raw_key), \"https://seeds.local/principal/\")) || (isLiteral(?raw_key) && sameTerm(?raw_key, STR(?raw_key)))";
    let guard =
        "sameTerm(?raw_key - ?raw_key, 0) && sameTerm(?raw_key, ?raw_key + 0) && ?raw_key >= 0";
    let grouped = |fields: &str, key: &str| {
        format!(
            "SELECT ?{key} (COUNT(DISTINCT ?id) AS ?n) WHERE {{ {} }} GROUP BY ?{key} LIMIT 1000",
            normalized(fields)
        )
    };
    vec![
        format!("SELECT ?status (COUNT(*) AS ?n) WHERE {{ {} }} GROUP BY ?status", g(item)),
        format!(
            "SELECT ?status ?p (COUNT(*) AS ?n) WHERE {{ {} }} GROUP BY ?status ?p",
            g(&format!("{item} ?s ex:priority ?p ."))
        ),
        // GROUP BY on an expression, and an unbound key from OPTIONAL.
        format!(
            "SELECT ?a (COUNT(?s) AS ?n) WHERE {{ {} }} GROUP BY (STR(?agent) AS ?a)",
            g(&format!("{item} OPTIONAL {{ ?s ex:agent ?agent }}"))
        ),
        format!(
            "SELECT ?d (COUNT(?s) AS ?n) WHERE {{ {} }} GROUP BY (SUBSTR(STR(?c), 1, 10) AS ?d)",
            g("?s ex:closed ?c .")
        ),
        // Non-COUNT aggregates, including type errors and DISTINCT.
        format!(
            "SELECT ?status (SUM(?p) AS ?sum) (AVG(?p) AS ?avg) (MIN(?p) AS ?lo) (MAX(?p) AS ?hi) (SAMPLE(?id) AS ?one) WHERE {{ {} }} GROUP BY ?status",
            g(&format!("{item} ?s ex:priority ?p ."))
        ),
        format!(
            "SELECT ?status (COUNT(DISTINCT ?l) AS ?nl) (GROUP_CONCAT(DISTINCT ?l ; separator=\",\") AS ?ls) (SUM(?l) AS ?bad) WHERE {{ {} }} GROUP BY ?status",
            g(&format!("{item} ?s ex:label ?l ."))
        ),
        // Three nested steps that issue the same scan text.
        format!(
            "SELECT ?l (COUNT(*) AS ?n) WHERE {{ {} }} GROUP BY ?l",
            g("?s ex:id ?id . ?s ex:status ?st . ?s ex:label ?l . ?s ex:priority ?p .")
        ),
        format!("SELECT ?s ?st WHERE {{ {} }} ORDER BY ?s ?st", g("?s ex:id ?id ; ex:status ?st .")),
        // The exact `sd stats` breakdowns.
        grouped("", "status"),
        grouped(
            &format!("{active} OPTIONAL {{ ?s ex:priority ?raw_key . FILTER({guard} && ?raw_key <= 255) }} BIND(CONCAT(\"P\", STR(COALESCE(?raw_key, 2))) AS ?key)"),
            "key",
        ),
        grouped(
            &format!("{active} OPTIONAL {{ ?s ex:agent ?raw_key . FILTER({principal}) }} BIND(COALESCE(?raw_key, \"(unassigned)\") AS ?key)"),
            "key",
        ),
        grouped(
            &format!("{active} OPTIONAL {{ ?s ex:label ?raw_key . FILTER(isLiteral(?raw_key) && sameTerm(?raw_key, STR(?raw_key))) }} BIND(COALESCE(?raw_key, \"(no labels)\") AS ?key)"),
            "key",
        ),
    ]
    .into_iter()
    .map(|q| format!("PREFIX ex: <{EX}> {q}"))
    .collect()
}

fn answer(store: &Store, q: &str) -> (Vec<String>, Vec<Bindings>) {
    match query(store, q).unwrap_or_else(|e| panic!("{q}: {e}")) {
        QueryResult::Select { variables, rows } => (variables, rows),
        other => panic!("expected SELECT: {other:?}"),
    }
}

/// Every query on the uncached evaluator: no statement survives its use.
fn uncached(store: &Store) -> Vec<(Vec<String>, Vec<Bindings>)> {
    store.conn.set_prepared_statement_cache_capacity(0);
    let out = queries().iter().map(|q| answer(store, q)).collect();
    store.conn.set_prepared_statement_cache_capacity(256);
    out
}

fn assert_cached_matches(store: &Store, label: &str) {
    let expected = uncached(store);
    // Cold, then warm: the second pass reuses every cached statement.
    for pass in ["cold", "warm"] {
        if pass == "cold" {
            store.conn.flush_prepared_statement_cache();
        }
        for (q, want) in queries().iter().zip(&expected) {
            assert_eq!(&answer(store, q), want, "{label} {pass}: {q}");
        }
    }
}

#[test]
fn randomized_boards_match_the_uncached_evaluator() {
    for seed in 1..=16 {
        let mut store = board(seed);
        assert_cached_matches(&store, &format!("seed {seed}"));
        // Writes between queries: cached statements read the new state.
        let mut rng = Rng(seed ^ 0xdead_beef);
        let main = store.graph_create(BOARD).unwrap();
        let body = ttl(&mut rng, 5, 1000);
        ingest(&mut store, main, &body);
        let status = store.lookup(&format!("{EX}status")).unwrap().unwrap();
        for i in 0..4 {
            if let Some(entity) = store.lookup(&format!("{EX}i{i}")).unwrap() {
                store
                    .retract_entity(entity, Some(status), TS, None)
                    .unwrap();
            }
        }
        assert_cached_matches(&store, &format!("seed {seed} after writes"));
    }
}

#[test]
fn current_graph_index_memo_follows_schema_changes() {
    let store = board(7);
    let expected = uncached(&store);
    let check = |label: &str| {
        for (q, want) in queries().iter().zip(&expected) {
            assert_eq!(&answer(&store, q), want, "{label}: {q}");
        }
    };
    let has_index = || {
        store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='idx_current_g'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
            == 1
    };
    assert!(has_index(), "fixture store carries idx_current_g");
    check("memo primed");
    assert_eq!(
        store.current_g_index.get().map(|(_, exact)| exact),
        Some(true)
    );
    // Dropped: a stale memo would name a missing index and fail the query.
    store
        .conn
        .execute_batch("DROP INDEX idx_current_g")
        .unwrap();
    check("index dropped");
    assert_eq!(
        store.current_g_index.get().map(|(_, exact)| exact),
        Some(false)
    );
    // Same name, another partial predicate: it proves nothing; keep fallback.
    store
        .conn
        .execute_batch("CREATE INDEX idx_current_g ON facts(g) WHERE op = 1")
        .unwrap();
    check("index redefined");
    assert_eq!(
        store.current_g_index.get().map(|(_, exact)| exact),
        Some(false)
    );
    store.conn.execute_batch("DROP INDEX idx_current_g; CREATE INDEX idx_current_g ON facts(g) WHERE op=1 AND valid_to IS NULL").unwrap();
    check("index restored");
    assert_eq!(
        store.current_g_index.get().map(|(_, exact)| exact),
        Some(true)
    );
}

#[test]
fn nested_scans_of_one_text_each_keep_a_cached_statement() {
    let store = Store::open_in_memory().unwrap();
    {
        let (outer, _a) = super::prepare(&store, "SELECT 1").unwrap();
        let (inner, _b) = super::prepare(&store, "SELECT 1").unwrap();
        let (deeper, _c) = super::prepare(&store, "SELECT 1").unwrap();
        assert_eq!(outer.expanded_sql().unwrap(), "SELECT 1");
        assert_eq!(inner.expanded_sql().unwrap(), "/* scan depth 1 */ SELECT 1");
        assert_eq!(
            deeper.expanded_sql().unwrap(),
            "/* scan depth 2 */ SELECT 1"
        );
    }
    super::OPEN_SCANS.with(|open| assert_eq!(open.get(), 0, "every scan closed"));
    // A board query with three same-text steps leaves no scan open either.
    let store = board(3);
    answer(&store, &queries()[6]);
    super::OPEN_SCANS.with(|open| assert_eq!(open.get(), 0));
}
