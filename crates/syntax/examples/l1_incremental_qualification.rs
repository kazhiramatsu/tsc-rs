//! Allocation-observed large-file parser operation used only by L1
//! qualification. The frozen fixture and edit are bound by
//! `ratchets/l0-fixtures.v1.json`; setup constructs the old immutable source
//! before the measured region so both modes model an already-open document.

use std::alloc::System;
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use serde_json::json;
use stats_alloc::{Region, StatsAlloc, INSTRUMENTED_SYSTEM};
use tsc_diagnostics::{ByteTextChangeRange, ByteTextSpan, DocumentVersion, TextSnapshot};
use tsc_syntax::{
    create_language_service_source_file_in_identity_domain,
    update_language_service_source_file_in_identity_domain, IncrementalParseOptions,
    IncrementalParseStats, ParseOptions,
};
use tsc_types::IdentityDomain;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const FIXTURE_BYTES: usize = 1_073_676;
const EDIT_START: usize = 498_911;
const EDIT_DELETE_BYTES: usize = 17;
const EDIT_OLD_TEXT: &str = "label: \"row-6137\"";
const EDIT_NEW_TEXT: &str = "label: \"編集-6137-😀\"";
const AFTER_BYTES: usize = 1_073_684;

#[derive(Clone, Copy, Debug)]
enum Mode {
    Fresh,
    Incremental,
}

impl Mode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "fresh" => Ok(Self::Fresh),
            "incremental" => Ok(Self::Incremental),
            _ => Err(format!("unknown L1 qualification mode {value:?}")),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Incremental => "incremental",
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mode = Mode::parse(
        &args
            .next()
            .ok_or("usage: l1_incremental_qualification <fresh|incremental> <fixture>")?,
    )?;
    let fixture = args
        .next()
        .ok_or("usage: l1_incremental_qualification <fresh|incremental> <fixture>")?;
    if args.next().is_some() {
        return Err("L1 qualification received unexpected arguments".into());
    }

    let old_text = std::fs::read_to_string(fixture)?;
    if old_text.len() != FIXTURE_BYTES
        || old_text.get(EDIT_START..EDIT_START + EDIT_DELETE_BYTES) != Some(EDIT_OLD_TEXT)
    {
        return Err("large-edit fixture does not match its frozen byte/edit contract".into());
    }
    let mut new_text = old_text.clone();
    new_text.replace_range(EDIT_START..EDIT_START + EDIT_DELETE_BYTES, EDIT_NEW_TEXT);
    if new_text.len() != AFTER_BYTES {
        return Err("large-edit result does not match its frozen byte contract".into());
    }

    let old_snapshot = TextSnapshot::new(old_text, DocumentVersion::new("old"));
    let new_snapshot = TextSnapshot::new(new_text, DocumentVersion::new("new"));
    let domain = IdentityDomain::reclaiming();
    let options = ParseOptions::default();
    let old_source = create_language_service_source_file_in_identity_domain(
        "large-edit.ts",
        old_snapshot,
        options.clone(),
        &domain,
    )?;

    let region = Region::new(GLOBAL);
    let started = Instant::now();
    let (new_source, parse_stats) = match mode {
        Mode::Fresh => {
            let source = create_language_service_source_file_in_identity_domain(
                "large-edit.ts",
                Arc::clone(&new_snapshot),
                options,
                &domain,
            )?;
            let mut stats = IncrementalParseStats::default();
            stats.freshly_parsed_nodes = source.node_count();
            (source, stats)
        }
        Mode::Incremental => {
            let result = update_language_service_source_file_in_identity_domain(
                Arc::clone(&old_source),
                Arc::clone(&new_snapshot),
                ByteTextChangeRange {
                    span: ByteTextSpan::new(EDIT_START as u32, EDIT_DELETE_BYTES as u32),
                    new_length: EDIT_NEW_TEXT.len() as u32,
                },
                options,
                IncrementalParseOptions::default(),
                &domain,
            )?;
            (result.source, result.stats)
        }
    };
    black_box(&new_source);
    let elapsed = started.elapsed();
    let allocation = region.change();

    if new_source.text() != new_snapshot.text()
        || new_source.snapshot().document_version().as_str() != "new"
    {
        return Err("L1 qualification produced the wrong snapshot".into());
    }
    let observation = json!({
        "schema": 1,
        "kind": "l1-incremental-parser-operation",
        "mode": mode.name(),
        "operation_nanoseconds": elapsed.as_nanos(),
        "allocations": allocation.allocations,
        "deallocations": allocation.deallocations,
        "reallocations": allocation.reallocations,
        "bytes_allocated": allocation.bytes_allocated,
        "bytes_deallocated": allocation.bytes_deallocated,
        "bytes_reallocated": allocation.bytes_reallocated,
        "source": {
            "bytes": new_source.text().len(),
            "nodes": new_source.node_count(),
            "node_arrays": new_source.arena.node_arrays().len(),
        },
        "reuse": {
            "incremental": parse_stats.incremental,
            "full_parse_fallback": parse_stats.full_parse_fallback,
            "list_elements": parse_stats.reused_list_elements,
            "nodes": parse_stats.reused_nodes,
            "node_arrays": parse_stats.reused_node_arrays,
            "freshly_parsed_nodes": parse_stats.freshly_parsed_nodes,
        },
    });
    println!("{observation}");
    Ok(())
}
