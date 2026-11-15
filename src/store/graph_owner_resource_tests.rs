use super::*;
use crate::read_authorization::{ComposedReadAdmission, ReadOperation};

fn single_query_reservation() -> crate::read_authorization::ComposedReadReservation {
    crate::read_authorization::ComposedReadReservation {
        query_bytes: 64 * 1_048_576,
        ..composition_reservation()
    }
}

#[test]
fn composed_output_lease_outlives_view_and_blocks_overlapping_evaluation() {
    let temp = tempfile::tempdir().unwrap();
    let (a, b) = strict_fixture(temp.path());
    let admission =
        ComposedReadAdmission::new(a.catalog.clone(), 512, 1, 1, single_query_reservation())
            .unwrap();
    let (policy, identity) = composition_authority(&[&a.route.graph, &b.route.graph]);
    let policy = policy.snapshot().unwrap();
    let output = Owner::read_strict(&[&a, &b], strict_deadline(), |reads| {
        let view = policy
            .authorize(&identity, ReadOperation::Query, None, 2)?
            .open_composed_cut(reads, admission.try_reserve()?, 2, strict_deadline())?;
        // Busy scratch admission must not leak temporary byte reservations.
        for _ in 0..5 {
            assert!(admission.try_reserve().is_err());
        }
        let output = view.query("ASK {}", false, 2)?;
        assert!(view.query("ASK {}", true, 2).is_err());
        assert_eq!(output.content_type(), "application/sparql-results+json");
        assert!(
            serde_json::from_slice::<Value>(&output).unwrap()["boolean"]
                .as_bool()
                .unwrap()
        );
        Ok(output)
    })
    .unwrap();
    // The scratch connection is gone, but its response still owns query bytes.
    Owner::read_strict(&[&a, &b], strict_deadline(), |reads| {
        let view = policy
            .authorize(&identity, ReadOperation::Query, None, 2)?
            .open_composed_cut(reads, admission.try_reserve()?, 2, strict_deadline())?;
        assert!(view.query("ASK {}", false, 2).is_err());
        drop(output);
        let fresh = view.query("ASK {}", true, 2)?;
        assert!(view.query("ASK {}", false, 2).is_err());
        drop(fresh);
        assert!(view.query("ASK {}", false, 2).is_ok());
        Ok(())
    })
    .unwrap();
    assert!(admission.try_reserve().is_ok());
}

#[test]
fn composed_overflow_parse_and_scope_errors_release_query_lease() {
    let temp = tempfile::tempdir().unwrap();
    let (a, b) = strict_fixture(temp.path());
    a.knot(&request(&a), &input(&a, &"x".repeat(128))).unwrap();
    let reservation = crate::read_authorization::ComposedReadReservation {
        max_response_bytes: 64,
        ..single_query_reservation()
    };
    let admission = ComposedReadAdmission::new(a.catalog.clone(), 512, 1, 1, reservation).unwrap();
    let (policy, identity) = composition_authority(&[&a.route.graph, &b.route.graph]);
    let policy = policy.snapshot().unwrap();
    Owner::read_strict(&[&a, &b], strict_deadline(), |reads| {
        let view = policy
            .authorize(&identity, ReadOperation::Query, None, 2)?
            .open_composed_cut(reads, admission.try_reserve()?, 2, strict_deadline())?;
        for query in [
            "SELECT ?s ?p ?o WHERE { ?s ?p ?o }",
            "garbage",
            "ASK { GRAPH <urn:hidden> { ?s ?p ?o } }",
        ] {
            assert!(view.query(query, false, 2).is_err());
            assert!(view.query("ASK {}", false, 2).is_ok());
        }
        assert!(view.query(&" ".repeat(65_537), false, 2).is_err());
        assert!(view.query("ASK {}", false, 2).is_ok());
        Ok(())
    })
    .unwrap();
    assert!(admission.try_reserve().is_ok());
}

#[test]
fn composed_reservation_refuses_zero_overflow_and_runtime_overcommit() {
    use crate::read_authorization::ComposedReadReservation as R;
    let temp = tempfile::tempdir().unwrap();
    let (a, _) = strict_fixture(temp.path());
    let good = single_query_reservation();
    for bad in [
        R {
            runtime_bytes: 0,
            ..good
        },
        R {
            view_bytes: 0,
            ..good
        },
        R {
            query_bytes: 0,
            ..good
        },
        R {
            max_response_bytes: 0,
            ..good
        },
        R {
            runtime_bytes: u64::MAX,
            ..good
        },
        R {
            view_bytes: u64::MAX,
            ..good
        },
        R {
            query_bytes: u64::MAX,
            ..good
        },
        R {
            max_response_bytes: usize::MAX,
            ..good
        },
        R {
            runtime_bytes: 129 * 1_048_576,
            ..good
        },
    ] {
        assert!(ComposedReadAdmission::new(a.catalog.clone(), 512, 1, 1, bad).is_err());
    }
    assert!(ComposedReadAdmission::new(a.catalog.clone(), 512, 1, 1, good).is_ok());
}
