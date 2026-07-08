#![forbid(unsafe_code)]

fn main() {
    let command = std::env::args().nth(1);

    match command.as_deref() {
        None | Some("scaffold-smoke") => scaffold_smoke(),
        Some(other) => {
            eprintln!("unknown xtask command: {other}");
            std::process::exit(2);
        }
    }
}

fn scaffold_smoke() {
    let harness_diags = tsrs2_harness::check_empty_program().diagnostics.len();
    let conformance_diags = tsrs2_conformance::run_empty_engine_smoke();

    if harness_diags != 0 || conformance_diags != 0 {
        eprintln!("empty-engine scaffold emitted diagnostics");
        std::process::exit(1);
    }

    println!("tsrs2 scaffold ready");
}
