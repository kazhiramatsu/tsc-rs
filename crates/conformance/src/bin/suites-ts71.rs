//! Compare the TypeScript 7.1 suites outside the compiler runner (transpile,
//! command-line and tsconfig parsing) with a vendored native profile's
//! reference baselines.
//!
//! usage: suites-ts71 [--profile <name>] [--filter <path substring>]
//!                    [--no-report] [--dump <directory>]
//!
//! Runs every case, writes target/suites-ts71/<profile>/report.json (unless
//! --no-report) and prints a summary; --dump writes every produced baseline
//! that differs from its reference under <directory>/<suite>/.
//! scripts/suites_ts71.py checks the report against the ratchet.

use std::path::PathBuf;

use tsc_conformance::ts71::suites::{report_path, run, summarize, SuiteRunOptions};

fn main() {
    let mut options = SuiteRunOptions {
        profile: "7.1.0-dev-aa814927".to_owned(),
        filter: None,
        dump: None,
    };
    let mut write_report = true;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| panic!("{arg} takes a value"));
        match arg.as_str() {
            "--profile" => options.profile = value(),
            "--filter" => options.filter = Some(value()),
            "--no-report" => write_report = false,
            "--dump" => options.dump = Some(PathBuf::from(value())),
            other => panic!("unknown argument {other}"),
        }
    }
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let started = std::time::Instant::now();
    let results = run(&workspace, &options).unwrap_or_else(|error| panic!("{error}"));
    let summary = summarize(&results);
    if write_report {
        let path = report_path(&workspace, &options.profile);
        std::fs::create_dir_all(path.parent().expect("report directory"))
            .expect("report directory");
        let report = serde_json::json!({
            "profile": options.profile,
            "filter": options.filter,
            "summary": summary,
            "results": results,
        });
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&report).expect("report json"),
        )
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        println!("report: {}", path.display());
    }
    for suite in &summary {
        println!(
            "{}: {} baselines, {} full, {} mismatch, {} harness error",
            suite.suite, suite.baselines, suite.full, suite.mismatch, suite.harness_error
        );
    }
    for result in &results {
        if result.outcome != tsc_conformance::ts71::suites::SuiteOutcome::Full {
            println!(
                "  {:?} {}/{}{}",
                result.outcome,
                result.suite,
                result.baseline,
                result
                    .detail
                    .as_deref()
                    .map(|detail| format!(": {detail}"))
                    .unwrap_or_default()
            );
        }
    }
    println!("{:.1} s", started.elapsed().as_secs_f64());
}
