use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--print-lib") {
        print!("{}", tsrs::LIB_TEXT);
        return;
    }
    if let Some(pos) = args.iter().position(|a| a == "--parse-batch") {
        // Parse-only conformance harness: read a newline-delimited list of
        // file paths, parse each in isolation, and emit one result line per
        // file: `PATH\x01code:offset,code:offset,...` (empty = clean parse),
        // or `PATH\x01PANIC` / `PATH\x01READERR`. Panics are contained so a
        // single bad file cannot abort the batch.
        let listfile = &args[pos + 1];
        let list = std::fs::read_to_string(listfile).expect("read list file");
        let mut out = String::new();
        for path in list.lines() {
            let path = path.trim();
            if path.is_empty() {
                continue;
            }
            let text = match std::fs::read_to_string(path) {
                Ok(t) => t,
                Err(_) => {
                    out.push_str(path);
                    out.push('\u{1}');
                    out.push_str("READERR\n");
                    continue;
                }
            };
            let jsx = path.ends_with(".tsx");
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                tsrs::parser::parse_with_jsx(&text, 0, jsx)
            }));
            out.push_str(path);
            out.push('\u{1}');
            match res {
                Ok((_ast, diags)) => {
                    let codes: Vec<String> = diags
                        .iter()
                        .map(|d| format!("{}:{}", d.message.code, d.start))
                        .collect();
                    out.push_str(&codes.join(","));
                }
                Err(_) => out.push_str("PANIC"),
            }
            out.push('\n');
        }
        print!("{}", out);
        return;
    }
    if let Some(pos) = args.iter().position(|a| a == "--check-batch") {
        // Full-check conformance harness: read a newline-delimited list of file
        // paths, run the whole pipeline (parse + bind + check) on each in
        // isolation under `--strict`, and emit one result line per file:
        // `PATH\x01<diag-json>` (the JSON diagnostics envelope), or
        // `PATH\x01PANIC` / `PATH\x01READERR`.
        //
        // Each file is an independent program (its own bind / types / diagnostics
        // — nothing is shared between files), so the checks are parallelised over
        // a work-stealing thread pool via `std::thread::scope` (zero-dependency;
        // no rayon). Results are buffered per file and emitted in input-list order
        // afterward, so the byte stream is identical to a sequential run (the
        // conformance differ compares it line-for-line). Panics are contained per
        // file; a separate per-file timeout scan handles hangs / non-unwinding
        // aborts (those are not attributable here once output is buffered).
        let listfile = &args[pos + 1];
        let list = std::fs::read_to_string(listfile).expect("read list file");
        let paths: Vec<&str> = list
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect();
        let mut base_opts = tsrs::options::CompilerOptions::default();
        base_opts.strict = Some(true);
        base_opts.diag_json = true;
        let base_opts_ref = &base_opts;

        // worker count: `--jobs N` overrides; else available parallelism, capped
        // at the file count.
        let jobs = args
            .iter()
            .position(|a| a == "--jobs")
            .and_then(|p| args.get(p + 1))
            .and_then(|n| n.parse::<usize>().ok())
            .or_else(|| std::thread::available_parallelism().ok().map(|n| n.get()))
            .unwrap_or(1)
            .max(1)
            .min(paths.len().max(1));

        let results: Vec<std::sync::Mutex<Option<String>>> = (0..paths.len())
            .map(|_| std::sync::Mutex::new(None))
            .collect();
        let next = std::sync::atomic::AtomicUsize::new(0);
        let paths_ref = &paths;
        let results_ref = &results;
        let next_ref = &next;

        std::thread::scope(|s| {
            for _ in 0..jobs {
                s.spawn(|| loop {
                    let i = next_ref.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if i >= paths_ref.len() {
                        break;
                    }
                    let path = paths_ref[i];
                    // dark-launch triage: emit a file marker so verbose
                    // FLOW_VERIFY mismatch lines can be attributed (run with
                    // --jobs 1 for deterministic interleaving)
                    if std::env::var("TSRS_FLOW_VERIFY").is_ok_and(|v| v == "v" || v == "verbose")
                    {
                        eprintln!("FLOW_VERIFY file {}", path);
                    }
                    let line = match std::fs::read_to_string(path) {
                        Err(_) => "READERR".to_string(),
                        Ok(text) => {
                            let res =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    match tsrs::harness::parse_fixture(&text) {
                                        Ok(mut fixture) => {
                                            if path.ends_with(".tsx")
                                                && fixture.files.len() == 1
                                                && fixture.files[0].0 == "main.ts"
                                            {
                                                fixture.files[0].0 = "main.tsx".to_string();
                                            }
                                            let mut opts = base_opts_ref.clone();
                                            tsrs::harness::apply_directives(
                                                &mut opts,
                                                &fixture.options,
                                            )
                                            .expect("apply harness directives");
                                            opts.diag_json = true;
                                            let inputs: Vec<tsrs::InputFile> = fixture
                                                .files
                                                .into_iter()
                                                .map(|(name, text)| tsrs::InputFile { name, text })
                                                .collect();
                                            tsrs::check_program_with_roots(
                                                inputs,
                                                &fixture.extra_root_files,
                                                &opts,
                                            )
                                        }
                                        Err(_) => {
                                            let name = if path.ends_with(".tsx") {
                                                "main.tsx"
                                            } else {
                                                "main.ts"
                                            };
                                            let inputs = vec![tsrs::InputFile {
                                                name: name.to_string(),
                                                text: text.clone(),
                                            }];
                                            tsrs::check_program(inputs, base_opts_ref)
                                        }
                                    }
                                }));
                            match res {
                                Ok((json, _code)) => json.replace('\n', " "),
                                Err(_) => "PANIC".to_string(),
                            }
                        }
                    };
                    *results_ref[i].lock().unwrap() = Some(line);
                });
            }
        });

        // emit in input-list order: `PATH\x01<line>\n`, byte-identical to sequential.
        let stdout = std::io::stdout();
        let mut h = stdout.lock();
        for (i, path) in paths.iter().enumerate() {
            let line = results[i].lock().unwrap().take().unwrap_or_default();
            let _ = h.write_all(path.as_bytes());
            let _ = h.write_all(b"\x01");
            let _ = h.write_all(line.as_bytes());
            let _ = h.write_all(b"\n");
        }
        // Tier-2 dark launch: report the flow-resolver vs fact-stack agreement
        // tally (stderr only; stdout stays byte-identical).
        if std::env::var("TSRS_FLOW_VERIFY").is_ok_and(|v| !v.is_empty() && v != "0") {
            use std::sync::atomic::Ordering;
            use tsrs::checker::flow::resolver as fv;
            eprintln!(
                "TSRS_FLOW_VERIFY: match={} mismatch={} unresolved={} no_flow_node={}",
                fv::FLOW_VERIFY_MATCH.load(Ordering::Relaxed),
                fv::FLOW_VERIFY_MISMATCH.load(Ordering::Relaxed),
                fv::FLOW_VERIFY_UNRESOLVED.load(Ordering::Relaxed),
                fv::FLOW_VERIFY_NO_NODE.load(Ordering::Relaxed),
            );
        }
        return;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!(
            "tsrs 0.1.0 (diagnostics catalog: {} entries)",
            tsrs::diagnostics::gen::ALL_BY_CODE.len()
        );
        return;
    }
    if args.is_empty() {
        eprintln!("usage: tsrs [--strict] [--noUnusedLocals] [...tsc-style flags] <files...>");
        std::process::exit(2);
    }
    let cwd = std::env::current_dir().expect("cwd");
    // harness knob: pins the directory string embedded in resolved-path
    // diagnostics so blessed baselines stay machine-portable
    let cwd_display =
        std::env::var("TSRS_VIRTUAL_CWD").unwrap_or_else(|_| cwd.to_string_lossy().into_owned());
    let read = |name: &str| {
        let path = if std::path::Path::new(name).is_absolute() {
            std::path::PathBuf::from(name)
        } else {
            cwd.join(name)
        };
        std::fs::read_to_string(&path).ok()
    };
    let (out, code) = tsrs::run_command_line(&args, read, &cwd_display);
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(out.as_bytes()).expect("write stdout");
    stdout.flush().ok();
    std::process::exit(code);
}
