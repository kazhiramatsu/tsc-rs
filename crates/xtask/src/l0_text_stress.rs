use std::collections::VecDeque;
use std::error::Error;
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde::Serialize;
use sha2::{Digest, Sha256};
use tsc_diagnostics::{
    ByteTextSpan, DocumentVersion, PositionIndex, PositionIndexKind, Utf16TextSpan,
    VersionedTextStore,
};

const DEFAULT_SEED: u64 = 0x5eed_1a01_cafe_f00d;
const DEFAULT_EDITS: usize = 20_000;
const DEFAULT_MAX_RSS_BYTES: u64 = 512 * 1024 * 1024;
const TRACE_LIMIT: usize = 256;

#[derive(Clone, Copy, Debug)]
struct Settings {
    seed: u64,
    edits: usize,
    max_rss_bytes: u64,
}

#[derive(Debug)]
struct Paths {
    fixture: PathBuf,
    report: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
struct SpanReport {
    start: u32,
    length: u32,
}

#[derive(Clone, Debug, Serialize)]
struct EditReport {
    ordinal: usize,
    coordinate: &'static str,
    byte_span: SpanReport,
    utf16_span: SpanReport,
    inserted_bytes: u32,
    inserted_utf16: u32,
    insertion_id: usize,
    document_version: String,
}

#[derive(Debug, Serialize)]
struct StressReport {
    schema: u32,
    kind: &'static str,
    status: &'static str,
    seed: String,
    requested_edits: usize,
    completed_edits: usize,
    snapshots_observed: usize,
    max_retained_snapshots: usize,
    fixture_path: String,
    initial_bytes: usize,
    initial_utf16: usize,
    initial_text_sha256: String,
    final_bytes: usize,
    final_utf16: usize,
    final_text_sha256: String,
    elapsed_millis: u128,
    peak_rss_bytes: Option<u64>,
    rss_ceiling_bytes: u64,
    trace_limit: usize,
    recent_edits: Vec<EditReport>,
    error: Option<String>,
}

#[derive(Debug)]
struct StressState {
    expected: String,
    completed_edits: usize,
    snapshots_observed: usize,
    max_retained_snapshots: usize,
    recent_edits: VecDeque<EditReport>,
}

pub(crate) fn run(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let (settings, paths) = parse_arguments(args)?;
    let initial = fs::read_to_string(&paths.fixture).map_err(|error| {
        format!(
            "cannot read L0 text-stress fixture {}: {error}",
            paths.fixture.display()
        )
    })?;
    if initial.len() < 1_000_000 {
        return Err("L0 text-stress fixture must contain at least 1,000,000 bytes".into());
    }
    let initial_hash = sha256(initial.as_bytes());
    let initial_bytes = initial.len();
    let initial_utf16 = initial.encode_utf16().count();
    let started = Instant::now();
    let execution = catch_unwind(AssertUnwindSafe(|| execute(settings, initial)));
    let (status, state, error) = match execution {
        Ok(Ok(state)) => ("passed", state, None),
        Ok(Err((state, error))) => ("failed", state, Some(error)),
        Err(payload) => {
            let detail = payload
                .downcast_ref::<&str>()
                .map(|value| (*value).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "text-stress panicked with a non-string payload".to_owned());
            (
                "failed",
                StressState {
                    expected: String::new(),
                    completed_edits: 0,
                    snapshots_observed: 0,
                    max_retained_snapshots: 0,
                    recent_edits: VecDeque::new(),
                },
                Some(detail),
            )
        }
    };
    let peak_rss_bytes = peak_rss_bytes();
    let rss_error = peak_rss_bytes
        .filter(|bytes| *bytes > settings.max_rss_bytes)
        .map(|bytes| {
            format!(
                "peak RSS {bytes} exceeds reviewed ceiling {}",
                settings.max_rss_bytes
            )
        });
    let error = error.or(rss_error);
    let status = if error.is_some() { "failed" } else { status };
    let final_text = &state.expected;
    let report = StressReport {
        schema: 1,
        kind: "l0-text-stress",
        status,
        seed: format!("0x{:016x}", settings.seed),
        requested_edits: settings.edits,
        completed_edits: state.completed_edits,
        snapshots_observed: state.snapshots_observed,
        max_retained_snapshots: state.max_retained_snapshots,
        fixture_path: paths.fixture.to_string_lossy().replace('\\', "/"),
        initial_bytes,
        initial_utf16,
        initial_text_sha256: initial_hash,
        final_bytes: final_text.len(),
        final_utf16: final_text.encode_utf16().count(),
        final_text_sha256: sha256(final_text.as_bytes()),
        elapsed_millis: started.elapsed().as_millis(),
        peak_rss_bytes,
        rss_ceiling_bytes: settings.max_rss_bytes,
        trace_limit: TRACE_LIMIT,
        recent_edits: state.recent_edits.into_iter().collect(),
        error,
    };
    write_report(&paths.report, &report)?;
    println!(
        "L0 text stress: status={} edits={}/{} snapshots={} peak-rss={:?}/{} report={}",
        report.status,
        report.completed_edits,
        report.requested_edits,
        report.snapshots_observed,
        report.peak_rss_bytes,
        report.rss_ceiling_bytes,
        paths.report.display()
    );
    if report.status != "passed" {
        return Err(format!(
            "L0 text stress failed: {}",
            report.error.as_deref().unwrap_or("unknown failure")
        )
        .into());
    }
    Ok(())
}

fn execute(settings: Settings, initial: String) -> Result<StressState, (StressState, String)> {
    let mut state = StressState {
        expected: initial.clone(),
        completed_edits: 0,
        snapshots_observed: 0,
        max_retained_snapshots: 1,
        recent_edits: VecDeque::new(),
    };
    let mut store = VersionedTextStore::new(initial, DocumentVersion::new("0"));
    let insertions = [
        "",
        "x",
        "😀",
        "日本",
        "\n",
        "\r",
        "\r\n",
        "\u{2028}",
        "\u{2029}",
        "\u{0085}",
        "z😀\r\n終",
    ];
    let mut random = XorShift64(settings.seed);

    for ordinal in 0..settings.edits {
        let start = boundary_at_or_before(
            &state.expected,
            random.index(state.expected.len().saturating_add(1)),
        );
        let tentative_end = start
            .saturating_add(random.index(33))
            .min(state.expected.len());
        let end = boundary_at_or_after(&state.expected, tentative_end);
        let insertion_id = random.index(insertions.len());
        let inserted = insertions[insertion_id];
        let document_version = if ordinal % 3 == 0 {
            "same".to_owned()
        } else {
            ordinal.to_string()
        };
        let byte_span = ByteTextSpan::new(start as u32, (end - start) as u32);
        let coordinate = if ordinal % 16 == 0 { "utf16" } else { "byte" };
        let outcome = if coordinate == "utf16" {
            let old = store.snapshot();
            let utf16_start = old
                .positions()
                .byte_to_utf16(byte_span.start)
                .expect("selected edit start is a scalar boundary");
            let utf16_end = old
                .positions()
                .byte_to_utf16(byte_span.start + byte_span.length)
                .expect("selected edit end is a scalar boundary");
            store.edit_utf16(
                Utf16TextSpan::new(utf16_start, utf16_end - utf16_start),
                inserted,
                DocumentVersion::new(document_version.clone()),
            )
        } else {
            store.edit_bytes(
                byte_span,
                inserted,
                DocumentVersion::new(document_version.clone()),
            )
        };
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                return Err((
                    state,
                    format!("edit {ordinal} was rejected unexpectedly: {error}"),
                ));
            }
        };
        state.expected.replace_range(start..end, inserted);
        state.completed_edits = ordinal + 1;
        state.recent_edits.push_back(EditReport {
            ordinal,
            coordinate,
            byte_span: SpanReport {
                start: outcome.byte_change().span.start,
                length: outcome.byte_change().span.length,
            },
            utf16_span: SpanReport {
                start: outcome.utf16_change().span.start,
                length: outcome.utf16_change().span.length,
            },
            inserted_bytes: outcome.byte_change().new_length,
            inserted_utf16: outcome.utf16_change().new_length,
            insertion_id,
            document_version,
        });
        if state.recent_edits.len() > TRACE_LIMIT {
            state.recent_edits.pop_front();
        }

        if outcome.published_snapshot().is_some() || ordinal % 64 == 0 {
            let snapshot = store.snapshot();
            if snapshot.text() != state.expected {
                return Err((
                    state,
                    format!("snapshot text diverged after edit {ordinal}"),
                ));
            }
            if snapshot.positions().kind() != PositionIndexKind::PersistentLines {
                return Err((
                    state,
                    format!("edited snapshot used a dense index after edit {ordinal}"),
                ));
            }
            let dense = PositionIndex::new_static(snapshot.text());
            if dense.byte_len() != snapshot.positions().byte_len()
                || dense.utf16_len() != snapshot.positions().utf16_len()
                || dense.line_count() != snapshot.positions().line_count()
            {
                return Err((
                    state,
                    format!("position-index totals diverged after edit {ordinal}"),
                ));
            }
            for sample in 0..32usize {
                let candidate = ordinal.wrapping_mul(131).wrapping_add(sample * 977)
                    % snapshot.text().len().saturating_add(1);
                let byte = boundary_at_or_before(snapshot.text(), candidate) as u32;
                if dense.byte_to_utf16(byte) != snapshot.positions().byte_to_utf16(byte) {
                    return Err((
                        state,
                        format!("position conversion diverged after edit {ordinal} at byte {byte}"),
                    ));
                }
            }
            state.snapshots_observed += 1;
            state.max_retained_snapshots = state
                .max_retained_snapshots
                .max(store.retained_snapshot_count());
            if store.retained_snapshot_count() > 8 {
                return Err((state, "snapshot history exceeded eight entries".to_owned()));
            }
        }
    }

    let final_snapshot = store.snapshot();
    if final_snapshot.text() != state.expected {
        return Err((state, "final snapshot text diverged".to_owned()));
    }
    state.snapshots_observed += 1;
    state.max_retained_snapshots = state
        .max_retained_snapshots
        .max(store.retained_snapshot_count());
    Ok(state)
}

fn parse_arguments(
    mut args: impl Iterator<Item = String>,
) -> Result<(Settings, Paths), Box<dyn Error>> {
    let mut seed = DEFAULT_SEED;
    let mut edits = DEFAULT_EDITS;
    let mut max_rss_bytes = DEFAULT_MAX_RSS_BYTES;
    let mut fixture = None;
    let mut report = PathBuf::from("target/l0/text-stress.json");
    while let Some(argument) = args.next() {
        let value = |name: &str, args: &mut dyn Iterator<Item = String>| {
            args.next()
                .ok_or_else(|| format!("{name} requires a value"))
        };
        match argument.as_str() {
            "--seed" => {
                let raw = value("--seed", &mut args)?;
                seed = raw
                    .strip_prefix("0x")
                    .map(|hex| u64::from_str_radix(hex, 16))
                    .unwrap_or_else(|| raw.parse())?;
            }
            "--edits" => edits = value("--edits", &mut args)?.parse()?,
            "--max-rss-bytes" => max_rss_bytes = value("--max-rss-bytes", &mut args)?.parse()?,
            "--fixture" => fixture = Some(PathBuf::from(value("--fixture", &mut args)?)),
            "--report" => report = PathBuf::from(value("--report", &mut args)?),
            other => return Err(format!("unknown l0 text-stress argument {other:?}").into()),
        }
    }
    let fixture = fixture.ok_or("l0 text-stress requires --fixture <large-edit.ts>")?;
    if edits == 0 || edits > 1_000_000 {
        return Err("--edits must be between 1 and 1,000,000".into());
    }
    if max_rss_bytes < 64 * 1024 * 1024 {
        return Err("--max-rss-bytes must be at least 64 MiB".into());
    }
    Ok((
        Settings {
            seed,
            edits,
            max_rss_bytes,
        },
        Paths { fixture, report },
    ))
}

fn write_report(path: &Path, report: &StressReport) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut bytes = serde_json::to_vec_pretty(report)?;
    bytes.push(b'\n');
    fs::write(path, bytes)?;
    Ok(())
}

fn boundary_at_or_before(text: &str, mut position: usize) -> usize {
    position = position.min(text.len());
    while !text.is_char_boundary(position) {
        position -= 1;
    }
    position
}

fn boundary_at_or_after(text: &str, mut position: usize) -> usize {
    position = position.min(text.len());
    while !text.is_char_boundary(position) {
        position += 1;
    }
    position
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn peak_rss_bytes() -> Option<u64> {
    if cfg!(target_os = "linux") {
        let status = fs::read_to_string("/proc/self/status").ok()?;
        let kilobytes = status
            .lines()
            .find_map(|line| line.strip_prefix("VmHWM:"))?
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()?;
        return kilobytes.checked_mul(1024);
    }
    let process_selector = ['-', 'p'].into_iter().collect::<String>();
    let output = Command::new("ps")
        .args(["-o", "rss="])
        .arg(process_selector)
        .arg(std::process::id().to_string())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()?
        .checked_mul(1024)
}

struct XorShift64(u64);

impl XorShift64 {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn index(&mut self, upper: usize) -> usize {
        debug_assert!(upper > 0);
        self.next() as usize % upper
    }
}
