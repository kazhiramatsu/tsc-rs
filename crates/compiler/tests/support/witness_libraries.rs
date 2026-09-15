//! Immutable library inputs only. Each caller still constructs a fresh host and Program.

use std::path::Path;
use std::sync::OnceLock;

pub(crate) fn files() -> &'static [(String, Vec<u8>)] {
    static FILES: OnceLock<Vec<(String, Vec<u8>)>> = OnceLock::new();
    FILES.get_or_init(|| {
        let directory =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/typescript-6.0.3/lib");
        let mut files = Vec::new();
        for entry in std::fs::read_dir(directory).expect("vendored TypeScript libraries") {
            let entry = entry.expect("library directory entry");
            let name = entry.file_name().into_string().expect("library filename");
            if name.starts_with("lib.") && name.ends_with(".d.ts") {
                files.push((
                    format!("/lib/{name}"),
                    std::fs::read(entry.path()).expect("library bytes"),
                ));
            }
        }
        files.sort_by(|left, right| left.0.cmp(&right.0));
        assert!(!files.is_empty(), "missing TypeScript library inputs");
        files
    })
}
