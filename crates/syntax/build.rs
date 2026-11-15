use std::{env, fs, path::PathBuf};

fn main() {
    let directory = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let mut hash = 0xcbf29ce484222325u64;
    for relative in [
        "nodes.schema.json",
        "nodes.layout.json",
        "src/nodes.rs",
        "src/kind.rs",
        "src/library_image.rs",
        "src/library_image/runtime.rs",
        "build.rs",
    ] {
        println!("cargo:rerun-if-changed={relative}");
        for byte in fs::read(directory.join(relative)).expect("library image provenance input") {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    fs::write(
        output.join("library_image_schema.rs"),
        format!("{hash}u64\n"),
    )
    .expect("write library image provenance");
}
