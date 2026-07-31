//! Compilation-local semantic identities.

macro_rules! identity {
    ($(#[$metadata:meta])* $name:ident) => {
        $(#[$metadata])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub(crate) u32);

        impl $name {
            pub const fn index(self) -> u32 {
                self.0
            }

            pub const fn from_index(index: u32) -> Self {
                Self(index)
            }
        }
    };
}

identity!(
    /// A field identity unique within one compilation.
    FieldId
);
identity!(
    /// A common-table-expression declaration identity.
    CteId
);
identity!(
    /// A relation-source occurrence identity.
    RelationOccurrenceId
);
