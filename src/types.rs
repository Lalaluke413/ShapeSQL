//! Portable ShapeSQL scalar types and static descriptors.

/// A portable ShapeSQL 0.1 scalar type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScalarType {
    Boolean,
    Int64,
    Text,
}

/// The scalar type and nullability assigned during static analysis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TypeDescriptor {
    pub scalar: ScalarType,
    pub nullable: bool,
}

impl TypeDescriptor {
    pub const fn new(scalar: ScalarType, nullable: bool) -> Self {
        Self { scalar, nullable }
    }

    pub const fn non_nullable(scalar: ScalarType) -> Self {
        Self::new(scalar, false)
    }

    pub const fn nullable(scalar: ScalarType) -> Self {
        Self::new(scalar, true)
    }

    pub const fn with_nullable(self, nullable: bool) -> Self {
        Self { nullable, ..self }
    }
}
