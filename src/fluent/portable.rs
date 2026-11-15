//! Portable v1 observations, without claiming caller authentication.
use super::{Observation, Scalar, invalid};
use crate::Result;
use oxrdf::{NamedNode, Term};
use oxsdatatypes::DateTime;
use std::str::FromStr;

pub(super) fn validate(observation: &Observation) -> Result<()> {
    for identity in [
        &observation.fluent,
        &observation.order.event,
        &observation.order.writer,
    ] {
        NamedNode::new(identity.as_str())
            .map_err(|_| invalid("observation identities must be absolute IRIs"))?;
    }
    if observation.order.tx == 0 {
        return Err(invalid("source transaction must be positive"));
    }
    for timestamp in [&observation.as_of, &observation.recorded_on] {
        let value = DateTime::from_str(timestamp)
            .map_err(|_| invalid("observation time must be a date-time"))?;
        if value.timezone_offset().is_none() {
            return Err(invalid("observation time requires an explicit timezone"));
        }
    }
    if let Scalar::Term(text) = &observation.value {
        let term =
            Term::from_str(text).map_err(|_| invalid("observation value must be one RDF term"))?;
        if !matches!(term, Term::NamedNode(_) | Term::Literal(_)) {
            return Err(invalid("v1 values require an IRI or literal"));
        }
    }
    Ok(())
}
