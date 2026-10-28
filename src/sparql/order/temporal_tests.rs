//! ORDER BY on temporal literals sorts by value, not lexeme (aegis-neg part 2).
use super::*;

const XSD: &str = "http://www.w3.org/2001/XMLSchema#";

fn ids(rows: &[Bindings]) -> Vec<i64> {
    rows.iter()
        .map(|r| match r["id"] {
            Value::Int(n) => n,
            ref other => panic!("id {other:?}"),
        })
        .collect()
}

/// `(lexical, xsd local name, id)` rows as a `VALUES` block.
fn values(rows: &[(&str, &str, i64)]) -> String {
    rows.iter()
        .map(|(lexical, datatype, id)| {
            if datatype.is_empty() {
                format!("(\"{lexical}\" {id})")
            } else {
                format!("(\"{lexical}\"^^<{XSD}{datatype}> {id})")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// ids under `ORDER BY order`, complete and through the LIMIT top-k heap.
fn ordered(rows: &[(&str, &str, i64)], order: &str) -> Vec<i64> {
    let store = Store::open_in_memory().unwrap();
    let q = format!(
        "SELECT ?x ?id WHERE {{ VALUES (?x ?id) {{ {} }} }} ORDER BY {order}",
        values(rows)
    );
    let all = ids(crate::sparql::query(&store, &q).unwrap().rows());
    for k in 1..=rows.len() {
        let top = ids(crate::sparql::query(&store, &format!("{q} LIMIT {k}"))
            .unwrap()
            .rows());
        assert_eq!(top, all[..k], "LIMIT {k} prefix differs: {q}");
    }
    all
}

/// Same instant at 10:00Z twice (ids 1, 2), 11:00Z written as 06:00-05:00.
const MIXED: [(&str, &str, i64); 4] = [
    ("2026-10-20T06:00:00-05:00", "dateTime", 0),
    ("2026-10-20T10:00:00Z", "dateTime", 1),
    ("2026-10-20T12:00:00+02:00", "dateTime", 2),
    ("2026-10-20T09:30:00Z", "dateTime", 3),
];

#[test]
fn mixed_offset_datetimes_sort_by_instant_asc_and_desc() {
    // Lexically 06:00-05:00 sorts first; by instant it is the latest.
    assert_eq!(ordered(&MIXED, "?x"), vec![3, 1, 2, 0]);
    assert_eq!(ordered(&MIXED, "ASC(?x)"), vec![3, 1, 2, 0]);
    // Ties at one instant keep input order under DESC too (stable).
    assert_eq!(ordered(&MIXED, "DESC(?x)"), vec![0, 1, 2, 3]);
}

#[test]
fn same_instant_ties_are_stable_in_input_order() {
    let mut swapped = MIXED;
    swapped.swap(1, 2);
    assert_eq!(ordered(&swapped, "?x"), vec![3, 2, 1, 0]);
    // A second key breaks the tie; the instant still leads.
    assert_eq!(ordered(&MIXED, "?x DESC(?id)"), vec![3, 2, 1, 0]);
}

#[test]
fn dates_and_times_with_offsets_sort_by_value() {
    let dates = [
        ("2026-10-21+13:00", "date", 0), // starts 2026-10-20T11:00Z
        ("2026-10-20Z", "date", 1),
        ("2026-10-20-10:00", "date", 2), // starts 2026-10-20T10:00Z
    ];
    assert_eq!(ordered(&dates, "?x"), vec![1, 2, 0]);
    let times = [
        ("07:00:00-05:00", "time", 0), // 12:00Z
        ("11:00:00Z", "time", 1),
        ("13:30:00+02:00", "time", 2), // 11:30Z
    ];
    assert_eq!(ordered(&times, "?x"), vec![1, 2, 0]);
    assert_eq!(ordered(&times, "DESC(?x)"), vec![0, 2, 1]);
}

fn permutations(n: usize) -> Vec<Vec<usize>> {
    if n == 0 {
        return vec![vec![]];
    }
    let mut out = Vec::new();
    for p in permutations(n - 1) {
        for i in 0..=p.len() {
            let mut q = p.clone();
            q.insert(i, n - 1);
            out.push(q);
        }
    }
    out
}

/// The order of `rows` must not depend on the input order.
fn assert_order_independent(rows: &[(&str, &str, i64)], expected: &[i64]) {
    for perm in permutations(rows.len()) {
        let input: Vec<_> = perm.iter().map(|&i| rows[i]).collect();
        assert_eq!(ordered(&input, "?x"), expected, "input {input:?}");
        let mut desc = expected.to_vec();
        desc.reverse();
        assert_eq!(ordered(&input, "DESC(?x)"), desc, "input {input:?}");
    }
}

#[test]
fn indeterminate_pairs_order_deterministically_and_transitively() {
    // An untimezoned value within 14h of a timezoned one is indeterminate
    // (XSD partial order). ORDER BY places it as if it were UTC, and before
    // a timezoned value at that same instant. A lexical fallback for the
    // indeterminate pairs is a cycle here: 20:00-10:00 < 21:00 < 23:00Z
    // lexically, yet 20:00-10:00 (06:00Z next day) > 23:00Z by instant.
    assert_order_independent(
        &[
            ("2026-01-01T20:00:00-10:00", "dateTime", 0),
            ("2026-01-01T23:00:00Z", "dateTime", 1),
            ("2026-01-01T21:00:00", "dateTime", 2),
        ],
        &[2, 1, 0],
    );
    assert_order_independent(
        &[
            ("2026-10-20T10:00:00Z", "dateTime", 0),
            ("2026-10-20T10:00:00", "dateTime", 1),
            ("2026-10-20T09:00:00Z", "dateTime", 2),
            ("2026-10-20T11:00:00+05:00", "dateTime", 3),
        ],
        &[3, 2, 1, 0],
    );
}

#[test]
fn mixed_kinds_and_ill_typed_lexemes_order_deterministically() {
    // Temporals follow other literals; kinds go dateTime, date, time; within
    // a kind valid values (by value) precede ill-typed lexemes (lexically).
    assert_order_independent(
        &[
            ("2026-10-20T06:00:00-05:00", "dateTime", 0),
            ("zzz", "", 1),
            ("not-a-date", "dateTime", 2),
            ("2026-10-20T10:00:00Z", "dateTime", 3),
            ("2026-10-20", "date", 4),
        ],
        &[1, 3, 0, 2, 4],
    );
}

#[test]
fn non_temporal_ordering_is_unchanged() {
    let rows = [
        ("b", "", 0),
        ("a", "", 1),
        ("10", "integer", 2),
        ("9", "integer", 3),
        ("2026-10-20T10:00:00Z", "string", 4),
        ("2026-10-20T06:00:00-05:00", "string", 5),
    ];
    // Numerics among themselves by value; strings (even dateTime-shaped
    // xsd:string) lexically.
    let all = ordered(&rows, "?x");
    let pos = |id| all.iter().position(|&x| x == id).unwrap();
    assert!(pos(3) < pos(2));
    assert!(pos(1) < pos(0));
    assert!(pos(5) < pos(4));
}

#[test]
fn min_and_max_use_the_value_order() {
    let store = Store::open_in_memory().unwrap();
    let q = format!(
        "SELECT (MIN(?x) AS ?lo) (MAX(?x) AS ?hi) WHERE {{ VALUES (?x ?id) {{ {} }} }}",
        values(&MIXED)
    );
    let result = crate::sparql::query(&store, &q).unwrap();
    let row = &result.rows()[0];
    assert_eq!(row["lo"].as_lexical(), Some("2026-10-20T09:30:00Z"));
    assert_eq!(row["hi"].as_lexical(), Some("2026-10-20T06:00:00-05:00"));
}

/// The spilling sorter (more than 4096 rows) uses the same comparator as the
/// in-memory one: both match a reference stable sort by instant.
#[test]
fn spill_path_matches_in_memory_order() {
    let store = Store::open_in_memory().unwrap();
    let rows = |n: usize| -> (Vec<Bindings>, Vec<i64>) {
        let mut keyed = Vec::new();
        let rows = (0..n)
            .map(|i| {
                let instant = (i * 37) % 500; // minutes after 12:00Z
                let offset = (i % 5) as i64 - 2; // -02:00 ..= +02:00
                let local = 720 + instant as i64 + offset * 60;
                let sign = if offset < 0 { '-' } else { '+' };
                let lexical = format!(
                    "2026-10-20T{:02}:{:02}:00{sign}{:02}:00",
                    local / 60,
                    local % 60,
                    offset.abs()
                );
                keyed.push((instant, i as i64));
                Bindings::from([
                    (
                        "x".to_owned(),
                        Value::Typed {
                            lexical,
                            datatype: format!("{XSD}dateTime"),
                        },
                    ),
                    ("id".to_owned(), Value::Int(i as i64)),
                ])
            })
            .collect();
        keyed.sort_by_key(|&(instant, _)| instant); // stable
        (rows, keyed.into_iter().map(|(_, id)| id).collect())
    };
    let x =
        || spargebra::algebra::Expression::Variable(spargebra::term::Variable::new("x").unwrap());
    for n in [4000, 5000] {
        let (input, expected) = rows(n);
        let asc = sort_rows(
            &store,
            input.clone(),
            &[OrderExpression::Asc(x())],
            &TemporalContext::default(),
        )
        .unwrap();
        assert_eq!(ids(&asc), expected, "ASC n={n}");
        let desc = sort_rows(
            &store,
            input,
            &[OrderExpression::Desc(x())],
            &TemporalContext::default(),
        )
        .unwrap();
        // Stable DESC: groups reversed, ties keep input order.
        let mut groups: Vec<Vec<i64>> = Vec::new();
        let instant = |id: i64| (usize::try_from(id).unwrap() * 37) % 500;
        for id in expected {
            match groups.last_mut() {
                Some(g) if instant(g[0]) == instant(id) => g.push(id),
                _ => groups.push(vec![id]),
            }
        }
        groups.reverse();
        assert_eq!(ids(&desc), groups.concat(), "DESC n={n}");
    }
}
