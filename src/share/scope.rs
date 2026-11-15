//! The graph slice a share covers, with its stable `{kind, value}` wire form.

use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize};

/// The graph slice written into a share.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ShareScope {
    /// ROOT/default graph.
    #[default]
    Root,
    /// One named graph IRI.
    Graph(String),
    /// Facts attributed to one episode provenance group.
    Group(String),
    /// A SPARQL CONSTRUCT or DESCRIBE result.
    Construct(String),
}

impl Serialize for ShareScope {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (kind, value) = match self {
            Self::Root => ("root", None),
            Self::Graph(value) => ("graph", Some(value.as_str())),
            Self::Group(value) => ("group", Some(value.as_str())),
            Self::Construct(value) => ("construct", Some(value.as_str())),
        };
        let mut state = serializer.serialize_struct("ShareScope", 2)?;
        state.serialize_field("kind", kind)?;
        state.serialize_field("value", &value)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for ShareScope {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            kind: String,
            value: Option<String>,
        }
        let wire = Wire::deserialize(deserializer)?;
        match (wire.kind.as_str(), wire.value) {
            ("root", None) => Ok(Self::Root),
            ("graph", Some(value)) => Ok(Self::Graph(value)),
            ("group", Some(value)) => Ok(Self::Group(value)),
            ("construct", Some(value)) => Ok(Self::Construct(value)),
            (kind, value) => Err(serde::de::Error::custom(format!(
                "invalid share scope kind={kind:?} value={value:?}"
            ))),
        }
    }
}
