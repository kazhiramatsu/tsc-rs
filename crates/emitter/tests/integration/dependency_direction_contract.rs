use std::fs;
use std::path::Path;

fn read(relative: impl AsRef<Path>) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(relative))
        .expect("read workspace contract input")
}

#[test]
fn emitter_dependency_direction_is_acyclic_and_host_stays_read_only() {
    let emitter = read("Cargo.toml");
    for dependency in [
        "tsc-diagnostics.workspace = true",
        "tsc-program.workspace = true",
        "tsc-syntax.workspace = true",
        "tsc-types.workspace = true",
    ] {
        assert!(emitter.contains(dependency), "missing {dependency}");
    }
    assert!(!emitter.contains("tsc-checker"));
    assert!(!emitter.contains("tsc-compiler"));

    let checker = read("../checker/Cargo.toml");
    assert!(checker.contains("tsc-emitter.workspace = true"));
    let compiler = read("../compiler/Cargo.toml");
    assert!(compiler.contains("tsc-checker.workspace = true"));
    assert!(compiler.contains("tsc-emitter.workspace = true"));
    assert!(compiler.contains("tsc-program.workspace = true"));
    assert!(compiler.contains("tsc-host.workspace = true"));

    let host = read("../host/src/lib.rs");
    let trait_start = host.find("pub trait CompilerHost {").expect("host trait");
    let trait_body = &host[trait_start..];
    assert!(!trait_body.contains("fn write_file"));
}
