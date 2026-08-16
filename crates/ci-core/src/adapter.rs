use core::fmt;

use crate::{ImplementationIdV1, SchemaIdV1};

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AdapterIdV1([u8; 16]);

impl AdapterIdV1 {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl fmt::Debug for AdapterIdV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("AdapterIdV1").field(&self.0).finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AdapterDescriptorV1 {
    adapter: AdapterIdV1,
    schema: SchemaIdV1,
    implementation: ImplementationIdV1,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AdapterDescriptorError {
    EmptyIdentity,
    Unsorted { index: usize },
}

impl AdapterDescriptorV1 {
    pub fn try_new(
        adapter: AdapterIdV1,
        schema: SchemaIdV1,
        implementation: ImplementationIdV1,
    ) -> Result<Self, AdapterDescriptorError> {
        if adapter.as_bytes().iter().all(|byte| *byte == 0)
            || schema.as_bytes().iter().all(|byte| *byte == 0)
            || implementation.as_bytes().iter().all(|byte| *byte == 0)
        {
            return Err(AdapterDescriptorError::EmptyIdentity);
        }
        Ok(Self {
            adapter,
            schema,
            implementation,
        })
    }

    pub const fn adapter(&self) -> AdapterIdV1 {
        self.adapter
    }

    pub const fn schema(&self) -> SchemaIdV1 {
        self.schema
    }

    pub const fn implementation(&self) -> ImplementationIdV1 {
        self.implementation
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AdapterDescriptorSetV1 {
    entries: Box<[AdapterDescriptorV1]>,
}

impl AdapterDescriptorSetV1 {
    pub fn try_from_sorted(
        entries: Vec<AdapterDescriptorV1>,
    ) -> Result<Self, AdapterDescriptorError> {
        if entries.windows(2).any(|pair| pair[0] >= pair[1]) {
            let index = entries
                .windows(2)
                .position(|pair| pair[0] >= pair[1])
                .map_or(0, |index| index + 1);
            return Err(AdapterDescriptorError::Unsorted { index });
        }
        Ok(Self {
            entries: entries.into_boxed_slice(),
        })
    }

    pub fn as_slice(&self) -> &[AdapterDescriptorV1] {
        &self.entries
    }
}
