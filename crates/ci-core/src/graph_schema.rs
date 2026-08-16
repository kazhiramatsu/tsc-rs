use crate::{
    AdapterInstanceRefV1, CanonicalEncode, CanonicalError, CanonicalSink, CompositeProfileV1,
    NodeClass, NodeRecord,
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GraphSchemaError {
    Unsorted { index: usize },
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ActionGraph<I, K, S> {
    nodes: Box<[NodeRecord<I, K, S>]>,
}

impl<I: Ord, K, S> ActionGraph<I, K, S> {
    pub fn try_from_sorted(nodes: Vec<NodeRecord<I, K, S>>) -> Result<Self, GraphSchemaError> {
        if nodes.windows(2).any(|pair| pair[0].id() >= pair[1].id()) {
            let index = nodes
                .windows(2)
                .position(|pair| pair[0].id() >= pair[1].id())
                .map_or(0, |index| index + 1);
            return Err(GraphSchemaError::Unsorted { index });
        }
        Ok(Self {
            nodes: nodes.into_boxed_slice(),
        })
    }

    pub fn as_slice(&self) -> &[NodeRecord<I, K, S>] {
        &self.nodes
    }
}

impl<I, K, S> CanonicalEncode for ActionGraph<I, K, S>
where
    I: CanonicalEncode,
    K: CanonicalEncode,
    S: CanonicalEncode,
{
    fn encode_canonical<T: CanonicalSink>(&self, out: &mut T) -> Result<(), CanonicalError> {
        out.write(br#"{"nodes":["#)?;
        for (index, node) in self.nodes.iter().enumerate() {
            if index != 0 {
                out.write(b",")?;
            }
            out.write(br#"{"class":"#)?;
            encode_class(node.class(), out)?;
            out.write(b",\"dependencies\":[")?;
            for (dependency_index, dependency) in node.dependencies().iter().enumerate() {
                if dependency_index != 0 {
                    out.write(b",")?;
                }
                dependency.encode_canonical(out)?;
            }
            out.write(b"],\"id\":")?;
            node.id().encode_canonical(out)?;
            out.write(b",\"kind\":")?;
            node.kind().encode_canonical(out)?;
            out.write(b",\"spec\":")?;
            node.spec().encode_canonical(out)?;
            out.write(b"}")?;
        }
        out.write(b"]}")
    }
}

fn encode_class<S: CanonicalSink>(class: NodeClass, out: &mut S) -> Result<(), CanonicalError> {
    let name = match class {
        NodeClass::Input => "input",
        NodeClass::Executable => "executable",
        NodeClass::Derived => "derived",
        NodeClass::Aggregate => "aggregate",
    };
    let value = crate::CanonicalValue::String(name.to_owned());
    value.encode_canonical(out)
}

impl CanonicalEncode for AdapterInstanceRefV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"adapter\":")?;
        write_hex_string(out, self.adapter().as_bytes())?;
        out.write(b",\"instance\":")?;
        write_hex_string(out, self.instance().as_bytes())?;
        out.write(b",\"schema\":")?;
        write_hex_string(out, self.schema().as_bytes())?;
        out.write(b"}")
    }
}

impl CanonicalEncode for CompositeProfileV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        if self.instances().windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(CanonicalError::InvalidKeyOrder);
        }
        out.write(br#"{"instances":["#)?;
        for (index, instance) in self.instances().iter().enumerate() {
            if index != 0 {
                out.write(b",")?;
            }
            instance.encode_canonical(out)?;
        }
        out.write(b"]}")
    }
}

fn write_hex_string<S: CanonicalSink>(out: &mut S, bytes: &[u8; 16]) -> Result<(), CanonicalError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.write(b"\"")?;
    for byte in bytes {
        out.write(&[HEX[(byte >> 4) as usize], HEX[(byte & 0xf) as usize]])?;
    }
    out.write(b"\"")
}
