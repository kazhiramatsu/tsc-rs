use std::env;
use std::fs;
use std::path::PathBuf;

// The 107 catalog files plus TypeScript's compatibility `lib.d.ts` entry.
const EXPECTED_LIBRARY_FILES: usize = 108;

fn main() {
    let manifest_directory = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo supplies CARGO_MANIFEST_DIR"),
    );
    let library_directory = manifest_directory.join("../../vendor/typescript-6.0.3/lib");
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
        String::from("pub(super) static TYPESCRIPT_6_0_3_LIBRARIES: &[(&str, &[u8])] = &[\n");
    for path in libraries {
        println!("cargo:rerun-if-changed={}", path.display());
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("pinned TypeScript library name is Unicode");
        generated.push_str(&format!(
            "    ({name:?}, include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../vendor/typescript-6.0.3/lib/{name}\"))),\n"
        ));
    }
    generated.push_str("];\n");

    let output_directory = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo supplies OUT_DIR"));
    fs::write(
        output_directory.join("typescript_6_0_3_libraries.rs"),
        generated,
    )
    .expect("write embedded TypeScript library table");
}
