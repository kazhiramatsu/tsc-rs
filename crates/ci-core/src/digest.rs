use core::fmt;

macro_rules! fixed_digest {
    ($name:ident) => {
        #[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name([u8; 32]);

        impl $name {
            pub const fn from_bytes(bytes: [u8; 32]) -> Self {
                Self(bytes)
            }

            pub const fn as_bytes(&self) -> &[u8; 32] {
                &self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_tuple(stringify!($name))
                    .field(&self.0)
                    .finish()
            }
        }
    };
}

fixed_digest!(ObjectDigestV1);
fixed_digest!(InputDigestV1);
fixed_digest!(ApplicationNamespaceDigestV1);
fixed_digest!(ActionKeyV1);
fixed_digest!(GraphDigestV1);
fixed_digest!(BuildArtifactIdV1);
fixed_digest!(OutcomeDigestV1);
fixed_digest!(AdapterRegistryDigestV1);
fixed_digest!(ConflictRegistryDigestV1);
fixed_digest!(AuthorityReceiptDigestV1);
fixed_digest!(PublicationEventDigestV1);
