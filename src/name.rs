//! Source-visible name values.

use std::fmt;

/// A decoded ShapeSQL or catalog name value.
///
/// Regular source identifiers are ASCII-folded before becoming a `Name`;
/// catalog names and delimited identifiers retain the value supplied by their
/// producer.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name(String);

impl Name {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl From<&str> for Name {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for Name {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for Name {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
