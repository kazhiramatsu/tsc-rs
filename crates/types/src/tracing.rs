//! `--generateTrace` (tsgo `internal/tracing`): a record of a compilation
//! for performance analysis. A session writes, into the directory the
//! option names:
//!
//! - `trace.json`, a Chrome trace-event array: the compilation's phases as
//!   begin/end pairs (program creation, each file's parse, bind and check,
//!   the emit), instants where the checker hit a depth limit, and the
//!   sampled events (an expression's check, a relation) whose duration
//!   crosses a 10 ms sampling boundary;
//! - `types_<n>.json` for each checker `n`, one descriptor per type the
//!   checker created, in creation order;
//! - `legend.json`, which names the trace and the types file of each
//!   checker.
//!
//! A session is shared by the program loader, the checkers (each through a
//! [`CheckerTracer`]) and the emit; every method takes `&self` and the
//! events are written under one lock. Under a test harness the session is
//! *deterministic* (tsgo's `deterministic` mode): every timestamp is the
//! next value of a counter and no sampled event is written, so the output
//! does not depend on timing.
//!
//! The files are kept in memory and returned by [`Tracing::finish`]; the
//! command writes them through its file system. (tsgo flushes the trace to
//! disk every 256 KiB; the events of a program are a few megabytes.)
//!
//! This is unrelated to [`crate::trace`], the opt-in phase timing printed
//! to stderr.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// The trace file's name in the session's directory.
pub const TRACE_FILE_NAME: &str = "trace.json";
/// The legend file's name in the session's directory.
pub const LEGEND_FILE_NAME: &str = "legend.json";

const MAIN_THREAD_ID: i64 = 1;
const FIRST_SYNTHETIC_THREAD_ID: i64 = 2;
const FIRST_FILE_THREAD_ID: u64 = 1_000_000;
const FILE_THREAD_ID_HASH_RANGE: u64 = 1_000_000_000;
/// TypeScript's 10 ms sampling interval: an event that does not separate
/// its begin and end is written only when its span crosses a multiple of
/// it (counted from the session's start).
const SAMPLE_INTERVAL: Duration = Duration::from_millis(10);
/// The argument keys that place an event on a file's thread, in priority
/// order (tsgo `traceThreadArgKeys`).
const THREAD_ARG_KEYS: [&str; 5] = [
    "path",
    "fileName",
    "containingFileName",
    "jsFilePath",
    "declarationFilePath",
];

/// The category of an event (tsgo `tracing.Phase`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Parse,
    Program,
    Bind,
    Check,
    CheckTypes,
    Emit,
    Session,
}

impl Phase {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::Program => "program",
            Self::Bind => "bind",
            Self::Check => "check",
            Self::CheckTypes => "checkTypes",
            Self::Emit => "emit",
            Self::Session => "session",
        }
    }
}

/// One argument of an event.
#[derive(Clone, Debug, PartialEq)]
pub enum ArgValue {
    Str(String),
    Int(i64),
    Bool(bool),
    StrList(Vec<String>),
}

impl From<&str> for ArgValue {
    fn from(value: &str) -> Self {
        Self::Str(value.to_owned())
    }
}

impl From<String> for ArgValue {
    fn from(value: String) -> Self {
        Self::Str(value)
    }
}

impl From<Vec<String>> for ArgValue {
    fn from(value: Vec<String>) -> Self {
        Self::StrList(value)
    }
}

impl From<bool> for ArgValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

macro_rules! int_arg {
    ($($ty:ty),*) => {
        $(impl From<$ty> for ArgValue {
            fn from(value: $ty) -> Self {
                Self::Int(i64::try_from(value).unwrap_or(i64::MAX))
            }
        })*
    };
}
int_arg!(i32, i64, u32, u64, usize);

/// The arguments of an event (tsgo's `map[string]any`). They are written
/// with the keys in sorted order, as tsgo's deterministic JSON writes a map.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Args(Vec<(&'static str, ArgValue)>);

impl Args {
    pub fn new() -> Self {
        Self::default()
    }

    /// The arguments with `key` set to `value`.
    pub fn with(mut self, key: &'static str, value: impl Into<ArgValue>) -> Self {
        self.set(key, value.into());
        self
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn set(&mut self, key: &'static str, value: ArgValue) {
        match self.0.iter_mut().find(|(existing, _)| *existing == key) {
            Some(entry) => entry.1 = value,
            None => self.0.push((key, value)),
        }
    }

    fn get(&self, key: &str) -> Option<&ArgValue> {
        self.0
            .iter()
            .find(|(existing, _)| *existing == key)
            .map(|(_, value)| value)
    }

    /// The thread an event with these arguments belongs to (tsgo
    /// `traceThreadKeyFromArgs`): a checker's, a file's, or none (the main
    /// thread).
    fn thread_key(&self) -> Option<ThreadKey> {
        if let Some(ArgValue::Int(index)) = self.get("checkerId") {
            return Some(ThreadKey::Checker(*index));
        }
        THREAD_ARG_KEYS.iter().find_map(|key| match self.get(key) {
            Some(ArgValue::Str(path)) if !path.is_empty() => Some(ThreadKey::File(path.clone())),
            _ => None,
        })
    }

    fn write(&self, out: &mut String) {
        let mut entries = self.0.iter().collect::<Vec<_>>();
        entries.sort_by_key(|(key, _)| *key);
        out.push('{');
        for (index, (key, value)) in entries.into_iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            push_json_string(out, key);
            out.push(':');
            match value {
                ArgValue::Str(text) => push_json_string(out, text),
                ArgValue::Int(number) => out.push_str(&number.to_string()),
                ArgValue::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
                ArgValue::StrList(items) => {
                    out.push('[');
                    for (index, item) in items.iter().enumerate() {
                        if index > 0 {
                            out.push(',');
                        }
                        push_json_string(out, item);
                    }
                    out.push(']');
                }
            }
        }
        out.push('}');
    }
}

/// The thread of an event: Chrome's trace viewer shows each as a row.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum ThreadKey {
    Checker(i64),
    File(String),
}

impl ThreadKey {
    /// tsgo `defaultThreadID`: a checker's row follows the main thread's; a
    /// file's is a stable hash of its name.
    fn default_id(&self) -> i64 {
        match self {
            Self::Checker(index) if *index >= 0 => FIRST_SYNTHETIC_THREAD_ID + index,
            _ => {
                let hash = xxhash_rust::xxh3::xxh3_64(self.display_name().as_bytes());
                (FIRST_FILE_THREAD_ID + hash % FILE_THREAD_ID_HASH_RANGE) as i64
            }
        }
    }

    fn display_name(&self) -> String {
        match self {
            Self::Checker(index) => format!("checker:{index}"),
            Self::File(path) => format!("file:{path}"),
        }
    }
}

/// One checker's entry of the legend (tsgo `TraceRecord`).
#[derive(Clone, Debug)]
struct TraceRecord {
    config_file_path: String,
    trace_path: String,
    types_path: String,
    checker_id: usize,
}

/// A file a session produced, for the command to write.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraceFile {
    pub path: String,
    pub text: String,
}

#[derive(Default)]
struct State {
    /// The trace's events so far, each preceded by `",\n"` but the first.
    events: String,
    counter: u64,
    metadata_ts: f64,
    thread_ids: HashMap<ThreadKey, i64>,
    taken_thread_ids: HashMap<i64, ThreadKey>,
    legend: Vec<TraceRecord>,
    /// Each checker's types file, by checker.
    types: BTreeMap<usize, String>,
    finished: bool,
}

/// A `--generateTrace` session (tsgo `tracing.Tracing`).
pub struct Tracing {
    trace_dir: String,
    trace_path: String,
    config_file_path: String,
    deterministic: bool,
    started: Instant,
    state: Mutex<State>,
}

impl fmt::Debug for Tracing {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Tracing")
            .field("trace_dir", &self.trace_dir)
            .field("deterministic", &self.deterministic)
            .finish_non_exhaustive()
    }
}

/// Which event of a span is written.
#[derive(Clone, Copy)]
enum Edge {
    Begin,
    End,
}

impl Tracing {
    /// Starts a session writing into `trace_dir` for the program of
    /// `config_file_path` (empty without a config file), with its metadata
    /// events (tsgo `StartTracing`).
    pub fn start(trace_dir: &str, config_file_path: &str, deterministic: bool) -> Arc<Self> {
        let tracing = Self {
            trace_dir: trace_dir.to_owned(),
            trace_path: combine_paths(trace_dir, TRACE_FILE_NAME),
            config_file_path: config_file_path.to_owned(),
            deterministic,
            started: Instant::now(),
            state: Mutex::new(State::default()),
        };
        {
            let mut state = tracing.lock();
            let ts = tracing.timestamp(&mut state, None);
            state.metadata_ts = ts;
            let main_thread = Args::new().with("name", "tsgo");
            write_event(
                &mut state.events,
                MAIN_THREAD_ID,
                "M",
                "__metadata",
                ts,
                "process_name",
                None,
                None,
                Some(&main_thread),
            );
            state.events.push_str(",\n");
            write_event(
                &mut state.events,
                MAIN_THREAD_ID,
                "M",
                "__metadata",
                ts,
                "thread_name",
                None,
                None,
                Some(&Args::new().with("name", "Main")),
            );
            state.events.push_str(",\n");
            write_event(
                &mut state.events,
                MAIN_THREAD_ID,
                "M",
                "disabled-by-default-devtools.timeline",
                ts,
                "TracingStartedInBrowser",
                None,
                None,
                None,
            );
        }
        Arc::new(tracing)
    }

    /// The directory the session writes into.
    pub fn trace_dir(&self) -> &str {
        &self.trace_dir
    }

    /// Whether timestamps are a counter (a test harness's session).
    pub fn is_deterministic(&self) -> bool {
        self.deterministic
    }

    /// A begin event now; the span writes its end event when it ends (tsgo
    /// `Push` with `separateBeginAndEnd`).
    pub fn begin(self: &Arc<Self>, phase: Phase, name: &'static str, args: Args) -> Span {
        let tid = self.write_span_event(phase, name, &args, Edge::Begin, None, None);
        Span {
            tracing: Some(Arc::clone(self)),
            phase,
            name,
            args,
            tid,
        }
    }

    /// A begin and end event for work measured elsewhere, from `start` to
    /// `end`; a deterministic session takes the next two counter values.
    pub fn record_span(
        &self,
        phase: Phase,
        name: &'static str,
        args: &Args,
        start: Instant,
        end: Instant,
    ) {
        let tid = self.write_span_event(phase, name, args, Edge::Begin, Some(start), None);
        self.write_span_event(phase, name, args, Edge::End, Some(end), tid);
    }

    /// An event written only when its span crosses a sampling boundary
    /// (tsgo `Push` without `separateBeginAndEnd`). A deterministic session
    /// writes none, and `args` is not computed for it.
    pub fn sample(
        self: &Arc<Self>,
        phase: Phase,
        name: &'static str,
        args: impl FnOnce() -> Args,
    ) -> Sample {
        if self.deterministic {
            return Sample { inner: None };
        }
        Sample {
            inner: Some(SampleInner {
                tracing: Arc::clone(self),
                phase,
                name,
                args: args(),
                started: Instant::now(),
            }),
        }
    }

    /// An instant event (tsgo `Instant`): a checker's depth limit.
    pub fn instant(&self, phase: Phase, name: &'static str, args: Args) {
        let mut state = self.lock();
        if state.finished {
            return;
        }
        let ts = self.timestamp(&mut state, None);
        let tid = Self::thread_id(&mut state, &args);
        state.events.push_str(",\n");
        write_event(
            &mut state.events,
            tid,
            "I",
            phase.name(),
            ts,
            name,
            Some("g"),
            None,
            Some(&args),
        );
    }

    /// Adds checker `index` to the legend (tsgo `NewTypeTracer`).
    pub fn register_checker(&self, index: usize) {
        let types_path = combine_paths(&self.trace_dir, &format!("types_{index}.json"));
        let mut state = self.lock();
        if state
            .legend
            .iter()
            .any(|record| record.types_path == types_path)
        {
            return;
        }
        state.legend.push(TraceRecord {
            config_file_path: self.config_file_path.clone(),
            trace_path: self.trace_path.clone(),
            types_path,
            checker_id: index,
        });
    }

    /// Checker `index`'s types file: one descriptor per line, as
    /// [`TypesFileWriter`] writes them.
    pub fn set_checker_types(&self, index: usize, text: String) {
        self.lock().types.insert(index, text);
    }

    /// Ends the session and returns its files (tsgo `StopTracing`): the
    /// trace, each checker's types file, and the legend sorted by the types
    /// files' paths.
    pub fn finish(&self) -> Vec<TraceFile> {
        let mut state = self.lock();
        state.finished = true;
        let mut files = Vec::new();
        let mut trace = String::with_capacity(state.events.len() + 8);
        trace.push_str("[\n");
        trace.push_str(&state.events);
        trace.push_str("\n]\n");
        files.push(TraceFile {
            path: self.trace_path.clone(),
            text: trace,
        });
        for (index, text) in std::mem::take(&mut state.types) {
            files.push(TraceFile {
                path: combine_paths(&self.trace_dir, &format!("types_{index}.json")),
                text,
            });
        }
        let mut legend = std::mem::take(&mut state.legend);
        legend.sort_by(|left, right| left.types_path.cmp(&right.types_path));
        files.push(TraceFile {
            path: combine_paths(&self.trace_dir, LEGEND_FILE_NAME),
            text: legend_text(&legend),
        });
        files
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The timestamp in microseconds since the session started, at `at`
    /// (now when absent); the next counter value when deterministic.
    fn timestamp(&self, state: &mut State, at: Option<Instant>) -> f64 {
        if self.deterministic {
            state.counter += 1;
            return state.counter as f64;
        }
        let at = at.unwrap_or_else(Instant::now);
        micros(at.saturating_duration_since(self.started))
    }

    /// The thread of `args` (tsgo `threadIDLocked`): a new thread is named
    /// by a metadata event before its first event.
    fn thread_id(state: &mut State, args: &Args) -> i64 {
        let Some(key) = args.thread_key() else {
            return MAIN_THREAD_ID;
        };
        if let Some(&tid) = state.thread_ids.get(&key) {
            return tid;
        }
        let mut tid = key.default_id();
        while state
            .taken_thread_ids
            .get(&tid)
            .is_some_and(|existing| *existing != key)
        {
            tid += 1;
        }
        state.thread_ids.insert(key.clone(), tid);
        state.taken_thread_ids.insert(tid, key.clone());
        let metadata_ts = state.metadata_ts;
        state.events.push_str(",\n");
        write_event(
            &mut state.events,
            tid,
            "M",
            "__metadata",
            metadata_ts,
            "thread_name",
            None,
            None,
            Some(&Args::new().with("name", key.display_name())),
        );
        tid
    }

    /// Writes one begin or end event and returns its thread; an end event
    /// stays on its begin event's thread.
    fn write_span_event(
        &self,
        phase: Phase,
        name: &'static str,
        args: &Args,
        which: Edge,
        at: Option<Instant>,
        tid: Option<i64>,
    ) -> Option<i64> {
        let mut state = self.lock();
        if state.finished {
            return tid;
        }
        let ts = self.timestamp(&mut state, at);
        let tid = match tid {
            Some(tid) => tid,
            None => Self::thread_id(&mut state, args),
        };
        state.events.push_str(",\n");
        write_event(
            &mut state.events,
            tid,
            match which {
                Edge::Begin => "B",
                Edge::End => "E",
            },
            phase.name(),
            ts,
            name,
            None,
            None,
            (!args.is_empty()).then_some(args),
        );
        Some(tid)
    }
}

/// A begin event's span: its end event is written when the span ends or
/// is dropped.
#[must_use = "the span ends when it is dropped"]
pub struct Span {
    tracing: Option<Arc<Tracing>>,
    phase: Phase,
    name: &'static str,
    args: Args,
    tid: Option<i64>,
}

impl Span {
    /// Sets an argument of the end event: tsgo writes the arguments as they
    /// are when the span ends (`getVariancesWorker` adds its result).
    pub fn set_arg(&mut self, key: &'static str, value: impl Into<ArgValue>) {
        self.args.set(key, value.into());
    }

    /// Ends the span now.
    pub fn end(self) {}
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some(tracing) = self.tracing.take() {
            tracing.write_span_event(self.phase, self.name, &self.args, Edge::End, None, self.tid);
        }
    }
}

struct SampleInner {
    tracing: Arc<Tracing>,
    phase: Phase,
    name: &'static str,
    args: Args,
    started: Instant,
}

/// A sampled event: when it ends, it is written as a complete (`X`) event
/// if its span crossed a sampling boundary.
#[must_use = "the sample ends when it is dropped"]
pub struct Sample {
    inner: Option<SampleInner>,
}

impl Drop for Sample {
    fn drop(&mut self) {
        let Some(inner) = self.inner.take() else {
            return;
        };
        let duration = micros(inner.started.elapsed());
        let start = micros(
            inner
                .started
                .saturating_duration_since(inner.tracing.started),
        );
        let interval = micros(SAMPLE_INTERVAL);
        if interval - start % interval > duration {
            return;
        }
        let mut state = inner.tracing.lock();
        if state.finished {
            return;
        }
        let tid = Tracing::thread_id(&mut state, &inner.args);
        state.events.push_str(",\n");
        write_event(
            &mut state.events,
            tid,
            "X",
            inner.phase.name(),
            start,
            inner.name,
            None,
            Some(duration),
            (!inner.args.is_empty()).then_some(&inner.args),
        );
    }
}

/// One checker's view of a session (tsgo `checker.Tracer`): its events
/// carry its `checkerId`, and its types file is listed in the legend.
#[derive(Clone, Debug)]
pub struct CheckerTracer {
    tracing: Arc<Tracing>,
    index: usize,
}

impl CheckerTracer {
    pub fn new(tracing: Arc<Tracing>, index: usize) -> Self {
        tracing.register_checker(index);
        Self { tracing, index }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn tracing(&self) -> &Arc<Tracing> {
        &self.tracing
    }

    pub fn begin(&self, phase: Phase, name: &'static str, args: Args) -> Span {
        self.tracing
            .begin(phase, name, args.with("checkerId", self.index))
    }

    pub fn sample(&self, phase: Phase, name: &'static str, args: impl FnOnce() -> Args) -> Sample {
        let index = self.index;
        self.tracing
            .sample(phase, name, || args().with("checkerId", index))
    }

    pub fn instant(&self, phase: Phase, name: &'static str, args: Args) {
        self.tracing
            .instant(phase, name, args.with("checkerId", self.index));
    }

    /// Stores this checker's types file in the session.
    pub fn set_types(&self, text: String) {
        self.tracing.set_checker_types(self.index, text);
    }
}

/// A position in a source file, 1-based in lines and UTF-16 characters
/// (tsgo `LineAndChar`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LineAndChar {
    pub line: u32,
    pub character: u32,
}

/// A range of a source file (tsgo `tracing.Location`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Location {
    pub path: String,
    pub start: LineAndChar,
    pub end: LineAndChar,
}

/// One type's descriptor in a types file (tsgo `TypeDescriptor`); the
/// fields are written in tsgo's order, absent ones omitted.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TypeDescriptor {
    pub id: u32,
    pub intrinsic_name: Option<String>,
    pub symbol_name: Option<String>,
    pub recursion_id: Option<usize>,
    pub is_tuple: bool,
    pub union_types: Vec<u32>,
    pub intersection_types: Vec<u32>,
    pub alias_type_arguments: Vec<u32>,
    pub keyof_type: Option<u32>,
    pub indexed_access_object_type: Option<u32>,
    pub indexed_access_index_type: Option<u32>,
    pub conditional_check_type: Option<u32>,
    pub conditional_extends_type: Option<u32>,
    /// `-1` for a branch the checker has not resolved.
    pub conditional_true_type: Option<i64>,
    pub conditional_false_type: Option<i64>,
    pub substitution_base_type: Option<u32>,
    pub constraint_type: Option<u32>,
    pub instantiated_type: Option<u32>,
    pub type_arguments: Vec<u32>,
    pub reference_location: Option<Location>,
    pub reverse_mapped_source_type: Option<u32>,
    pub reverse_mapped_mapped_type: Option<u32>,
    pub reverse_mapped_constraint_type: Option<u32>,
    pub evolving_array_element_type: Option<u32>,
    pub evolving_array_final_type: Option<u32>,
    pub destructuring_pattern: Option<Location>,
    pub first_declaration: Option<Location>,
    pub flags: Vec<&'static str>,
    pub display: Option<String>,
}

impl TypeDescriptor {
    fn write(&self, out: &mut String) {
        let mut object = JsonObject::new(out);
        object.number("id", i64::from(self.id));
        if let Some(name) = &self.intrinsic_name {
            object.string("intrinsicName", name);
        }
        if let Some(name) = &self.symbol_name {
            object.string("symbolName", name);
        }
        if let Some(id) = self.recursion_id {
            object.number("recursionId", id as i64);
        }
        if self.is_tuple {
            object.raw("isTuple", "true");
        }
        object.ids("unionTypes", &self.union_types);
        object.ids("intersectionTypes", &self.intersection_types);
        object.ids("aliasTypeArguments", &self.alias_type_arguments);
        object.id("keyofType", self.keyof_type);
        object.id("indexedAccessObjectType", self.indexed_access_object_type);
        object.id("indexedAccessIndexType", self.indexed_access_index_type);
        object.id("conditionalCheckType", self.conditional_check_type);
        object.id("conditionalExtendsType", self.conditional_extends_type);
        if let Some(id) = self.conditional_true_type {
            object.number("conditionalTrueType", id);
        }
        if let Some(id) = self.conditional_false_type {
            object.number("conditionalFalseType", id);
        }
        object.id("substitutionBaseType", self.substitution_base_type);
        object.id("constraintType", self.constraint_type);
        object.id("instantiatedType", self.instantiated_type);
        object.ids("typeArguments", &self.type_arguments);
        object.location("referenceLocation", self.reference_location.as_ref());
        object.id("reverseMappedSourceType", self.reverse_mapped_source_type);
        object.id("reverseMappedMappedType", self.reverse_mapped_mapped_type);
        object.id(
            "reverseMappedConstraintType",
            self.reverse_mapped_constraint_type,
        );
        object.id("evolvingArrayElementType", self.evolving_array_element_type);
        object.id("evolvingArrayFinalType", self.evolving_array_final_type);
        object.location("destructuringPattern", self.destructuring_pattern.as_ref());
        object.location("firstDeclaration", self.first_declaration.as_ref());
        let mut flags = String::from("[");
        for (index, flag) in self.flags.iter().enumerate() {
            if index > 0 {
                flags.push(',');
            }
            push_json_string(&mut flags, flag);
        }
        flags.push(']');
        object.raw("flags", &flags);
        if let Some(display) = &self.display {
            object.string("display", display);
        }
        object.end();
    }
}

/// Writes a types file (tsgo `DumpTypes`): `[`, the descriptors separated
/// by `",\n"` (so that a type's line is its id), and `"]\n"`.
#[derive(Default)]
pub struct TypesFileWriter {
    text: String,
    count: usize,
}

impl TypesFileWriter {
    pub fn new() -> Self {
        Self {
            text: String::from("["),
            count: 0,
        }
    }

    pub fn push(&mut self, descriptor: &TypeDescriptor) {
        if self.count > 0 {
            self.text.push_str(",\n");
        }
        descriptor.write(&mut self.text);
        self.count += 1;
    }

    /// The file, or none when no type was written (tsgo writes no file
    /// for a checker without types).
    pub fn finish(mut self) -> Option<String> {
        (self.count > 0).then(|| {
            self.text.push_str("]\n");
            self.text
        })
    }
}

/// The names of the bits of a type's flags (tsgo `FormatTypeFlags`), or
/// `None` when no bit is set.
pub fn format_type_flags(flags: i32) -> Vec<&'static str> {
    const NAMES: [&str; 29] = [
        "Any",
        "Unknown",
        "Undefined",
        "Null",
        "Void",
        "String",
        "Number",
        "BigInt",
        "Boolean",
        "ESSymbol",
        "StringLiteral",
        "NumberLiteral",
        "BigIntLiteral",
        "BooleanLiteral",
        "UniqueESSymbol",
        "EnumLiteral",
        "Enum",
        "NonPrimitive",
        "Never",
        "TypeParameter",
        "Object",
        "Index",
        "TemplateLiteral",
        "StringMapping",
        "Substitution",
        "IndexedAccess",
        "Conditional",
        "Union",
        "Intersection",
    ];
    let names = NAMES
        .iter()
        .enumerate()
        .filter(|(bit, _)| flags & (1 << bit) != 0)
        .map(|(_, name)| *name)
        .collect::<Vec<_>>();
    if names.is_empty() {
        vec!["None"]
    } else {
        names
    }
}

/// A JSON object written field by field.
struct JsonObject<'a> {
    out: &'a mut String,
    first: bool,
}

impl<'a> JsonObject<'a> {
    fn new(out: &'a mut String) -> Self {
        out.push('{');
        Self { out, first: true }
    }

    fn key(&mut self, key: &str) {
        if !self.first {
            self.out.push(',');
        }
        self.first = false;
        push_json_string(self.out, key);
        self.out.push(':');
    }

    fn raw(&mut self, key: &str, value: &str) {
        self.key(key);
        self.out.push_str(value);
    }

    fn string(&mut self, key: &str, value: &str) {
        self.key(key);
        push_json_string(self.out, value);
    }

    fn number(&mut self, key: &str, value: i64) {
        self.key(key);
        self.out.push_str(&value.to_string());
    }

    fn id(&mut self, key: &str, value: Option<u32>) {
        if let Some(value) = value {
            self.number(key, i64::from(value));
        }
    }

    fn ids(&mut self, key: &str, values: &[u32]) {
        if values.is_empty() {
            return;
        }
        self.key(key);
        self.out.push('[');
        for (index, value) in values.iter().enumerate() {
            if index > 0 {
                self.out.push(',');
            }
            self.out.push_str(&value.to_string());
        }
        self.out.push(']');
    }

    fn location(&mut self, key: &str, value: Option<&Location>) {
        let Some(location) = value else {
            return;
        };
        self.key(key);
        let mut object = JsonObject::new(self.out);
        object.string("path", &location.path);
        for (key, position) in [("start", location.start), ("end", location.end)] {
            object.key(key);
            let mut inner = JsonObject::new(object.out);
            inner.number("line", i64::from(position.line));
            inner.number("character", i64::from(position.character));
            inner.end();
        }
        object.end();
    }

    fn end(self) {
        self.out.push('}');
    }
}

/// tsgo `traceEvent`, written with its fields in declaration order and the
/// optional ones omitted when empty.
#[allow(clippy::too_many_arguments)]
fn write_event(
    out: &mut String,
    tid: i64,
    ph: &str,
    cat: &str,
    ts: f64,
    name: &str,
    scope: Option<&str>,
    duration: Option<f64>,
    args: Option<&Args>,
) {
    out.push_str("{\"pid\":1,\"tid\":");
    out.push_str(&tid.to_string());
    out.push_str(",\"ph\":");
    push_json_string(out, ph);
    out.push_str(",\"cat\":");
    push_json_string(out, cat);
    out.push_str(",\"ts\":");
    push_json_number(out, ts);
    if !name.is_empty() {
        out.push_str(",\"name\":");
        push_json_string(out, name);
    }
    if let Some(scope) = scope {
        out.push_str(",\"s\":");
        push_json_string(out, scope);
    }
    if let Some(duration) = duration {
        out.push_str(",\"dur\":");
        push_json_number(out, duration);
    }
    if let Some(args) = args {
        out.push_str(",\"args\":");
        args.write(out);
    }
    out.push('}');
}

/// The legend as tsgo's `json.MarshalIndent(legend, "", "  ")` writes it.
fn legend_text(legend: &[TraceRecord]) -> String {
    if legend.is_empty() {
        return "[]".to_owned();
    }
    let mut out = String::from("[\n");
    for (index, record) in legend.iter().enumerate() {
        if index > 0 {
            out.push_str(",\n");
        }
        out.push_str("  {\n");
        for (key, value) in [
            ("configFilePath", &record.config_file_path),
            ("tracePath", &record.trace_path),
            ("typesPath", &record.types_path),
        ] {
            if value.is_empty() {
                continue;
            }
            out.push_str("    ");
            push_json_string(&mut out, key);
            out.push_str(": ");
            push_json_string(&mut out, value);
            out.push_str(",\n");
        }
        out.push_str("    \"checkerId\": ");
        out.push_str(&record.checker_id.to_string());
        out.push_str("\n  }");
    }
    out.push_str("\n]");
    out
}

fn micros(duration: Duration) -> f64 {
    duration.as_nanos() as f64 / 1000.0
}

/// tsgo `tspath.CombinePaths` of a directory and a file name.
fn combine_paths(directory: &str, file_name: &str) -> String {
    if directory.is_empty() {
        return file_name.to_owned();
    }
    if directory.ends_with('/') {
        format!("{directory}{file_name}")
    } else {
        format!("{directory}/{file_name}")
    }
}

/// A number as Go's JSON writer prints a float64: the shortest decimal
/// that reads back, without an exponent in a trace's range.
fn push_json_number(out: &mut String, value: f64) {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        out.push_str(&(value as i64).to_string());
    } else {
        out.push_str(&value.to_string());
    }
}

/// A JSON string as Go's `encoding/json/v2` writes it (no HTML escaping).
pub fn push_json_string(out: &mut String, text: &str) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if (character as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => out.push(character),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace_text(tracing: &Tracing) -> String {
        tracing
            .finish()
            .into_iter()
            .find(|file| file.path.ends_with(TRACE_FILE_NAME))
            .expect("trace file")
            .text
    }

    #[test]
    fn file_threads_hash_like_tsgo() {
        // tsgo's thread ids in its generateTrace baselines.
        assert_eq!(
            ThreadKey::File("/home/src/tslibs/TS/Lib/lib.es2026.full.d.ts".to_owned()).default_id(),
            181_085_097
        );
        assert_eq!(
            ThreadKey::File("/home/src/workspaces/project/a.ts".to_owned()).default_id(),
            354_130_385
        );
        assert_eq!(ThreadKey::Checker(0).default_id(), 2);
    }

    #[test]
    fn deterministic_session_writes_tsgo_events() {
        let tracing = Tracing::start("/p/trace", "/p/tsconfig.json", true);
        let program = tracing.begin(
            Phase::Program,
            "createProgram",
            Args::new().with("configFilePath", "/p/tsconfig.json"),
        );
        let now = Instant::now();
        tracing.record_span(
            Phase::Parse,
            "createSourceFile",
            &Args::new().with("path", "/p/a.ts"),
            now,
            now,
        );
        program.end();
        let checker = CheckerTracer::new(Arc::clone(&tracing), 0);
        {
            let _span = checker.begin(
                Phase::Check,
                "checkSourceFile",
                Args::new().with("path", "/p/a.ts"),
            );
            checker.instant(
                Phase::CheckTypes,
                "getTypeAtFlowNode_DepthLimit",
                Args::new().with("depth", 2000u32),
            );
            // No sampled event in a deterministic session.
            let _sample = checker.sample(Phase::Check, "checkExpression", Args::new);
        }
        let trace = trace_text(&tracing);
        let expected = [
            "[",
            r#"{"pid":1,"tid":1,"ph":"M","cat":"__metadata","ts":1,"name":"process_name","args":{"name":"tsgo"}},"#,
            r#"{"pid":1,"tid":1,"ph":"M","cat":"__metadata","ts":1,"name":"thread_name","args":{"name":"Main"}},"#,
            r#"{"pid":1,"tid":1,"ph":"M","cat":"disabled-by-default-devtools.timeline","ts":1,"name":"TracingStartedInBrowser"},"#,
            r#"{"pid":1,"tid":1,"ph":"B","cat":"program","ts":2,"name":"createProgram","args":{"configFilePath":"/p/tsconfig.json"}},"#,
        ];
        for (line, expected) in trace.lines().zip(expected) {
            assert_eq!(line, expected);
        }
        assert!(trace.contains(
            r#""ph":"B","cat":"parse","ts":3,"name":"createSourceFile","args":{"path":"/p/a.ts"}}"#
        ));
        assert!(trace.contains(r#"{"pid":1,"tid":2,"ph":"M","cat":"__metadata","ts":1,"name":"thread_name","args":{"name":"checker:0"}}"#));
        assert!(trace.contains(r#"{"pid":1,"tid":2,"ph":"B","cat":"check","ts":6,"name":"checkSourceFile","args":{"checkerId":0,"path":"/p/a.ts"}}"#));
        assert!(trace.contains(r#"{"pid":1,"tid":2,"ph":"I","cat":"checkTypes","ts":7,"name":"getTypeAtFlowNode_DepthLimit","s":"g","args":{"checkerId":0,"depth":2000}}"#));
        assert!(!trace.contains("checkExpression"));
        assert!(trace.ends_with("}\n]\n"));
    }

    #[test]
    fn legend_and_types_files() {
        let tracing = Tracing::start("/p/trace", "/p/tsconfig.json", true);
        let first = CheckerTracer::new(Arc::clone(&tracing), 1);
        let second = CheckerTracer::new(Arc::clone(&tracing), 0);
        let mut types = TypesFileWriter::new();
        types.push(&TypeDescriptor {
            id: 1,
            intrinsic_name: Some("any".to_owned()),
            recursion_id: Some(0),
            flags: format_type_flags(1),
            ..TypeDescriptor::default()
        });
        types.push(&TypeDescriptor {
            id: 2,
            symbol_name: Some("Array".to_owned()),
            recursion_id: Some(1),
            type_arguments: vec![1],
            instantiated_type: Some(2),
            first_declaration: Some(Location {
                path: "/lib.d.ts".to_owned(),
                start: LineAndChar {
                    line: 1,
                    character: 1,
                },
                end: LineAndChar {
                    line: 1,
                    character: 5,
                },
            }),
            flags: format_type_flags(1 << 20),
            ..TypeDescriptor::default()
        });
        first.set_types(types.finish().expect("two types"));
        assert!(TypesFileWriter::new().finish().is_none());
        drop(second);
        let files = tracing.finish();
        let names = files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "/p/trace/trace.json",
                "/p/trace/types_1.json",
                "/p/trace/legend.json"
            ]
        );
        assert_eq!(
            files[1].text,
            "[{\"id\":1,\"intrinsicName\":\"any\",\"recursionId\":0,\"flags\":[\"Any\"]},\n\
             {\"id\":2,\"symbolName\":\"Array\",\"recursionId\":1,\"instantiatedType\":2,\"typeArguments\":[1],\
             \"firstDeclaration\":{\"path\":\"/lib.d.ts\",\"start\":{\"line\":1,\"character\":1},\"end\":{\"line\":1,\"character\":5}},\
             \"flags\":[\"Object\"]}]\n"
        );
        assert_eq!(
            files[2].text,
            "[\n  {\n    \"configFilePath\": \"/p/tsconfig.json\",\n    \"tracePath\": \"/p/trace/trace.json\",\n    \"typesPath\": \"/p/trace/types_0.json\",\n    \"checkerId\": 0\n  },\n  {\n    \"configFilePath\": \"/p/tsconfig.json\",\n    \"tracePath\": \"/p/trace/trace.json\",\n    \"typesPath\": \"/p/trace/types_1.json\",\n    \"checkerId\": 1\n  }\n]"
        );
    }

    #[test]
    fn flag_names() {
        assert_eq!(format_type_flags(0), ["None"]);
        assert_eq!(
            format_type_flags((1 << 8) | (1 << 27)),
            ["Boolean", "Union"]
        );
    }
}
