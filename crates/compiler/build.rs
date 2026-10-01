use std::env;
use std::fs;
use std::path::PathBuf;

/// TypeScript 7.1's embedded standard libraries (`tsc/internal/bundled/libs`
/// at the vendored native profile): 112 catalog files plus the compatibility
/// `lib.d.ts` entry.
const LIBRARY_DIRECTORY: &str =
    "../../vendor/typescript-native/7.1.0-dev-19dadef8/upstream/tsc/internal/bundled/libs";
const EXPECTED_LIBRARY_FILES: usize = 113;

fn main() {
    let manifest_directory = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo supplies CARGO_MANIFEST_DIR"),
    );
    let library_directory = manifest_directory.join(LIBRARY_DIRECTORY);
    println!("cargo:rerun-if-changed={}", library_directory.display());

    let mut libraries = fs::read_dir(&library_directory)
        .expect("read pinned TypeScript library directory")
        .map(|entry| entry.expect("read pinned TypeScript library entry").path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("lib.") && name.ends_with(".d.ts"))
        })
        .collect::<Vec<_>>();
    libraries.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    assert_eq!(
        libraries.len(),
        EXPECTED_LIBRARY_FILES,
        "pinned TypeScript library file count drifted"
    );

    let mut generated =
        String::from("pub(super) static EMBEDDED_LIBRARIES: &[(&str, &[u8])] = &[\n");
    for path in libraries {
        println!("cargo:rerun-if-changed={}", path.display());
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("pinned TypeScript library name is Unicode");
        generated.push_str(&format!(
            "    ({name:?}, include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{LIBRARY_DIRECTORY}/{name}\"))),\n"
        ));
    }
    generated.push_str("];\n");

    let output_directory = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo supplies OUT_DIR"));
    fs::write(output_directory.join("embedded_libraries.rs"), generated)
        .expect("write embedded TypeScript library table");
}
