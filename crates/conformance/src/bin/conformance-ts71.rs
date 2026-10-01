//! Run the TypeScript 7.1 conformance lanes over a vendored native profile.
//!
//! usage: conformance-ts71 [--profile <name>] [--filter <path substring>]
//!                         [--case <suite>/<path>] [--threads <n>] [--checkers <n>]
//!                         [--no-report] [--dump <directory>]
//!        conformance-ts71 [--profile <name>] [--filter <path substring>] --list
//!        conformance-ts71 [--profile <name>] [--checkers <n>] --worker <cases file>
//!
//! By default runs the selected cases on threads of this process, writes
//! target/conformance-ts71/<profile>/report.json (unless --no-report) and
//! prints a summary; --dump writes tsc-rs's error baseline of every lane-A
//! configuration that differs from its reference. A case that overflows the
//! stack aborts the process, so the whole corpus runs through
//! scripts/conformance_ts71.py, which lists the cases with --list and runs
//! them in restartable --worker processes. Every configuration checks on one
//! checker unless --checkers asks for the sharded control.

use std::path::PathBuf;

use tsc_conformance::ts71::{list, report_path, run, run_worker, summarize, RunOptions};

fn main() {
    let mut options = RunOptions {
        profile: "7.1.0-dev-19dadef8".to_owned(),
        filter: None,
        case: None,
        threads: std::thread::available_parallelism().map_or(2, |n| n.get().min(8)),
        checkers: 1,
        dump: None,
    };
    let mut write_report = true;
    let mut list_only = false;
    let mut worker: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| panic!("{arg} takes a value"));
        match arg.as_str() {
            "--profile" => options.profile = value(),
            "--filter" => options.filter = Some(value()),
            "--case" => options.case = Some(value()),
            "--threads" => options.threads = value().parse().expect("--threads takes a number"),
            "--checkers" => {
                options.checkers = value().parse().expect("--checkers takes a number");
            }
            "--no-report" => write_report = false,
            "--list" => list_only = true,
            "--worker" => worker = Some(PathBuf::from(value())),
            "--dump" => options.dump = Some(PathBuf::from(value())),
            other => panic!("unknown argument {other}"),
        }
    }
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    if let Some(cases_file) = worker {
        run_worker(&workspace, &options.profile, &cases_file, options.checkers)
            .unwrap_or_else(|error| panic!("{error}"));
        return;
    }
    if list_only {
        for key in list(&workspace, &options).unwrap_or_else(|error| panic!("{error}")) {
            println!("{key}");
        }
        return;
    }
    let started = std::time::Instant::now();
    let results = run(&workspace, &options).unwrap_or_else(|error| panic!("{error}"));
    let summary = summarize(&results);
    let path = report_path(&workspace, &options.profile);
    if write_report {
        std::fs::create_dir_all(path.parent().expect("report directory"))
            .expect("report directory");
        let report = serde_json::json!({ "profile": options.profile, "summary": summary, "results": results });
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&report).expect("report json"),
        )
        .expect("write report");
    }
    println!(
        "{} configurations in {:.1}s: lane A {} (full {}, text {}, category {}, location {}, mismatch {}, emit {}/{}/{}, maps {}/{}, harness errors {}), deprecated {}, not run {}",
        summary.configurations,
        started.elapsed().as_secs_f64(),
        summary.lane_a,
        summary.full,
        summary.text,
        summary.category,
        summary.location,
        summary.mismatch,
        summary.emit_full,
        summary.emit_mismatch,
        summary.emit_not_assessed,
        summary.map_full,
        summary.map_mismatch,
        summary.harness_errors,
        summary.deprecated,
        summary.not_run,
    );
    if write_report {
        println!("report: {}", path.display());
    }
}
