use core::fmt;

use crate::{
    ActionKeyV1, ApplicationNamespaceV1, ImplementationIdV1, ObjectDigestV1,
    PublicationEventDigestV1, SchemaIdV1,
};

macro_rules! fixed_token {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name([u8; 16]);

        impl $name {
            pub const fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(bytes)
            }

            pub const fn as_bytes(&self) -> &[u8; 16] {
                &self.0
            }
        }
    };
}

fixed_token!(PlatformTokenV1);
fixed_token!(ToolIdV1);
fixed_token!(ToolRoleV1);
fixed_token!(InvocationIdV1);
fixed_token!(EvidenceAudienceV1);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum IdentityError {
    EmptyIdentity,
    Unsorted { index: usize },
    EmptyEnvironmentKey,
    SecretEnvironmentKey,
    InvalidLimit,
}

impl fmt::Display for IdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyIdentity => formatter.write_str("identity must not be all zero"),
            Self::Unsorted { index } => write!(formatter, "identity list is unsorted at {index}"),
            Self::EmptyEnvironmentKey => formatter.write_str("environment key must not be empty"),
            Self::SecretEnvironmentKey => {
                formatter.write_str("secret environment key is not reusable")
            }
            Self::InvalidLimit => formatter.write_str("identity limit is invalid"),
        }
    }
}

impl std::error::Error for IdentityError {}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExecutionPlatformV1 {
    os: PlatformTokenV1,
    architecture: PlatformTokenV1,
    target: PlatformTokenV1,
    runtime: PlatformTokenV1,
    filesystem: PlatformTokenV1,
    path_behavior: PlatformTokenV1,
    sandbox: PlatformTokenV1,
    kernel_capability: PlatformTokenV1,
    platform_independent: bool,
}

impl ExecutionPlatformV1 {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        os: PlatformTokenV1,
        architecture: PlatformTokenV1,
        target: PlatformTokenV1,
        runtime: PlatformTokenV1,
        filesystem: PlatformTokenV1,
        path_behavior: PlatformTokenV1,
        sandbox: PlatformTokenV1,
        kernel_capability: PlatformTokenV1,
        platform_independent: bool,
    ) -> Self {
        Self {
            os,
            architecture,
            target,
            runtime,
            filesystem,
            path_behavior,
            sandbox,
            kernel_capability,
            platform_independent,
        }
    }

    pub const fn os(&self) -> PlatformTokenV1 {
        self.os
    }

    pub const fn architecture(&self) -> PlatformTokenV1 {
        self.architecture
    }

    pub const fn target(&self) -> PlatformTokenV1 {
        self.target
    }

    pub const fn runtime(&self) -> PlatformTokenV1 {
        self.runtime
    }

    pub const fn filesystem(&self) -> PlatformTokenV1 {
        self.filesystem
    }

    pub const fn path_behavior(&self) -> PlatformTokenV1 {
        self.path_behavior
    }

    pub const fn sandbox(&self) -> PlatformTokenV1 {
        self.sandbox
    }

    pub const fn kernel_capability(&self) -> PlatformTokenV1 {
        self.kernel_capability
    }

    pub const fn platform_independent(&self) -> bool {
        self.platform_independent
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ToolRefV1 {
    id: ToolIdV1,
    role: ToolRoleV1,
    artifact: ObjectDigestV1,
    platform: ExecutionPlatformV1,
}

impl ToolRefV1 {
    pub const fn new(
        id: ToolIdV1,
        role: ToolRoleV1,
        artifact: ObjectDigestV1,
        platform: ExecutionPlatformV1,
    ) -> Self {
        Self {
            id,
            role,
            artifact,
            platform,
        }
    }

    pub const fn id(&self) -> ToolIdV1 {
        self.id
    }

    pub const fn role(&self) -> ToolRoleV1 {
        self.role
    }

    pub const fn artifact(&self) -> ObjectDigestV1 {
        self.artifact
    }

    pub const fn platform(&self) -> ExecutionPlatformV1 {
        self.platform
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ToolchainSetV1 {
    entries: Box<[ToolRefV1]>,
}

impl ToolchainSetV1 {
    pub fn try_from_sorted(entries: Vec<ToolRefV1>) -> Result<Self, IdentityError> {
        if entries.windows(2).any(|pair| pair[0] >= pair[1]) {
            let index = entries
                .windows(2)
                .position(|pair| pair[0] >= pair[1])
                .map_or(0, |index| index + 1);
            return Err(IdentityError::Unsorted { index });
        }
        Ok(Self {
            entries: entries.into_boxed_slice(),
        })
    }

    pub fn as_slice(&self) -> &[ToolRefV1] {
        &self.entries
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BuildComponentV1 {
    id: ToolIdV1,
    schema: SchemaIdV1,
    digest: ObjectDigestV1,
}

impl BuildComponentV1 {
    pub const fn new(id: ToolIdV1, schema: SchemaIdV1, digest: ObjectDigestV1) -> Self {
        Self { id, schema, digest }
    }

    pub const fn id(&self) -> ToolIdV1 {
        self.id
    }

    pub const fn schema(&self) -> SchemaIdV1 {
        self.schema
    }

    pub const fn digest(&self) -> ObjectDigestV1 {
        self.digest
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BuildComponentSetV1 {
    entries: Box<[BuildComponentV1]>,
}

impl BuildComponentSetV1 {
    pub fn try_from_sorted(entries: Vec<BuildComponentV1>) -> Result<Self, IdentityError> {
        if entries.windows(2).any(|pair| pair[0] >= pair[1]) {
            let index = entries
                .windows(2)
                .position(|pair| pair[0] >= pair[1])
                .map_or(0, |index| index + 1);
            return Err(IdentityError::Unsorted { index });
        }
        Ok(Self {
            entries: entries.into_boxed_slice(),
        })
    }

    pub fn as_slice(&self) -> &[BuildComponentV1] {
        &self.entries
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PublicEnvironmentEntryV1 {
    key: Box<[u8]>,
    value: Box<[u8]>,
}

impl PublicEnvironmentEntryV1 {
    pub fn try_new(key: Vec<u8>, value: Vec<u8>) -> Result<Self, IdentityError> {
        if key.is_empty() {
            return Err(IdentityError::EmptyEnvironmentKey);
        }
        Ok(Self {
            key: key.into_boxed_slice(),
            value: value.into_boxed_slice(),
        })
    }

    pub fn key(&self) -> &[u8] {
        &self.key
    }

    pub fn value(&self) -> &[u8] {
        &self.value
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SecretFreeEnvironmentV1 {
    entries: Box<[PublicEnvironmentEntryV1]>,
}

impl SecretFreeEnvironmentV1 {
    pub fn try_from_sorted(
        entries: Vec<PublicEnvironmentEntryV1>,
        forbidden_secret_keys: &[&[u8]],
    ) -> Result<Self, IdentityError> {
        for (index, entry) in entries.iter().enumerate() {
            if forbidden_secret_keys
                .iter()
                .any(|forbidden| *forbidden == entry.key())
            {
                return Err(IdentityError::SecretEnvironmentKey);
            }
            if index > 0 && entries[index - 1].key() >= entry.key() {
                return Err(IdentityError::Unsorted { index });
            }
        }
        Ok(Self {
            entries: entries.into_boxed_slice(),
        })
    }

    pub fn as_slice(&self) -> &[PublicEnvironmentEntryV1] {
        &self.entries
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ReuseScopeV1 {
    NonReusable,
    LocalReusable,
    SharedReusable { audience: EvidenceAudienceV1 },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DisclosureEntryV1 {
    audience: EvidenceAudienceV1,
    first_event: PublicationEventDigestV1,
}

impl DisclosureEntryV1 {
    pub const fn new(audience: EvidenceAudienceV1, first_event: PublicationEventDigestV1) -> Self {
        Self {
            audience,
            first_event,
        }
    }

    pub const fn audience(&self) -> EvidenceAudienceV1 {
        self.audience
    }

    pub const fn first_event(&self) -> PublicationEventDigestV1 {
        self.first_event
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DisclosureHistoryV1 {
    entries: Box<[DisclosureEntryV1]>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DisclosureError {
    Unsorted { index: usize },
    Shrunk { index: usize },
    ChangedFirstEvent { index: usize },
}

impl DisclosureHistoryV1 {
    pub fn try_from_sorted(entries: Vec<DisclosureEntryV1>) -> Result<Self, DisclosureError> {
        if entries
            .windows(2)
            .any(|pair| pair[0].audience >= pair[1].audience)
        {
            let index = entries
                .windows(2)
                .position(|pair| pair[0].audience >= pair[1].audience)
                .map_or(0, |index| index + 1);
            return Err(DisclosureError::Unsorted { index });
        }
        Ok(Self {
            entries: entries.into_boxed_slice(),
        })
    }

    pub fn as_slice(&self) -> &[DisclosureEntryV1] {
        &self.entries
    }

    pub fn merge_monotonic(prior: &Self, replacement: &Self) -> Result<Self, DisclosureError> {
        for (index, old) in prior.entries.iter().enumerate() {
            let Some(current) = replacement
                .entries
                .iter()
                .find(|entry| entry.audience == old.audience)
            else {
                return Err(DisclosureError::Shrunk { index });
            };
            if current.first_event != old.first_event {
                return Err(DisclosureError::ChangedFirstEvent { index });
            }
        }
        Self::try_from_sorted(replacement.entries.to_vec())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NetworkAccessV1 {
    Disabled,
    Allowlisted,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FilesystemAccessV1 {
    ReadOnly,
    ReadWrite,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SandboxCapabilitiesV1 {
    abi: SchemaIdV1,
    network: NetworkAccessV1,
    filesystem: FilesystemAccessV1,
    max_output_bytes: u64,
}

impl SandboxCapabilitiesV1 {
    pub const fn new(
        abi: SchemaIdV1,
        network: NetworkAccessV1,
        filesystem: FilesystemAccessV1,
        max_output_bytes: u64,
    ) -> Result<Self, IdentityError> {
        if max_output_bytes == 0 {
            return Err(IdentityError::InvalidLimit);
        }
        Ok(Self {
            abi,
            network,
            filesystem,
            max_output_bytes,
        })
    }

    pub const fn abi(&self) -> SchemaIdV1 {
        self.abi
    }

    pub const fn network(&self) -> NetworkAccessV1 {
        self.network
    }

    pub const fn filesystem(&self) -> FilesystemAccessV1 {
        self.filesystem
    }

    pub const fn max_output_bytes(&self) -> u64 {
        self.max_output_bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ProcessObservationStatusV1 {
    Exited { code: i32 },
    Signaled { signal: u8 },
    TimedOut,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProcessObservationV1 {
    status: ProcessObservationStatusV1,
    stdout: ObjectDigestV1,
    stderr: ObjectDigestV1,
    output_truncated: bool,
}

impl ProcessObservationV1 {
    pub const fn new(
        status: ProcessObservationStatusV1,
        stdout: ObjectDigestV1,
        stderr: ObjectDigestV1,
        output_truncated: bool,
    ) -> Self {
        Self {
            status,
            stdout,
            stderr,
            output_truncated,
        }
    }

    pub const fn status(&self) -> ProcessObservationStatusV1 {
        self.status
    }

    pub const fn stdout(&self) -> ObjectDigestV1 {
        self.stdout
    }

    pub const fn stderr(&self) -> ObjectDigestV1 {
        self.stderr
    }

    pub const fn output_truncated(&self) -> bool {
        self.output_truncated
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct InvocationIdentityV1 {
    id: InvocationIdV1,
    namespace: ApplicationNamespaceV1,
    adapter_schema: SchemaIdV1,
    implementation: ImplementationIdV1,
    action: ActionKeyV1,
    argv: Box<[Box<[u8]>]>,
    working_directory: Box<[u8]>,
    environment: SecretFreeEnvironmentV1,
    platform: ExecutionPlatformV1,
    toolchain: ToolchainSetV1,
    sandbox: SandboxCapabilitiesV1,
    invocation_builder: ImplementationIdV1,
    observation_encoder: ImplementationIdV1,
    failure_classifier: ImplementationIdV1,
}

impl InvocationIdentityV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: InvocationIdV1,
        namespace: ApplicationNamespaceV1,
        adapter_schema: SchemaIdV1,
        implementation: ImplementationIdV1,
        action: ActionKeyV1,
        argv: Vec<Vec<u8>>,
        working_directory: Vec<u8>,
        environment: SecretFreeEnvironmentV1,
        platform: ExecutionPlatformV1,
        toolchain: ToolchainSetV1,
        sandbox: SandboxCapabilitiesV1,
        invocation_builder: ImplementationIdV1,
        observation_encoder: ImplementationIdV1,
        failure_classifier: ImplementationIdV1,
    ) -> Self {
        Self {
            id,
            namespace,
            adapter_schema,
            implementation,
            action,
            argv: argv
                .into_iter()
                .map(Vec::into_boxed_slice)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            working_directory: working_directory.into_boxed_slice(),
            environment,
            platform,
            toolchain,
            sandbox,
            invocation_builder,
            observation_encoder,
            failure_classifier,
        }
    }

    pub const fn id(&self) -> InvocationIdV1 {
        self.id
    }

    pub const fn namespace(&self) -> ApplicationNamespaceV1 {
        self.namespace
    }

    pub const fn adapter_schema(&self) -> SchemaIdV1 {
        self.adapter_schema
    }

    pub const fn implementation(&self) -> ImplementationIdV1 {
        self.implementation
    }

    pub const fn action(&self) -> ActionKeyV1 {
        self.action
    }

    pub fn argv(&self) -> &[Box<[u8]>] {
        &self.argv
    }

    pub fn working_directory(&self) -> &[u8] {
        &self.working_directory
    }

    pub const fn environment(&self) -> &SecretFreeEnvironmentV1 {
        &self.environment
    }

    pub const fn platform(&self) -> ExecutionPlatformV1 {
        self.platform
    }

    pub const fn toolchain(&self) -> &ToolchainSetV1 {
        &self.toolchain
    }

    pub const fn sandbox(&self) -> SandboxCapabilitiesV1 {
        self.sandbox
    }

    pub const fn invocation_builder(&self) -> ImplementationIdV1 {
        self.invocation_builder
    }

    pub const fn observation_encoder(&self) -> ImplementationIdV1 {
        self.observation_encoder
    }

    pub const fn failure_classifier(&self) -> ImplementationIdV1 {
        self.failure_classifier
    }
}
