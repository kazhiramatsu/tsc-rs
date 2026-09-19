//! Standalone probe compiled with the identical digest source against the
//! candidate, the restored pre-interface projection, and the main merge base.
#[path = "../crates/xtask/src/recovery_parse_snapshot.rs"]
mod snapshot;

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};

fn main() {
    let mut bytes = Vec::new();
    std::io::stdin().read_to_end(&mut bytes).unwrap();
    let artifact: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(artifact["schema"], 1);
    assert_eq!(artifact["kind"], "emitter-recovery-parse-snapshot");
    let digest_hash = snapshot::sha256(include_bytes!(
        "../crates/xtask/src/recovery_parse_snapshot.rs"
    ));
    assert_eq!(artifact["digest_code_sha256"], digest_hash);
    let mut digests = BTreeMap::new();
    for (id, input) in artifact["inputs"].as_object().unwrap() {
        assert_eq!(id, input["input_id"].as_str().unwrap());
        let source = snapshot::replay(input);
        assert!(digests
            .insert(id.clone(), json!({"core":snapshot::digest(&source),"profiles":profiles(&source)}))
            .is_none());
    }
    let result = json!({"schema":1,"kind":"emitter-recovery-parse-replay",
        "input_artifact_sha256":snapshot::sha256(bytes),"digest_code_sha256":digest_hash,
        "digests":digests});
    serde_json::to_writer(std::io::stdout().lock(), &result).unwrap();
    std::io::stdout().write_all(b"\n").unwrap();
}

#[cfg(feature = "current-recovery-profiles")]
fn profiles(source: &tsc_syntax::SourceFile) -> Value {
    json!({"literal":source.has_only_literal_recovery(),
        "missing_await":source.has_only_literal_or_missing_await_recovery(),
        "missing_declaration":source.has_only_missing_node_emit_recovery(),
        "parameter_gaps":source.has_only_parameter_gap_emit_recovery(),
        "statement_gaps":source.has_only_statement_gap_emit_recovery(),
        "context_recovery":source.has_supported_emit_recovery()})
}

#[cfg(not(feature = "current-recovery-profiles"))]
fn profiles(_source: &tsc_syntax::SourceFile) -> Value { Value::Null }
