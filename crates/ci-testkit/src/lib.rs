//! Development-only generic fixtures for the Functional-CI conformance suite.
//!
//! This package may depend on both framework crates, but no normal runtime
//! package may depend on it. It intentionally contains no repository nouns.

#![forbid(unsafe_code)]

use tsc_ci_core::{CanonicalValue, NodeClass, NodeRecord};

pub fn flat_fixture() -> Vec<NodeRecord<CanonicalValue, CanonicalValue, CanonicalValue>> {
    vec![NodeRecord::new(
        CanonicalValue::String("leaf".to_owned()),
        NodeClass::Input,
        CanonicalValue::String("flat".to_owned()),
        CanonicalValue::Null,
        Vec::new(),
    )]
}

pub fn composite_fixture() -> Vec<NodeRecord<CanonicalValue, CanonicalValue, CanonicalValue>> {
    vec![
        NodeRecord::new(
            CanonicalValue::String("input".to_owned()),
            NodeClass::Input,
            CanonicalValue::String("composite".to_owned()),
            CanonicalValue::Null,
            Vec::new(),
        ),
        NodeRecord::new(
            CanonicalValue::String("derived".to_owned()),
            NodeClass::Derived,
            CanonicalValue::String("composite".to_owned()),
            CanonicalValue::Bool(true),
            vec![CanonicalValue::String("input".to_owned())],
        ),
    ]
}
