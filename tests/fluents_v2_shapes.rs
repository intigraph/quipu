#![cfg(feature = "shacl")]
//! aegis-gv8: the proposed v2 successor closes three structural gaps that the
//! accepted v1 set states in its contract but does not enforce. Each gap case
//! is asserted against BOTH revisions so the test pins what v2 changes.

const V1: &str = include_str!("../shapes/fluents-v1.ttl");
const V2: &str = include_str!("../shapes/fluents-v2.ttl");
const PREFIXES: &str = "@prefix qp: <https://w3id.org/intigraph/quipu/ns#> .\n\
    @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n";
const FLUENT: &str = "<urn:f> a qp:Fluent ; qp:slotSubject <urn:s> ; qp:slotPredicate <urn:p> ; \
    qp:inGraph <urn:g> ; qp:propertyShape <urn:ps> ; qp:currentValuePolicy \"max\" .\n";

fn conforms(shapes: &str, data: &str) -> bool {
    quipu::validate_shapes(shapes, &format!("{PREFIXES}{FLUENT}{data}"))
        .unwrap()
        .conforms
}

fn observation(as_of: &str, value: &str) -> String {
    format!(
        "<urn:o> a qp:FluentObservation ; qp:forFluent <urn:f> ; qp:originTx 1 ; \
         qp:writer <urn:w> ; qp:asOf \"{as_of}\"^^xsd:dateTime ; \
         qp:recordedOn \"2026-11-13T00:00:00Z\"^^xsd:dateTime ; qp:observationValue {value} ."
    )
}

#[test]
fn v2_admits_what_v1_admits_for_conforming_writer_output() {
    for as_of in [
        "2026-11-13T00:00:00Z",
        "2026-11-13T00:00:00+02:00",
        "2026-11-13T00:00:00-05:30",
    ] {
        for value in ["5", "<urn:v>", "\"text\""] {
            let data = observation(as_of, value);
            assert!(conforms(V1, &data), "{as_of} {value}");
            assert!(conforms(V2, &data), "{as_of} {value}");
        }
    }
}

#[test]
fn v2_requires_timezone_qualified_times() {
    let data = observation("2026-11-13T00:00:00", "5");
    assert!(conforms(V1, &data), "the v1 gap this revision closes");
    assert!(!conforms(V2, &data));
}

#[test]
fn v2_refuses_a_blank_node_observation_value() {
    let data = observation("2026-11-13T00:00:00Z", "[ qp:x 1 ]");
    assert!(conforms(V1, &data), "the v1 gap this revision closes");
    assert!(!conforms(V2, &data));
}

#[test]
fn v2_requires_fluent_slot_fields_on_an_observable() {
    let bare = "<urn:ob> a qp:Observable ; qp:observes <urn:f> ; qp:computedBy \"max\" .";
    assert!(conforms(V1, bare), "the v1 gap this revision closes");
    assert!(!conforms(V2, bare));
    let full = "<urn:ob> a qp:Observable, qp:Fluent ; qp:slotSubject <urn:s> ; \
        qp:slotPredicate <urn:p> ; qp:inGraph <urn:g> ; qp:propertyShape <urn:ps> ; \
        qp:currentValuePolicy \"max\" ; qp:observes <urn:f> ; qp:computedBy \"max\" .";
    assert!(conforms(V2, full));
}
