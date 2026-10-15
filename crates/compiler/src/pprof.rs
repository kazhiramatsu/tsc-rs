//! tsgo `internal/pprof`: CPU profiles in Go's pprof format (a gzipped
//! `perftools.profiles.Profile`, sampled 100 times a second as Go's
//! `pprof.StartCPUProfile`), for `--pprofDir` and the language server's
//! profile commands. tsc-rs has no runtime that records its allocations, so
//! heap and allocation profiles are not supported; CPU profiles need Unix
//! signals.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Go's CPU profiling rate.
const FREQUENCY: i32 = 100;

/// tsgo `ProfileSession`: a CPU profile being taken, written when it stops.
pub struct ProfileSession {
    cpu_file_path: PathBuf,
    cpu_file: File,
    sampler: Sampler,
}

/// tsgo `BeginProfiling`: a CPU profile of the process to
/// `<dir>/<pid>-cpuprofile.pb.gz` (tsgo's memory profile is not supported).
/// A failure panics, as tsgo's.
pub fn begin_profiling(profile_dir: &str) -> ProfileSession {
    let begin = || -> io::Result<ProfileSession> {
        std::fs::create_dir_all(profile_dir)?;
        let path = Path::new(profile_dir).join(format!("{}-cpuprofile.pb.gz", std::process::id()));
        let file = File::create(&path)?;
        let sampler = Sampler::start()?;
        Ok(ProfileSession {
            cpu_file_path: path,
            cpu_file: file,
            sampler,
        })
    };
    begin().unwrap_or_else(|error| panic!("{error}"))
}

impl ProfileSession {
    /// tsgo `Stop`: the profile written (tsgo ignores the write's errors),
    /// its path reported on `log`.
    pub fn stop(self, log: &mut dyn Write) {
        let _ = self.finish();
        let _ = writeln!(log, "CPU profile: {}", self.cpu_file_path.display());
    }

    /// The profile written to its file.
    fn finish(&self) -> io::Result<()> {
        let profile = self.sampler.profile()?;
        (&self.cpu_file).write_all(&gzip(&profile))
    }
}

/// tsgo `CPUProfiler`: the language server's CPU profile, started and
/// stopped on request.
#[derive(Default)]
pub struct CpuProfiler {
    session: Mutex<Option<ProfileSession>>,
}

impl CpuProfiler {
    /// tsgo `StartCPUProfile`: a profile written to `profile_dir` when it
    /// stops.
    pub fn start(&self, profile_dir: &str) -> Result<(), String> {
        let mut session = self.session.lock().unwrap_or_else(PoisonError::into_inner);
        if session.is_some() {
            return Err("CPU profiling already in progress".to_owned());
        }
        std::fs::create_dir_all(profile_dir)
            .map_err(|error| format!("failed to create profile directory: {error}"))?;
        let path = Path::new(profile_dir).join(format!(
            "{}-{}-cpuprofile.pb.gz",
            std::process::id(),
            unix_millis()
        ));
        let file = File::create(&path)
            .map_err(|error| format!("failed to create CPU profile file: {error}"))?;
        let sampler = match Sampler::start() {
            Ok(sampler) => sampler,
            Err(error) => {
                drop(file);
                let _ = std::fs::remove_file(&path);
                return Err(format!("failed to start CPU profile: {error}"));
            }
        };
        *session = Some(ProfileSession {
            cpu_file_path: path,
            cpu_file: file,
            sampler,
        });
        Ok(())
    }

    /// tsgo `StopCPUProfile`: the profile's path.
    pub fn stop(&self) -> Result<String, String> {
        let session = self
            .session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .ok_or_else(|| "CPU profiling not in progress".to_owned())?;
        let path = session.cpu_file_path.to_string_lossy().into_owned();
        // tsgo writes the profile as it stops and ignores the write's errors.
        let _ = session.finish();
        Ok(path)
    }
}

/// tsgo `SaveHeapProfile`: not supported (tsc-rs's allocator records no
/// allocations).
pub fn save_heap_profile(_profile_dir: &str) -> Result<String, String> {
    Err("heap profiles are not supported by tsc-rs".to_owned())
}

/// tsgo `SaveAllocProfile`: not supported, as heap profiles.
pub fn save_alloc_profile(_profile_dir: &str) -> Result<String, String> {
    Err("allocation profiles are not supported by tsc-rs".to_owned())
}

/// What frees the memory the process no longer uses (the binary's
/// allocator), for [`run_gc`].
static GC: OnceLock<fn()> = OnceLock::new();

/// Sets what [`run_gc`] runs (once, by the binary).
pub fn set_gc(gc: fn()) {
    let _ = GC.set(gc);
}

/// tsgo `RunGC`: tsc-rs has no garbage collector; the allocator returns the
/// memory it no longer uses.
pub fn run_gc() {
    if let Some(gc) = GC.get() {
        gc();
    }
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis())
}

/// The sampling of the process's threads: pprof-rs's `SIGPROF` timer.
#[cfg(unix)]
struct Sampler(pprof::ProfilerGuard<'static>);

#[cfg(unix)]
impl Sampler {
    fn start() -> io::Result<Self> {
        pprof::ProfilerGuardBuilder::default()
            .frequency(FREQUENCY)
            .blocklist(&["libc", "libgcc", "pthread", "vdso"])
            .build()
            .map(Sampler)
            .map_err(io::Error::other)
    }

    /// The samples so far as an encoded profile.
    fn profile(&self) -> io::Result<Vec<u8>> {
        let report = self.0.report().build().map_err(io::Error::other)?;
        let samples = report.data.iter().map(|(frames, count)| {
            let stack = frames
                .frames
                .iter()
                .flatten()
                .map(|symbol| Frame {
                    name: symbol.name(),
                    system_name: symbol.sys_name().into_owned(),
                    file_name: symbol.filename().into_owned(),
                    line: i64::from(symbol.lineno()),
                })
                .collect::<Vec<_>>();
            (stack, *count as i64)
        });
        Ok(encode_profile(
            samples,
            report.timing.start_time,
            report.timing.duration,
        ))
    }
}

/// Without Unix signals no CPU profile is taken.
#[cfg(not(unix))]
struct Sampler;

#[cfg(not(unix))]
impl Sampler {
    fn start() -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "CPU profiles are not supported on this platform",
        ))
    }

    fn profile(&self) -> io::Result<Vec<u8>> {
        unreachable!("no sampler starts")
    }
}

/// A frame of a sampled stack, innermost first.
#[cfg_attr(not(unix), allow(dead_code))]
struct Frame {
    name: String,
    system_name: String,
    file_name: String,
    line: i64,
}

/// A `perftools.profiles.Profile` of CPU samples, as Go's CPU profiles:
/// `samples/count` and `cpu/nanoseconds` values, a `cpu/nanoseconds` period
/// of a sampling interval, a location and a function per symbol.
#[cfg_attr(not(unix), allow(dead_code))]
fn encode_profile(
    samples: impl Iterator<Item = (Vec<Frame>, i64)>,
    start: SystemTime,
    duration: Duration,
) -> Vec<u8> {
    let period = 1_000_000_000 / i64::from(FREQUENCY);
    let mut strings = StringTable::default();
    let samples_type = strings.index("samples");
    let count_unit = strings.index("count");
    let cpu_type = strings.index("cpu");
    let nanoseconds_unit = strings.index("nanoseconds");
    let mut functions: HashMap<String, u64> = HashMap::new();
    let mut function_messages = Vec::new();
    let mut location_messages = Vec::new();
    let mut sample_messages = Vec::new();
    for (stack, count) in samples {
        let mut location_ids = Vec::with_capacity(stack.len());
        for frame in stack {
            let id = match functions.get(&frame.name) {
                Some(id) => *id,
                None => {
                    let id = function_messages.len() as u64 + 1;
                    let mut function = Vec::new();
                    field_varint(&mut function, 1, id);
                    field_varint(&mut function, 2, strings.index(&frame.name));
                    field_varint(&mut function, 3, strings.index(&frame.system_name));
                    field_varint(&mut function, 4, strings.index(&frame.file_name));
                    function_messages.push(function);
                    let mut line = Vec::new();
                    field_varint(&mut line, 1, id);
                    field_varint(&mut line, 2, frame.line as u64);
                    let mut location = Vec::new();
                    field_varint(&mut location, 1, id);
                    field_bytes(&mut location, 4, &line);
                    location_messages.push(location);
                    functions.insert(frame.name, id);
                    id
                }
            };
            location_ids.push(id);
        }
        let mut sample = Vec::new();
        field_packed(&mut sample, 1, &location_ids);
        field_packed(&mut sample, 2, &[count as u64, (count * period) as u64]);
        sample_messages.push(sample);
    }
    let value_type = |kind: u64, unit: u64| {
        let mut message = Vec::new();
        field_varint(&mut message, 1, kind);
        field_varint(&mut message, 2, unit);
        message
    };
    let mut profile = Vec::new();
    field_bytes(&mut profile, 1, &value_type(samples_type, count_unit));
    field_bytes(&mut profile, 1, &value_type(cpu_type, nanoseconds_unit));
    for sample in &sample_messages {
        field_bytes(&mut profile, 2, sample);
    }
    for location in &location_messages {
        field_bytes(&mut profile, 4, location);
    }
    for function in &function_messages {
        field_bytes(&mut profile, 5, function);
    }
    for string in &strings.strings {
        field_bytes(&mut profile, 6, string.as_bytes());
    }
    let time_nanos = start
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos() as u64);
    field_varint(&mut profile, 9, time_nanos);
    field_varint(&mut profile, 10, duration.as_nanos() as u64);
    field_bytes(&mut profile, 11, &value_type(cpu_type, nanoseconds_unit));
    field_varint(&mut profile, 12, period as u64);
    profile
}

/// The profile's strings, the empty one first.
struct StringTable {
    strings: Vec<String>,
    indexes: HashMap<String, u64>,
}

impl Default for StringTable {
    fn default() -> Self {
        Self {
            strings: vec![String::new()],
            indexes: HashMap::from([(String::new(), 0)]),
        }
    }
}

impl StringTable {
    fn index(&mut self, string: &str) -> u64 {
        if let Some(index) = self.indexes.get(string) {
            return *index;
        }
        let index = self.strings.len() as u64;
        self.strings.push(string.to_owned());
        self.indexes.insert(string.to_owned(), index);
        index
    }
}

fn varint(buffer: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        buffer.push(value as u8 | 0x80);
        value >>= 7;
    }
    buffer.push(value as u8);
}

/// A varint field (wire type 0); a zero is left out, as proto3 does.
fn field_varint(buffer: &mut Vec<u8>, field: u64, value: u64) {
    if value != 0 {
        varint(buffer, field << 3);
        varint(buffer, value);
    }
}

/// A length-delimited field (wire type 2).
fn field_bytes(buffer: &mut Vec<u8>, field: u64, bytes: &[u8]) {
    varint(buffer, (field << 3) | 2);
    varint(buffer, bytes.len() as u64);
    buffer.extend_from_slice(bytes);
}

/// A packed repeated varint field.
fn field_packed(buffer: &mut Vec<u8>, field: u64, values: &[u64]) {
    let mut packed = Vec::new();
    for value in values {
        varint(&mut packed, *value);
    }
    field_bytes(buffer, field, &packed);
}

/// A gzip member (RFC 1952) of `data`, as Go's `gzip.Writer` writes one: no
/// name or time, an unknown operating system, the default level.
fn gzip(data: &[u8]) -> Vec<u8> {
    let mut member = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];
    member.extend(miniz_oxide::deflate::compress_to_vec(data, 6));
    member.extend(crc32(data).to_le_bytes());
    member.extend((data.len() as u32).to_le_bytes());
    member
}

/// The CRC-32 (IEEE) of `data`, as gzip checks it.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (crc & 1).wrapping_neg());
        }
    }
    !crc
}

/// The tests that take a profile, one at a time (a process has one
/// profiler, as Go's).
#[cfg(test)]
pub(crate) static TEST_PROFILER: Mutex<()> = Mutex::new(());

#[cfg(test)]
#[path = "../tests/unit/pprof.rs"]
mod tests;
