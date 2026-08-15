use crate::{
    hash_adapter_registry, AdapterDescriptorSetV1, AdapterDescriptorV1, BoundedBytesSink,
    CanonicalEncode, CanonicalError, CanonicalSink,
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AdapterDecodeError {
    Malformed,
    NonCanonical,
    LimitExceeded,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RegistryError {
    Duplicate,
    Unexpected,
    Missing,
    DescriptorMismatch,
    CanonicalEncoding,
    Decode(AdapterDecodeError),
}

pub trait AdapterCodec: Send + Sync + 'static {
    type RawObservation: CanonicalEncode;

    fn descriptor() -> AdapterDescriptorV1;

    fn decode(bytes: &[u8]) -> Result<Self::RawObservation, AdapterDecodeError>;
}

type DecodeReencodeFn = fn(&[u8]) -> Result<Vec<u8>, AdapterDecodeError>;

#[derive(Clone, Copy)]
pub struct AdapterRegistration {
    descriptor: AdapterDescriptorV1,
    decode_reencode: DecodeReencodeFn,
}

impl AdapterRegistration {
    pub fn of<C: AdapterCodec>() -> Self {
        Self {
            descriptor: C::descriptor(),
            decode_reencode: decode_reencode::<C>,
        }
    }

    pub const fn descriptor(&self) -> AdapterDescriptorV1 {
        self.descriptor
    }
}

fn decode_reencode<C: AdapterCodec>(bytes: &[u8]) -> Result<Vec<u8>, AdapterDecodeError> {
    let value = C::decode(bytes)?;
    let mut sink = BoundedBytesSink::new(16 * 1024 * 1024);
    value
        .encode_canonical(&mut sink)
        .map_err(|error| match error {
            CanonicalError::LimitExceeded => AdapterDecodeError::LimitExceeded,
            _ => AdapterDecodeError::NonCanonical,
        })?;
    if sink.bytes() != bytes {
        return Err(AdapterDecodeError::NonCanonical);
    }
    Ok(sink.into_bytes())
}

pub struct AdapterRegistryBuilder {
    registrations: Vec<AdapterRegistration>,
}

impl AdapterRegistryBuilder {
    pub const fn new() -> Self {
        Self {
            registrations: Vec::new(),
        }
    }

    pub fn register(&mut self, registration: AdapterRegistration) -> Result<(), RegistryError> {
        if self
            .registrations
            .iter()
            .any(|entry| entry.descriptor == registration.descriptor)
        {
            return Err(RegistryError::Duplicate);
        }
        self.registrations.push(registration);
        Ok(())
    }

    pub fn seal(
        mut self,
        expected: &AdapterDescriptorSetV1,
    ) -> Result<VerifiedAdapterRegistry, RegistryError> {
        self.registrations
            .sort_by_key(|registration| registration.descriptor);
        if self.registrations.len() != expected.as_slice().len() {
            return if self.registrations.len() < expected.as_slice().len() {
                Err(RegistryError::Missing)
            } else {
                Err(RegistryError::Unexpected)
            };
        }
        if self
            .registrations
            .iter()
            .zip(expected.as_slice())
            .any(|(actual, expected)| actual.descriptor != *expected)
        {
            return Err(RegistryError::DescriptorMismatch);
        }
        let digest = registry_digest(expected)?;
        Ok(VerifiedAdapterRegistry {
            registrations: self.registrations.into_boxed_slice(),
            digest,
        })
    }
}

impl Default for AdapterRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub struct VerifiedAdapterRegistry {
    registrations: Box<[AdapterRegistration]>,
    digest: crate::AdapterRegistryDigestV1,
}

impl VerifiedAdapterRegistry {
    pub const fn digest(&self) -> crate::AdapterRegistryDigestV1 {
        self.digest
    }

    pub fn descriptors(&self) -> impl Iterator<Item = AdapterDescriptorV1> + '_ {
        self.registrations
            .iter()
            .map(|registration| registration.descriptor)
    }

    pub fn decode_reencode(
        &self,
        descriptor: AdapterDescriptorV1,
        bytes: &[u8],
    ) -> Result<Vec<u8>, RegistryError> {
        let registration = self
            .registrations
            .iter()
            .find(|registration| registration.descriptor == descriptor)
            .ok_or(RegistryError::Unexpected)?;
        (registration.decode_reencode)(bytes).map_err(RegistryError::Decode)
    }
}

fn registry_digest(
    descriptors: &AdapterDescriptorSetV1,
) -> Result<crate::AdapterRegistryDigestV1, RegistryError> {
    let mut sink = BoundedBytesSink::new(1024 * 1024);
    sink.write(b"[")
        .map_err(|_| RegistryError::CanonicalEncoding)?;
    for (index, descriptor) in descriptors.as_slice().iter().enumerate() {
        if index != 0 {
            sink.write(b",")
                .map_err(|_| RegistryError::CanonicalEncoding)?;
        }
        descriptor
            .encode_canonical(&mut sink)
            .map_err(|_| RegistryError::CanonicalEncoding)?;
    }
    sink.write(b"]")
        .map_err(|_| RegistryError::CanonicalEncoding)?;
    Ok(hash_adapter_registry(sink.bytes()))
}

impl CanonicalEncode for AdapterDescriptorV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"adapter\":")?;
        write_hex(out, self.adapter().as_bytes())?;
        out.write(b",\"implementation\":")?;
        write_hex(out, self.implementation().as_bytes())?;
        out.write(b",\"schema\":")?;
        write_hex(out, self.schema().as_bytes())?;
        out.write(b"}")
    }
}

fn write_hex<S: CanonicalSink>(out: &mut S, bytes: &[u8; 16]) -> Result<(), CanonicalError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.write(b"\"")?;
    for byte in bytes {
        out.write(&[HEX[(byte >> 4) as usize], HEX[(byte & 0xf) as usize]])?;
    }
    out.write(b"\"")
}
