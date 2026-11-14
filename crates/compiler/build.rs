use std::env;
use std::fs;
use std::path::PathBuf;

/// TypeScript 7.1's embedded standard libraries (`tsc/internal/bundled/libs`
/// at the vendored native profile): 115 catalog files plus the compatibility
/// `lib.d.ts` entry.
const LIBRARY_DIRECTORY: &str =
    "../../vendor/typescript-native/7.1.0-dev-aa814927/upstream/tsc/internal/bundled/libs";
const EXPECTED_LIBRARY_FILES: usize = 116;

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

    let mut generated = String::from(
        "pub(super) static EMBEDDED_LIBRARIES: &[(&str, tsc_diagnostics::SharedText)] = &[\n",
    );
    let mut text_allocations = String::new();
    for (index, path) in libraries.into_iter().enumerate() {
        println!("cargo:rerun-if-changed={}", path.display());
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("pinned TypeScript library name is Unicode");
        let text = fs::read_to_string(&path).expect("bundled declarations are valid UTF-8");
        assert!(
            !text.starts_with('\u{feff}'),
            "embedded declarations have no BOM"
        );
        generated.push_str(&format!("    ({name:?}, LIBRARY_TEXT_{index}.shared()),\n"));
        text_allocations.push_str(&format!(
            "static LIBRARY_TEXT_{index}: tsc_diagnostics::StaticText<{{ include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{LIBRARY_DIRECTORY}/{name}\")).len() }}> = tsc_diagnostics::StaticText::new(include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{LIBRARY_DIRECTORY}/{name}\")));\n",
        ));
    }
    generated.push_str("];\n");
    generated.push_str(&text_allocations);

    let output_directory = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo supplies OUT_DIR"));
    fs::write(output_directory.join("embedded_libraries.rs"), generated)
        .expect("write embedded TypeScript library table");
}
