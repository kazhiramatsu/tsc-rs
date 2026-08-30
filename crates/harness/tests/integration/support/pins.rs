//! gate-tax 8 S3: expected artifact hashes for the converted
//! integration tests, read from the committed harness pin manifest
//! (ratchets/pins/harness-expected.v1.json) instead of volatile .rs
//! literals. The manifest diff is the review surface; the walk
//! refreshes only its `values` section, so a re-mint no longer
//! changes harness source bytes. The `descriptors` section is the
//! reviewed structural authority: its canonical sha256 is FROZEN
//! below (the Rust half of the dual anchor beside the pin-index row)
//! and moves only with a reviewed descriptor-set change.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde_json::Value;
use sha2::{Digest, Sha256};

const MANIFEST: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/pins/harness-expected.v1.json"
));
const DESCRIPTOR_SHA256: &str = "360e906e1f2d6e4b21526583c3fcf47dfef11b12f50b298715c363c17820f8e5";

fn sha256(bytes: impl AsRef<[u8]>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes.as_ref());
    format!("{:x}", hasher.finalize())
}

fn field(row: &Value, key: &str) -> String {
    row[key]
        .as_str()
        .unwrap_or_else(|| panic!("manifest row field {key} is not a string"))
        .to_string()
}

fn table() -> &'static BTreeMap<(String, String), String> {
    static TABLE: OnceLock<BTreeMap<(String, String), String>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let manifest: Value = serde_json::from_slice(MANIFEST).expect("harness pin manifest");
        assert_eq!(manifest["schema"], 1, "harness pin manifest schema");
        let descriptors = manifest["descriptors"]
            .as_array()
            .expect("descriptors section");
        // Canonical serialization: sorted keys, compact separators —
        // byte-identical to the python side's
        // json.dumps(descriptors, sort_keys=True, separators=(",", ":")).
        let canonical_rows: Vec<BTreeMap<String, String>> = descriptors
            .iter()
            .map(|row| {
                row.as_object()
                    .expect("descriptor object")
                    .iter()
                    .map(|(key, value)| {
                        (
                            key.clone(),
                            value.as_str().expect("descriptor value string").to_string(),
                        )
                    })
                    .collect()
            })
            .collect();
        let canonical = serde_json::to_string(&canonical_rows).expect("canonical form");
        assert_eq!(
            sha256(canonical.as_bytes()),
            DESCRIPTOR_SHA256,
            "harness pin descriptor anchor: the descriptors section moved \
             without its reviewed same-slice anchor update"
        );
        let values = manifest["values"].as_array().expect("values section");
        assert_eq!(
            values.len(),
            descriptors.len(),
            "values/descriptors bijection (row count)"
        );
        let mut expected_hashes = BTreeMap::new();
        for row in values {
            let identity = (field(row, "test_file"), field(row, "check_id"));
            let previous = expected_hashes.insert(identity.clone(), field(row, "sha256"));
            assert!(previous.is_none(), "duplicate value identity {identity:?}");
        }
        for descriptor in descriptors {
            let identity = (
                field(descriptor, "test_file"),
                field(descriptor, "check_id"),
            );
            assert!(
                expected_hashes.contains_key(&identity),
                "value row missing for descriptor {identity:?}"
            );
        }
        expected_hashes
    })
}

/// The manifest's expected sha256 for one converted assert site,
/// keyed by (test module, check id) where check id is the pinned
/// path (the v1 identity rule).
pub fn expected(test_file: &str, check_id: &str) -> String {
    table()
        .get(&(test_file.to_string(), check_id.to_string()))
        .unwrap_or_else(|| panic!("no harness pin row for {test_file}:{check_id}"))
        .clone()
}
