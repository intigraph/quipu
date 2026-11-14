//! Bounded W3C output sink. The limit is checked before reserving or copying.
use std::io::{self, Write};

pub(super) struct Output {
    bytes: Vec<u8>,
    limit: usize,
}

impl Output {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
    pub(super) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

impl Write for Output {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = self
            .bytes
            .len()
            .checked_add(buf.len())
            .filter(|len| *len <= self.limit)
            .ok_or_else(|| io::Error::other("response byte reservation exceeded"))?;
        if len > self.bytes.capacity() {
            let capacity = len
                .max(self.bytes.capacity().saturating_mul(2))
                .min(self.limit);
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(io::Error::other)?;
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_before_growth_and_preserves_existing_bytes() {
        let mut out = Output::new(4);
        out.write_all(b"123").unwrap();
        let capacity = out.bytes.capacity();
        assert!(out.write_all(b"45").is_err());
        assert_eq!(out.bytes, b"123");
        assert_eq!(out.bytes.capacity(), capacity);
        out.write_all(b"4").unwrap();
        assert_eq!(out.into_bytes(), b"1234");
    }
    #[test]
    fn bounded_serializers_match_exact_output_and_refuse_one_byte_short() {
        use crate::{
            Store, Value,
            sparql::{Bindings, QueryResult, Triple},
            w3c::{ResultFormat as F, serialize, serialize_bounded},
        };
        let store = Store::open_in_memory().unwrap();
        let selected = QueryResult::Select {
            variables: vec!["value".into()],
            rows: vec![Bindings::from_iter([(
                "value".into(),
                Value::Str("quoted \" λ & < >".into()),
            )])],
        };
        let graph = QueryResult::Graph(vec![Triple {
            subject: "urn:subject".into(),
            predicate: "urn:predicate".into(),
            object: Value::Str("quoted \" λ & < >".into()),
        }]);
        for (result, formats) in [
            (&selected, vec![F::SparqlJson, F::SparqlXml, F::Csv, F::Tsv]),
            (
                &QueryResult::Ask(true),
                vec![F::SparqlJson, F::SparqlXml, F::Csv, F::Tsv],
            ),
            (&graph, vec![F::Turtle, F::NTriples]),
        ] {
            for fmt in formats {
                let ordinary = serialize(&store, result, fmt).unwrap().unwrap();
                let exact = serialize_bounded(&store, result, fmt, ordinary.1.len())
                    .unwrap()
                    .unwrap();
                assert_eq!(ordinary, exact);
                assert!(serialize_bounded(&store, result, fmt, ordinary.1.len() - 1).is_err());
                assert!(serialize_bounded(&store, result, fmt, 0).is_err());
            }
        }
    }
}
