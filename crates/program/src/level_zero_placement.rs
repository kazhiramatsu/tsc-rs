//! Thin level-0 caller of the scheduler-independent dense room API.
use super::*;
use crate::PreparsedSyntax;
use std::collections::{BTreeMap, VecDeque};
type RootRead = Option<Result<Option<Vec<u8>>, HostError>>;
use tsc_syntax::placement::{self as direct, SyntaxStripe};

#[derive(Default)]
pub(super) struct Stripes(Vec<(usize, &'static str, SyntaxStripe)>);

impl Stripes {
    pub(super) fn layout(&self) -> Arc<[direct::Placement]> {
        self.0
            .iter()
            .flat_map(|(_, _, stripe)| stripe.placements().iter().cloned())
            .collect::<Vec<_>>()
            .into()
    }
    pub(super) fn seal(&mut self) {
        for (_, label, stripe) in self.0.drain(..) {
            match stripe.seal() {
                Ok(statistics) => direct::trace_statistics(label, &statistics),
                Err(error) => eprintln!("[syntax-placement] stripe seal failed: {error}"),
            }
        }
    }
}
impl Drop for Stripes {
    fn drop(&mut self) {
        self.seal();
    }
}

struct Entry {
    path: ProgramPath,
    byte_len: usize,
    text: String,
    order: usize,
}
enum Job {
    Stripe(usize, Vec<Entry>, SyntaxStripe, &'static str),
    /// A worker takes the next ordered prefix, then parses it without the
    /// admission lock. Tokens have no stripe identity of their own.
    Roots,
    Ordinary(ProgramPath),
}

/// A completed parse keeps its payload while earlier stripes are still
/// running. Parking never occupies a worker or duplicates a source snapshot.
struct CompletedStripe {
    ordinal: usize,
    label: &'static str,
    sources: Vec<ReadAheadSource>,
    syntax: SyntaxStripe,
}

struct BindAdmission {
    domain: tsc_types::IdentityDomain,
    expected: VecDeque<usize>,
    parked: BTreeMap<usize, CompletedStripe>,
    exhausted: bool,
    stripes: usize,
    sources: usize,
    deferred: usize,
    elapsed: std::time::Duration,
}

impl BindAdmission {
    fn new(domain: tsc_types::IdentityDomain, libraries: usize) -> Self {
        Self {
            domain,
            expected: (0..libraries).collect(),
            parked: BTreeMap::new(),
            exhausted: false,
            stripes: 0,
            sources: 0,
            deferred: 0,
            elapsed: std::time::Duration::ZERO,
        }
    }

    fn syntax(source: &ReadAheadSource) -> Option<&PreparsedSyntax> {
        match &source.read {
            PrefetchedRead::Parsed {
                decoded: Ok(parsed),
                ..
            } => Some(parsed.prepared.preparsed_syntax()),
            _ => None,
        }
    }

    fn reserve(&mut self, stripe: &CompletedStripe) {
        let sources: Vec<_> = stripe
            .sources
            .iter()
            .filter_map(Self::syntax)
            .filter_map(|syntax| {
                syntax
                    .bind_counts(&self.domain)
                    .map(|counts| (syntax, counts))
            })
            .collect();
        if sources.is_empty() {
            return;
        }
        let totals = sources.iter().try_fold([0u32; 2], |sum, (_, counts)| {
            Some([
                sum[0].checked_add(counts[0])?,
                sum[1].checked_add(counts[1])?,
            ])
        });
        let room = totals
            .filter(|_| !self.exhausted)
            .and_then(|[symbols, serials]| {
                self.domain
                    .lease_rooms(&[vec![
                        (tsc_types::IdentitySpace::Symbol, symbols),
                        (tsc_types::IdentitySpace::PrivateNameSerial, serials),
                    ]])
                    .ok()?
                    .pop()
            });
        let Some(mut room) = room else {
            // Loading alone need not use a binder. An exhausted symbol
            // space must retain the ordinary driver's failure boundary.
            self.exhausted = true;
            self.deferred += sources.len();
            return;
        };
        for (syntax, counts) in sources {
            syntax.reserve_bind(
                tsc_binder::BindReservations::finish_entry(&mut room, counts)
                    .expect("exact bind slice fits its stripe"),
            );
            self.sources += 1;
        }
        room.seal().expect("exact bind room seals");
        self.stripes += 1;
    }

    fn complete(&mut self, stripe: CompletedStripe) -> Vec<CompletedStripe> {
        let started = tsc_types::trace::enabled().then(std::time::Instant::now);
        assert!(self.parked.insert(stripe.ordinal, stripe).is_none());
        let mut completed = Vec::new();
        while let Some(&ordinal) = self.expected.front() {
            let Some(stripe) = self.parked.remove(&ordinal) else {
                break;
            };
            self.expected.pop_front();
            self.reserve(&stripe);
            completed.push(stripe);
        }
        if let Some(started) = started {
            self.elapsed += started.elapsed();
        }
        completed
    }

    fn finish(&self) {
        assert!(self.expected.is_empty() && self.parked.is_empty());
        if tsc_types::trace::enabled() {
            eprintln!(
                "[bind-placement] exact stripes={} sources={} domain_ranges={} deferred_sources={}",
                self.stripes,
                self.sources,
                self.stripes * 2,
                self.deferred
            );
            let now = std::time::Instant::now();
            tsc_types::trace::mark(
                "load: direct bind stripe admission (summed)",
                now.checked_sub(self.elapsed).unwrap_or(now),
            );
        }
    }
}

/// Three readers plus the admitting worker make at most four active root
/// reads. The crew may get only four reads ahead of ordered admission. This
/// bounds retained raw buffers even when decoding one root is slow. A parser
/// can take an unclaimed SourceReads slot itself, including at one worker.
struct ReadWindow {
    state: Mutex<(usize, bool)>,
    changed: std::sync::Condvar,
}
impl ReadWindow {
    fn read(&self, index: usize, read: impl FnOnce() -> RootRead) -> RootRead {
        let mut state = self.state.lock().expect("direct read window");
        while index >= state.0.saturating_add(4) && !state.1 {
            state = self.changed.wait(state).expect("direct read window");
        }
        if state.1 {
            return None;
        }
        drop(state);
        read()
    }
    fn advance(&self, next: usize, stopped: bool) {
        *self.state.lock().expect("direct read window") = (next, stopped);
        self.changed.notify_all();
    }
}

struct StopReaders<'a>(&'a ReadWindow);
impl Drop for StopReaders<'_> {
    fn drop(&mut self) {
        self.0.advance(usize::MAX, true);
    }
}

/// Only ordered admission and leasing hold this lock, one candidate stripe
/// at a time. Parsing, planning and resolution never hold it. In particular,
/// a worker cannot reserve a later stripe and make an earlier owner wait for
/// that later stripe's reads.
struct RootStream<'a> {
    identity_domain: tsc_types::IdentityDomain,
    bind_admission: Option<&'a Mutex<BindAdmission>>,
    next: usize,
    width: usize,
    ordinal: usize,
    admitted: usize,
    leased: usize,
    reserved_sources: usize,
    reserved_bytes: usize,
    stopped: bool,
    complete: bool,
    other_reads: Vec<(ProgramPath, usize, PrefetchedRead)>,
    // Private reads must not race root admission for the resource budget.
    // They are queued, without blocking a worker, until root leasing ends.
    deferred: Vec<ProgramPath>,
    started: std::time::Instant,
}

impl RootStream<'_> {
    fn take(
        &mut self,
        roots: &[(usize, ProgramPath)],
        reads: &crate::source_reads::SourceReads<RootRead>,
        read: &impl Fn(usize) -> RootRead,
        window: &ReadWindow,
        limits: ProgramLoadLimits,
        pass: &ParallelReadAhead<'_>,
    ) -> Option<Job> {
        if self.complete {
            return None;
        }
        let end = (self.next + self.width).min(roots.len());
        let mut entries = Vec::new();
        while self.next < end && !self.stopped {
            let index = self.next;
            let (order, path) = &roots[index];
            if self.reserved_sources >= limits.max_source_files {
                self.stopped = true;
                break;
            }
            let Some(bytes) = reads.take(index, || read(index)) else {
                self.stopped = true;
                break;
            };
            match bytes {
                Err(error) => {
                    self.other_reads
                        .push((path.clone(), *order, PrefetchedRead::Failed(error)));
                    self.stopped = true;
                }
                Ok(None) => self
                    .other_reads
                    .push((path.clone(), *order, PrefetchedRead::Missing)),
                Ok(Some(bytes)) => {
                    let byte_len = bytes.len();
                    if byte_len > limits.max_source_file_bytes
                        || self.reserved_bytes.saturating_add(byte_len)
                            > limits.max_total_source_bytes
                    {
                        self.stopped = true;
                    } else {
                        self.reserved_sources += 1;
                        self.reserved_bytes += byte_len;
                        pass.retained_sources.fetch_add(1, Ordering::Relaxed);
                        pass.retained_bytes.fetch_add(byte_len, Ordering::Relaxed);
                        match decode_host_text(bytes) {
                            Ok(text) => {
                                entries.push(Entry {
                                    path: path.clone(),
                                    byte_len,
                                    text,
                                    order: *order,
                                });
                                self.admitted += 1;
                            }
                            Err(error) => {
                                self.other_reads.push((
                                    path.clone(),
                                    *order,
                                    PrefetchedRead::Parsed {
                                        byte_len,
                                        decoded: Err(error),
                                    },
                                ));
                                self.stopped = true;
                            }
                        }
                    }
                }
            }
            self.next += 1;
            window.advance(self.next, self.stopped);
        }
        let ordinal = self.ordinal;
        self.ordinal += 1;
        let job = (!entries.is_empty())
            .then(|| stripe_job(ordinal, "roots", entries, &self.identity_domain));
        if job.is_some() {
            // Empty/missing batches have no pending bind entry. Register
            // before the parsing job can complete, under ordered admission.
            if let Some(admission) = self.bind_admission {
                admission
                    .lock()
                    .expect("ordered bind admission")
                    .expected
                    .push_back(ordinal);
            }
            self.leased += 1;
            if self.leased == 1 {
                tsc_types::trace::mark("load: first root stripe read/decode/lease", self.started);
            }
        }
        if self.stopped || self.next == roots.len() {
            self.complete = true;
            pass.stopped.store(self.stopped, Ordering::Relaxed);
            window.advance(roots.len(), true);
            tsc_types::trace::mark("load: streaming root read/decode/leases", self.started);
        }
        job
    }
}

fn stripe_bounds(entries: &[Entry]) -> Vec<(tsc_types::IdentitySpace, u32)> {
    direct::bounds(
        entries.iter().map(|entry| entry.text.len()).sum(),
        entries.len(),
    )
    .unwrap_or_else(|| direct::SPACES.into_iter().map(|space| (space, 0)).collect())
}

fn stripe_job(
    ordinal: usize,
    label: &'static str,
    entries: Vec<Entry>,
    identity_domain: &tsc_types::IdentityDomain,
) -> Job {
    let room = identity_domain
        .lease_rooms(&[stripe_bounds(&entries)])
        .expect("ordered root stripe lease")
        .pop()
        .expect("one stripe");
    Job::Stripe(ordinal, entries, SyntaxStripe::new(room), label)
}

impl<'host: 'options, 'options, 'resolver> StagedGraph<'host, 'options, 'resolver> {
    pub(super) fn read_ahead_direct(&mut self, seeds: &[ProgramPath], root_count: usize) -> bool {
        // The finite caller owns the context; host capabilities determine
        // whether its level zero can be streamed at final addresses.
        let Some(identity_domain) = self.program_options.identity_domain() else {
            return false;
        };
        if self.program_options.parse_cache().is_some()
            || self.program_options.project_reference_sources()
            || !self.host.permits_source_read_ahead()
            || root_count == 0
        {
            return false;
        }
        let (Some(host), Some(reader)) = (
            self.resolution_host.parallel_resolution_host(),
            self.host.parallel_source_reader(),
        ) else {
            return false;
        };
        let phase_started = std::time::Instant::now();
        let limits = self.limits;
        let workers = limits.workers;
        let options = self.compiler_options;
        let program_options = self.program_options;
        let case_sensitive = self.resolver.path_context().use_case_sensitive_file_names();
        let mut seen = HashSet::default();
        let mut roots: Vec<_> = seeds[..root_count]
            .iter()
            .enumerate()
            .filter(|(_, path)| {
                path_has_extension(path.display())
                    && is_admitted_source(path.canonical(), options)
                    && !is_json_source(path.canonical())
                    && self.content_mapper_for(path).is_none()
                    && self
                        .project_reference_redirect(path.canonical())
                        .is_ok_and(|target| target.is_none())
                    && self.project_for_resolution(path.canonical()).is_none()
                    && seen.insert(path.canonical().clone())
            })
            .map(|(order, path)| (order, path.clone()))
            .collect();
        // Configuration and bundled-library directives alone determine this
        // prefix. No root has been read, and root-only library directives use
        // the ordinary discovered-source path.
        let library_started = std::time::Instant::now();
        let mut reserved_sources = self.sources.len() + self.reserved_sources;
        let mut reserved_bytes = self.total_source_bytes.saturating_add(self.reserved_bytes);
        let mut library_paths: Vec<_> = seeds[root_count..]
            .iter()
            .filter(|path| path.display().starts_with("bundled:///libs/"))
            .cloned()
            .collect();
        let mut library_seen = HashSet::default();
        let mut library_entries = Vec::new();
        let initial_reserved_sources = reserved_sources;
        let initial_reserved_bytes = reserved_bytes;
        while let Some(path) = library_paths.pop() {
            if !path.display().starts_with("bundled:///libs/")
                || !library_seen.insert(path.canonical().clone())
            {
                continue;
            }
            // On a bounded/failed library prefix, libraries all use the
            // ordinary path. Root admission then has the original budget.
            let bytes = reader.read_source_js(path.display());
            let Ok(Some(bytes)) = bytes else {
                library_entries.clear();
                break;
            };
            let byte_len = bytes.len();
            if byte_len > self.limits.max_source_file_bytes
                || reserved_sources >= self.limits.max_source_files
                || reserved_bytes.saturating_add(byte_len) > self.limits.max_total_source_bytes
            {
                library_entries.clear();
                break;
            }
            let Ok(text) = decode_host_text(bytes) else {
                library_entries.clear();
                break;
            };
            if let Some(catalog) = self.library_catalog {
                for reference in direct::leading_library_references(&text) {
                    let name = to_file_name_lower_case_js(reference.file_name.as_js());
                    if let Some(name) = catalog.reference_file_name(&name) {
                        if let Ok(path) = self.resolved_library_path(name) {
                            library_paths.push(path);
                        }
                    }
                }
            }
            reserved_sources += 1;
            reserved_bytes += byte_len;
            // A config library can also be an explicit root. Keep it in
            // the library prefix, with its root ordinal for eviction, and
            // remove its duplicate root job below.
            let order = if seen.contains(path.canonical()) {
                roots
                    .iter()
                    .find(|(_, root)| root.canonical() == path.canonical())
                    .map_or(usize::MAX, |(order, _)| *order)
            } else {
                usize::MAX
            };
            library_entries.push(Entry {
                path,
                byte_len,
                text,
                order,
            });
        }
        if library_entries.is_empty() {
            reserved_sources = initial_reserved_sources;
            reserved_bytes = initial_reserved_bytes;
        }
        if let (Some(catalog), Some(directory)) =
            (self.library_catalog, self.library_directory.as_ref())
        {
            library_entries
                .sort_by_key(|entry| catalog.source_file_priority(&entry.path, directory));
        }
        let library_members: HashSet<_> = library_entries
            .iter()
            .map(|entry| entry.path.canonical().clone())
            .collect();
        roots.retain(|(_, path)| !library_members.contains(path.canonical()));
        tsc_types::trace::mark("load: config library closure", library_started);
        let lease_started = std::time::Instant::now();
        let width = direct::stripe_width(library_entries.len());
        let groups: Vec<Vec<Entry>> = {
            let mut entries = library_entries.into_iter();
            std::iter::from_fn(|| {
                let group: Vec<_> = entries.by_ref().take(width).collect();
                (!group.is_empty()).then_some(group)
            })
            .collect()
        };
        let bounds: Vec<_> = groups.iter().map(|group| stripe_bounds(group)).collect();
        let rooms = identity_domain
            .lease_rooms(&bounds)
            .expect("config library stripe leases");
        let library_stripes = rooms.len();
        tsc_types::trace::mark("load: config library stripe leases", lease_started);
        let pass = ParallelReadAhead {
            reader,
            options,
            program_options,
            content_mappers: program_options
                .content_mapper_spec()
                .zip(program_options.content_mapper_project())
                .map(|(spec, project)| {
                    (&**spec, &*project.0 as &dyn crate::content_mapper::Project)
                }),
            library_catalog: self.library_catalog,
            library_directory: self.library_directory.clone(),
            references: ReferenceView {
                program_options,
                host,
                case_sensitive,
            },
            case_sensitive,
            max_source_file_bytes: self.limits.max_source_file_bytes,
            source_budget: self
                .limits
                .max_source_files
                .saturating_sub(self.sources.len() + self.reserved_sources),
            byte_budget: self
                .limits
                .max_total_source_bytes
                .saturating_sub(self.total_source_bytes + self.reserved_bytes),
            retained_sources: AtomicUsize::new(
                reserved_sources - self.sources.len() - self.reserved_sources,
            ),
            retained_bytes: AtomicUsize::new(
                reserved_bytes - self.total_source_bytes - self.reserved_bytes,
            ),
            stopped: AtomicBool::new(false),
            queued: Sharded::new(),
            resolutions: Sharded::new(),
            target_resolutions: Sharded::new(),
            resolved: AtomicUsize::new(0),
            reused: AtomicUsize::new(0),
            target_reused: AtomicUsize::new(0),
        };
        for path in seeds {
            if pass.admits(path) {
                pass.queued.insert_new(path.canonical());
            }
        }
        let mut placed = HashSet::default();
        let mut jobs = Vec::new();
        for (ordinal, (entries, room)) in groups.into_iter().zip(rooms).enumerate() {
            for entry in &entries {
                placed.insert(entry.path.canonical().clone());
                pass.queued.insert_new(entry.path.canonical());
            }
            jobs.push(Job::Stripe(
                ordinal,
                entries,
                SyntaxStripe::new(room),
                "config libraries",
            ));
        }
        let root_width = direct::stripe_width(roots.len());
        jobs.extend((0..roots.len().div_ceil(root_width)).map(|_| Job::Roots));
        // Every root is already queued before any parse can discover it.
        // Exclusions stay private, with serial mapper callbacks on the walk.
        let mut ordinary_seen = HashSet::default();
        for path in seeds {
            if !seen.contains(path.canonical())
                && !placed.contains(path.canonical())
                && pass.admits(path)
                && ordinary_seen.insert(path.canonical().clone())
            {
                jobs.push(Job::Ordinary(path.clone()));
            }
        }
        let reads = crate::source_reads::SourceReads::new(roots.len());
        let read_order: Vec<_> = (0..roots.len()).collect();
        let window = ReadWindow {
            state: Mutex::new((0, false)),
            changed: std::sync::Condvar::new(),
        };
        let read = |index| {
            window.read(index, || {
                Some(reader.read_source_js(roots[index].1.display()))
            })
        };
        let parse_started = std::time::Instant::now();
        let bind_admission = (options.list_files_only != Some(true))
            .then(|| Mutex::new(BindAdmission::new(identity_domain.clone(), library_stripes)));
        let stream = Mutex::new(RootStream {
            identity_domain: identity_domain.clone(),
            bind_admission: bind_admission.as_ref(),
            next: 0,
            width: root_width,
            ordinal: library_stripes,
            admitted: 0,
            leased: 0,
            reserved_sources,
            reserved_bytes,
            stopped: false,
            complete: roots.is_empty(),
            other_reads: Vec::new(),
            deferred: Vec::new(),
            started: parse_started,
        });
        let finished = std::thread::scope(|scope| {
            let _stop_on_unwind = StopReaders(&window);
            if workers.is_parallel() {
                for _ in 0..3.min(workers.max_workers()).min(roots.len()) {
                    let _ = std::thread::Builder::new()
                        .name("tsc-rs-reader".into())
                        .stack_size(crate::workers::WORKER_STACK_BYTES)
                        .spawn_scoped(scope, || {
                            crate::workers::run_thread_start_hook();
                            reads.prepare(&read_order, &read);
                        });
                }
            }
            workers.map_spawning(
                jobs,
                || ModuleResolver::new_with_program_options(host, options, program_options).ok(),
                |resolver, job, spawned| {
                    let mut next = Vec::new();
                    let mut results = Vec::new();
                    let mut stripe_result = None;
                    let job = if matches!(job, Job::Roots) {
                        let mut stream = stream.lock().expect("ordered root stream");
                        let job = stream.take(&roots, &reads, &read, &window, limits, &pass);
                        if stream.complete {
                            spawned.extend(stream.deferred.drain(..).map(Job::Ordinary));
                        }
                        job?
                    } else {
                        job
                    };
                    match job {
                        Job::Roots => unreachable!("root token replaced by its leased stripe"),
                        Job::Ordinary(path) => {
                            // The serial Program walk already plans and resolves
                            // private sources. Speculative publication only pays
                            // when another worker can overlap that work. Root
                            // follow/resolve still runs before later admission.
                            if !workers.is_parallel() {
                                return None;
                            }
                            {
                                let mut stream = stream.lock().expect("ordered root stream");
                                if !stream.complete {
                                    stream.deferred.push(path);
                                    return None;
                                }
                            }
                            // Mapper callbacks retain today's serial walk and
                            // failure budget. Parallel mapping uses the existing
                            // private read-ahead path; it never owns a room.
                            if workers.is_parallel() || pass.mapper_for(&path).is_none() {
                                if let Some(source) =
                                    pass.read_source(resolver, path, usize::MAX, &mut next)
                                {
                                    results.push(source);
                                }
                            }
                        }
                        Job::Stripe(ordinal, entries, mut stripe, label) => {
                            for entry in entries {
                                let scope = if label == "roots"
                                    && implied_node_format_needs_package_scope(
                                        entry.path.display(),
                                        options,
                                    ) {
                                    resolver
                                        .as_mut()
                                        .and_then(|resolver| {
                                            resolver
                                                .package_scope_for_file(entry.path.display())
                                                .ok()
                                        })
                                        .flatten()
                                } else {
                                    None
                                };
                                let implied = implied_node_format(
                                    entry.path.display(),
                                    scope.as_ref(),
                                    options,
                                );
                                let implied_for_emit = implied_node_format_for_emit(
                                    entry.path.display(),
                                    scope.as_ref(),
                                    options,
                                );
                                let prepared =
                                    PreparedSourceFile::new(entry.path.clone(), entry.text)
                                        .with_implied_node_formats(implied, implied_for_emit);
                                let planned =
                                    crate::module_requests::plan_source_requests_in_stripe(
                                        &prepared,
                                        options,
                                        &mut stripe,
                                    );
                                let parsed = match planned {
                                    Ok((plan, syntax)) => PrefetchedParse {
                                        implied,
                                        implied_for_emit,
                                        prepared: prepared.with_preparsed_syntax(syntax),
                                        plan: Ok(plan),
                                        resolutions: Vec::new(),
                                    },
                                    Err(error) => PrefetchedParse {
                                        implied,
                                        implied_for_emit,
                                        prepared,
                                        plan: Err(error),
                                        resolutions: Vec::new(),
                                    },
                                };
                                let mut source = ReadAheadSource {
                                    path: entry.path,
                                    order: entry.order,
                                    read: PrefetchedRead::Parsed {
                                        byte_len: entry.byte_len,
                                        decoded: Ok(Box::new(parsed)),
                                    },
                                    type_resolutions: Vec::new(),
                                };
                                if let Some(resolver) = resolver.as_mut() {
                                    pass.follow(resolver, &mut source, &mut next);
                                }
                                results.push(source);
                            }
                            if label == "roots" && ordinal == library_stripes {
                                tsc_types::trace::mark(
                                    "load: first root stripe parse/plan/resolve complete",
                                    parse_started,
                                );
                            }
                            stripe_result = Some((ordinal, label, stripe));
                        }
                    }
                    spawned.extend(next.into_iter().map(|(path, _)| Job::Ordinary(path)));
                    if let Some((ordinal, label, syntax)) = stripe_result {
                        let Some(admission) = bind_admission.as_ref() else {
                            return Some((results, vec![(ordinal, label, syntax)]));
                        };
                        let completed = admission.lock().expect("ordered bind admission").complete(
                            CompletedStripe {
                                ordinal,
                                label,
                                sources: results,
                                syntax,
                            },
                        );
                        let mut completed = completed.into_iter();
                        let first = completed.next()?;
                        let mut sources = first.sources;
                        let mut stripes = vec![(first.ordinal, first.label, first.syntax)];
                        for stripe in completed {
                            sources.extend(stripe.sources);
                            stripes.push((stripe.ordinal, stripe.label, stripe.syntax));
                        }
                        Some((sources, stripes))
                    } else {
                        Some((results, Vec::new()))
                    }
                },
                |resolver| {
                    resolver
                        .map(|mut resolver| resolver.take_package_json_probes())
                        .unwrap_or_default()
                },
            )
        });
        if let Some(admission) = bind_admission.as_ref() {
            admission.lock().expect("ordered bind admission").finish();
        }
        drop(reads);
        let stream = stream.into_inner().expect("ordered root stream");
        if tsc_types::trace::enabled() {
            eprintln!("[syntax-placement] level 0: config_lease_batches={} root_lease_batches={} leases={} root_prefix={} root_candidates={} stopped={}",
                usize::from(library_stripes != 0), stream.leased,
                (library_stripes + stream.leased) * direct::SPACES.len(), stream.admitted, roots.len(), stream.stopped);
        }
        tsc_types::trace::mark("load: streaming stripe parse/plan/resolve", parse_started);
        let mut sources = Vec::new();
        for (probes, results) in finished {
            self.read_ahead_package_json_probes.extend(probes);
            for (group, stripes) in results {
                sources.extend(group);
                self.direct_stripes.0.extend(stripes);
            }
        }
        // map_spawning's finish order is a scheduling detail. Stripe owners
        // must be sealed in their lease order, never worker completion order.
        self.direct_stripes
            .0
            .sort_by_key(|(ordinal, _, _)| *ordinal);
        sources.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.path.canonical().cmp(right.path.canonical()))
        });
        for (path, order, read) in stream.other_reads {
            sources.push(ReadAheadSource {
                path,
                order,
                read,
                type_resolutions: Vec::new(),
            });
        }
        sources.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.path.canonical().cmp(right.path.canonical()))
        });
        let count = sources.len();
        for source in sources {
            if let Some(bytes) = source.read.reserved_bytes() {
                self.reserved_sources += 1;
                self.reserved_bytes += bytes;
            }
            for (key, outcome, trace) in source.type_resolutions {
                self.pre_resolved_types.insert(key, (outcome, trace));
            }
            self.prefetch_order.push(source.path.canonical().clone());
            self.prefetched.insert(
                source.path.canonical().clone(),
                PrefetchedSource {
                    display: source.path.display().to_owned(),
                    read: source.read,
                },
            );
        }
        self.directory_resolution_hits += pass.reused.load(Ordering::Relaxed);
        self.target_resolution_hits += pass.target_reused.load(Ordering::Relaxed);
        self.directory_resolutions
            .extend(pass.resolutions.into_entries());
        self.target_resolutions
            .extend(pass.target_resolutions.into_entries());
        self.direct_layout = true;
        tsc_types::trace::mark(
            &format!("load: level-0 streaming read-ahead ({count} sources)"),
            phase_started,
        );
        true
    }
}
