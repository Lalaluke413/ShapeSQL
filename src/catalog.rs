//! Immutable compile-time catalog snapshots.

use crate::{Name, TypeDescriptor};

/// An opaque host identity copied into Shape IR `input` nodes.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelationBinding(String);

impl RelationBinding {
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

impl From<&str> for RelationBinding {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for RelationBinding {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// An ordered field declaration in a catalog relation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogField {
    pub name: Name,
    pub descriptor: TypeDescriptor,
}

impl CatalogField {
    pub fn new(name: impl Into<Name>, descriptor: TypeDescriptor) -> Self {
        Self {
            name: name.into(),
            descriptor,
        }
    }
}

/// One source-visible relation declaration in a catalog snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogRelation {
    pub name: Name,
    pub binding: RelationBinding,
    pub fields: Vec<CatalogField>,
}

impl CatalogRelation {
    pub fn new(
        name: impl Into<Name>,
        binding: impl Into<RelationBinding>,
        fields: Vec<CatalogField>,
    ) -> Self {
        Self {
            name: name.into(),
            binding: binding.into(),
            fields,
        }
    }
}

/// A materialized, immutable compile-time relation namespace.
///
/// A `Catalog` contains declarations only. It performs no I/O and does not
/// provide relation rows, statistics, authorization checks, or transaction
/// state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    relations: Vec<CatalogRelation>,
}

impl Catalog {
    pub fn new(relations: Vec<CatalogRelation>) -> Self {
        Self { relations }
    }

    pub fn relations(&self) -> &[CatalogRelation] {
        &self.relations
    }

    pub fn push(&mut self, relation: CatalogRelation) {
        self.relations.push(relation);
    }
}

impl FromIterator<CatalogRelation> for Catalog {
    fn from_iter<T: IntoIterator<Item = CatalogRelation>>(iter: T) -> Self {
        Self::new(iter.into_iter().collect())
    }
}
