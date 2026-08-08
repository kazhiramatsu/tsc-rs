//! H2.0b approved-runner observation driver.
//!
//! `cli` executes the production command driver under the allocation observer
//! and exposes only operational evidence in addition to the ordinary command
//! result. `fault` exercises the H1 multi-output callback boundary with one
//! deterministic failing output index. This example is never linked into the
//! production binary.

use std::alloc::System;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};
use stats_alloc::{Region, StatsAlloc, INSTRUMENTED_SYSTEM};
use tsc_compiler::{
    EmitFileSystem, FsOutputSink, H2ActivityCounters, H2RuntimeSlice, ProgramSession,
};
use tsc_program::{CompilerOptions, PathContext, PreparedProgram, PreparedSourceFile, ProgramPath};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    match arguments.next().as_deref() {
        Some("cli") => observe_cli(arguments.collect()),
        Some("fault") => {
            let index = arguments
                .next()
                .ok_or("fault mode requires an output index")?
                .parse::<usize>()?;
            if arguments.next().is_some() {
                return Err("fault mode received unexpected arguments".into());
            }
            observe_fault(index)
        }
        _ => Err("usage: h2_baseline_qualification <cli ARGS...|fault INDEX>".into()),
    }
}

fn observe_cli(arguments: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let region = Region::new(GLOBAL);
    let output = tsc_compiler::run_cli(&arguments);
    let stats = region.change();
    println!(
        "{}",
        json!({
            "schema": 1,
            "kind": "h2-cli-baseline-observation",
            "exit_code": output.exit_code(),
            "stdout": output.stdout(),
            "stderr": output.stderr(),
            "allocations": {
                "allocations": stats.allocations,
                "deallocations": stats.deallocations,
                "reallocations": stats.reallocations,
                "bytes_allocated": stats.bytes_allocated,
                "bytes_deallocated": stats.bytes_deallocated,
                "bytes_reallocated": stats.bytes_reallocated,
            },
            "work": {
                "parsed_documents": output.work_counters().parsed_documents(),
                "bound_documents": output.work_counters().bound_documents(),
                "full_text_copies": output.work_counters().full_text_copies(),
                "full_text_bytes_copied": output.work_counters().full_text_bytes_copied(),
            },
            "h1_no_emit": {
                "emit_resolver_constructions": output.no_emit_activity().emit_resolver_constructions(),
                "transformer_initializations": output.no_emit_activity().transformer_initializations(),
                "transform_context_constructions": output.no_emit_activity().transform_context_constructions(),
                "emit_side_table_allocations": output.no_emit_activity().emit_side_table_allocations(),
                "printer_writer_constructions": output.no_emit_activity().printer_writer_constructions(),
                "output_plan_constructions": output.no_emit_activity().output_plan_constructions(),
                "emit_artifact_creations": output.no_emit_activity().emit_artifact_creations(),
                "output_sink_writes": output.no_emit_activity().output_sink_writes(),
            },
            "h2_activity": activity_json(output.h2_activity()),
        })
    );
    Ok(())
}

struct InjectedFileSystem {
    fail_path: PathBuf,
    attempts: Vec<PathBuf>,
    files: BTreeMap<PathBuf, Vec<u8>>,
}

impl EmitFileSystem for InjectedFileSystem {
    fn write_file(&mut self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        self.attempts.push(path.to_path_buf());
        if path == self.fail_path {
            return Err("injected stable H2.0b write failure".to_owned());
        }
        self.files.insert(path.to_path_buf(), bytes.to_vec());
        Ok(())
    }

    fn create_directory(&mut self, path: &Path) -> Result<(), String> {
        Err(format!(
            "unexpected parent-directory construction for {}",
            path.display()
        ))
    }

    fn directory_exists(&mut self, path: &Path) -> bool {
        path == Path::new("/project")
    }
}

fn trusted_path(value: &str) -> ProgramPath {
    ProgramPath::from_trusted_parts(value, value).expect("static H2 baseline path")
}

fn fault_program() -> Result<PreparedProgram, Box<dyn std::error::Error>> {
    let mut builder = PreparedProgram::emitting_builder(
        PathContext::new(trusted_path("/project"), true),
        CompilerOptions {
            no_emit: Some(false),
            target: Some(99),
            module: Some(200),
            list_emitted_files: Some(true),
            ..CompilerOptions::default()
        },
    );
    for (name, text) in [
        ("/project/first.ts", "export const first: number = 1;\n"),
        ("/project/second.ts", "export const second: number = 2;\n"),
    ] {
        let source = builder.add_source_file(PreparedSourceFile::new(trusted_path(name), text))?;
        builder.add_root_file(source)?;
    }
    Ok(builder.build()?)
}

fn observe_fault(failed_index: usize) -> Result<(), Box<dyn std::error::Error>> {
    let output_paths = [
        PathBuf::from("/project/first.js"),
        PathBuf::from("/project/second.js"),
    ];
    let fail_path = output_paths
        .get(failed_index)
        .ok_or("fault index must be 0 or 1")?
        .clone();
    let mut filesystem = InjectedFileSystem {
        fail_path,
        attempts: Vec::new(),
        files: BTreeMap::new(),
    };
    let mut sink = FsOutputSink::new(&mut filesystem);
    let outcome = ProgramSession::new(fault_program()?).emit(&mut sink)?;
    let diagnostics = outcome
        .diagnostics()
        .iter()
        .map(|diagnostic| {
            json!({
                "code": diagnostic.code(),
                "category": diagnostic.category().name(),
                "message": diagnostic.message_text(),
            })
        })
        .collect::<Vec<_>>();
    let files = filesystem
        .files
        .iter()
        .map(|(path, bytes)| {
            json!({
                "path": path,
                "utf8": String::from_utf8_lossy(bytes),
                "bytes": bytes.len(),
            })
        })
        .collect::<Vec<_>>();
    println!(
        "{}",
        json!({
            "schema": 1,
            "kind": "h2-output-fault-observation",
            "failed_index": failed_index,
            "diagnostics": diagnostics,
            "emit_skipped": outcome.emit_skipped(),
            "emitted_files": outcome.emitted_files(),
            "source_maps_present": outcome.source_maps().is_some(),
            "filesystem_attempts": filesystem.attempts,
            "successful_files": files,
            "h2_activity": activity_json(outcome.h2_activity()),
        })
    );
    Ok(())
}

fn activity_json(counters: H2ActivityCounters) -> Value {
    let mut runtime = Map::new();
    for slice in H2RuntimeSlice::ALL {
        runtime.insert(
            slice.name().to_owned(),
            json!(counters.runtime_slice(slice)),
        );
    }
    json!({
        "positive": {
            "emit_session_constructions": counters.emit_session_constructions(),
            "output_plan_constructions": counters.output_plan_constructions(),
            "emit_resolver_borrows": counters.emit_resolver_borrows(),
            "script_transformer_list_constructions": counters.script_transformer_list_constructions(),
            "transform_typescript_constructions": counters.transform_typescript_constructions(),
            "transform_class_fields_constructions": counters.transform_class_fields_constructions(),
            "transform_ecmascript_module_constructions": counters.transform_ecmascript_module_constructions(),
            "transform_context_constructions": counters.transform_context_constructions(),
            "printer_constructions": counters.printer_constructions(),
            "javascript_artifact_creations": counters.javascript_artifact_creations(),
            "output_sink_write_attempts": counters.output_sink_write_attempts(),
            "output_sink_failures": counters.output_sink_failures(),
        },
        "runtime_slices": runtime,
    })
}
