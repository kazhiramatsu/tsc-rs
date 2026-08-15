use core::marker::PhantomData;

use crate::{AdapterIdV1, SchemaIdV1};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NodeClass {
    Input,
    Executable,
    Derived,
    Aggregate,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NodeRecord<I, K, S> {
    id: I,
    class: NodeClass,
    kind: K,
    spec: S,
    dependencies: Box<[I]>,
}

impl<I, K, S> NodeRecord<I, K, S> {
    pub fn new(id: I, class: NodeClass, kind: K, spec: S, dependencies: Vec<I>) -> Self {
        Self {
            id,
            class,
            kind,
            spec,
            dependencies: dependencies.into_boxed_slice(),
        }
    }

    pub fn id(&self) -> &I {
        &self.id
    }

    pub const fn class(&self) -> NodeClass {
        self.class
    }

    pub fn kind(&self) -> &K {
        &self.kind
    }

    pub fn spec(&self) -> &S {
        &self.spec
    }

    pub fn dependencies(&self) -> &[I] {
        &self.dependencies
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionRecord<I, A> {
    id: I,
    spec: A,
    dependencies: Box<[I]>,
}

impl<I, A> ActionRecord<I, A> {
    pub fn new(id: I, spec: A, dependencies: Vec<I>) -> Self {
        Self {
            id,
            spec,
            dependencies: dependencies.into_boxed_slice(),
        }
    }

    pub fn id(&self) -> &I {
        &self.id
    }

    pub fn spec(&self) -> &A {
        &self.spec
    }

    pub fn dependencies(&self) -> &[I] {
        &self.dependencies
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootRecord<I, R> {
    spec: R,
    members: Box<[I]>,
}

impl<I, R> RootRecord<I, R> {
    pub fn new(spec: R, members: Vec<I>) -> Self {
        Self {
            spec,
            members: members.into_boxed_slice(),
        }
    }

    pub fn spec(&self) -> &R {
        &self.spec
    }

    pub fn members(&self) -> &[I] {
        &self.members
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct InstanceIdV1([u8; 16]);

impl InstanceIdV1 {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AdapterInstanceRefV1 {
    instance: InstanceIdV1,
    adapter: AdapterIdV1,
    schema: SchemaIdV1,
}

impl AdapterInstanceRefV1 {
    pub const fn new(instance: InstanceIdV1, adapter: AdapterIdV1, schema: SchemaIdV1) -> Self {
        Self {
            instance,
            adapter,
            schema,
        }
    }

    pub const fn instance(&self) -> InstanceIdV1 {
        self.instance
    }

    pub const fn adapter(&self) -> AdapterIdV1 {
        self.adapter
    }

    pub const fn schema(&self) -> SchemaIdV1 {
        self.schema
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositeProfileV1 {
    instances: Box<[AdapterInstanceRefV1]>,
}

impl CompositeProfileV1 {
    pub fn new(instances: Vec<AdapterInstanceRefV1>) -> Self {
        Self {
            instances: instances.into_boxed_slice(),
        }
    }

    pub fn instances(&self) -> &[AdapterInstanceRefV1] {
        &self.instances
    }

    pub fn try_from_sorted(
        instances: Vec<AdapterInstanceRefV1>,
    ) -> Result<Self, crate::graph_schema::GraphSchemaError> {
        if instances.windows(2).any(|pair| pair[0] >= pair[1]) {
            let index = instances
                .windows(2)
                .position(|pair| pair[0] >= pair[1])
                .map_or(0, |index| index + 1);
            return Err(crate::graph_schema::GraphSchemaError::Unsorted { index });
        }
        Ok(Self::new(instances))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingMembership<I, V> {
    expected: Box<[I]>,
    marker: PhantomData<fn() -> V>,
}

impl<I, V> PendingMembership<I, V> {
    pub fn new(expected: Vec<I>) -> Self {
        Self {
            expected: expected.into_boxed_slice(),
            marker: PhantomData,
        }
    }

    pub fn expected(&self) -> &[I] {
        &self.expected
    }
}

/// A complete membership value is declared here so later APIs can name it,
/// but its sealed construction belongs to FCI-4a.3.
#[derive(Debug)]
pub struct CompleteMembership<I, V> {
    _sealed: SealedMembership,
    marker: PhantomData<fn() -> (I, V)>,
}

#[derive(Debug)]
struct SealedMembership;
