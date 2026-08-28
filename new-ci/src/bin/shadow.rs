use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

use new_ci::shadow::run_sample;

fn main() {
    if let Err(error) = run() {
        eprintln!("shadow adapter failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let repository = repository_root()?;
    // Cargo's manifest directory keeps generated evidence inside new-ci/ even
    // when this binary is launched with an outer working directory.
    let output_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/new-ci-shadow");
    let written = run_sample(repository, output_root)?;
    println!("{}", written.summary);
    Ok(())
}

fn repository_root() -> Result<PathBuf, Box<dyn Error>> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git rev-parse failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    let root = String::from_utf8(output.stdout)?.trim().to_string();
    if root.is_empty() {
        return Err("git returned an empty repository root".into());
    }
    Ok(PathBuf::from(root))
}
