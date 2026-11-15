use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn inputs(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("read binder source directory") {
        let path = entry.expect("read binder source entry").path();
        if path.is_dir() {
            inputs(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

fn main() {
    let directory = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let mut files = vec![directory.join("build.rs")];
    inputs(&directory.join("src"), &mut files);
    files.sort();
    let mut hash = 0xcbf29ce484222325u64;
    println!("cargo:rerun-if-changed=src");
    for path in files {
        println!("cargo:rerun-if-changed={}", path.display());
        for byte in fs::read(path).expect("binder provenance input") {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    fs::write(
        output.join("library_image_schema.rs"),
        format!("{hash}u64\n"),
    )
    .expect("write binder provenance");
}
