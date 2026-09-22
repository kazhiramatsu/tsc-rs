//! Supplemental project-loader qualification. This never changes or relaxes
//! the original parser census's selection, source pins or failure records.
use super::*;

pub(crate) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let roster_path = PathBuf::from(args.next().ok_or("missing project roster")?);
    let output = PathBuf::from(args.next().ok_or("missing project output")?);
    let input_workspace = fs::canonicalize(args.next().ok_or("missing input workspace")?)?;
    require(
        args.next().is_none(),
        "unexpected project supplement argument",
    )?;
    require(
        !output.exists(),
        "refusing to overwrite project observations",
    )?;
    let workspace = find_workspace_root()?;
    let head = clean(&workspace)?;
    let input_head = clean(&input_workspace)?;
    let roster_bytes = fs::read(&roster_path)?;
    let roster: Value = serde_json::from_slice(&roster_bytes)?;
    require(
        roster["schema"] == 1 && roster["kind"] == "emitter-project-projection-roster",
        "invalid project roster",
    )?;
    let ids = roster["case_ids"]
        .as_array()
        .ok_or("missing project case IDs")?;
    require(!ids.is_empty(), "empty project roster")?;
    let library_root = fs::canonicalize(input_workspace.join("vendor/typescript-6.0.3/lib"))?;
    let compiler_sha256 = sha256(fs::read(library_root.join("typescript.js"))?);
    let manifest_path = tsc_harness::upstream_suites::MANIFEST_RELATIVE_PATH;
    let plan_manifest_sha256 = sha256(fs::read(input_workspace.join(manifest_path))?);
    let vendor_tree_hash = git(
        &input_workspace,
        &["rev-parse", "HEAD:vendor/typescript-6.0.3"],
    )?;
    require(
        vendor_tree_hash == git(&workspace, &["rev-parse", "HEAD:vendor/typescript-6.0.3"])?,
        "project library/data trees differ",
    )?;
    let corpus = load_recorded_execution_plans(&input_workspace)?;
    let plans = index_plans(&corpus.plans)?;
    let mut seen = BTreeSet::new();
    let mut pool = BTreeMap::new();
    let mut rows = Vec::new();
    for id in ids {
        let id = id.as_str().ok_or("project ID is not text")?;
        require(seen.insert(id), format!("duplicate project ID {id}"))?;
        let matching: Vec<_> = corpus
            .plans
            .iter()
            .filter(|plan| plan.provenance.case_id.as_ref() == id)
            .collect();
        require(
            matching.len() == 1,
            format!("project roster ID does not resolve uniquely: {id}"),
        )?;
        let recorded = matching[0];
        let UpstreamExecutionInput::Project(plan) = &recorded.input else {
            return Err(format!("project roster contains non-project ID {id}").into());
        };
        let program = match load_project_emit(&input_workspace, plan, observable_input::limits()) {
            Ok(loaded) => loaded.prepared_program,
            Err(error) => {
                let first = error.to_string();
                let second = load_project_emit(&input_workspace, plan, observable_input::limits())
                    .err()
                    .ok_or("project load refusal changed on repetition")?
                    .to_string();
                require(
                    first == second,
                    format!("project load refusal changed: {id}"),
                )?;
                rows.push(json!({"case_id":id,"disposition":"not-loaded; emit-not-qualified","load_error":first}));
                eprintln!(
                    "project supplement {}/{} {id}: load refusal",
                    rows.len(),
                    ids.len()
                );
                continue;
            }
        };
        let input = observable_input::plan_input(&recorded.input, &program, &mut |bytes| {
            let hash = sha256(bytes);
            pool.insert(hash.clone(), bytes.to_vec());
            hash
        });
        let row = json!({"case_id":id,"universe":"recorded-execution-plans","loader":"load_project_emit",
            "command_input":input,"emit_load_error":null,"emit_disposition":"pending-complete-command-comparison"});
        let option_snapshot = options::snapshot(&program);
        let first = observe(program);
        // Reuse the census consumer's byte-for-byte input and document checks.
        let repeat = prepare(&input_workspace, &row, &plans, &pool)?;
        require(
            option_snapshot == options::snapshot(&repeat),
            format!("project options changed: {id}"),
        )?;
        let second = observe(repeat);
        require(
            first == second,
            format!("project command repetition differs: {id}"),
        )?;
        let input_sha256 = sha256(serde_json::to_vec(&canonical_input(
            &row["command_input"],
        )?)?);
        rows.push(json!({"case_id":id,"disposition":"observed-twice; pending-typescript-comparison",
            "input_sha256":input_sha256,"row":row,"options":option_snapshot,"complete_command_runs":[first,second]}));
        eprintln!(
            "project supplement {}/{} {id}: observed twice",
            rows.len(),
            ids.len()
        );
    }
    require(
        clean(&workspace)? == head && clean(&input_workspace)? == input_head,
        "project source/data HEAD changed",
    )?;
    require(
        sha256(fs::read(library_root.join("typescript.js"))?) == compiler_sha256,
        "project TS library changed",
    )?;
    require(
        sha256(fs::read(input_workspace.join(manifest_path))?) == plan_manifest_sha256,
        "project plan manifest changed",
    )?;
    let dependencies: BTreeMap<_, _> = [
        "crates/harness/src/upstream_suites/execution/project.rs",
        "crates/harness/src/upstream_suites/execution/observable_input.rs",
        "crates/xtask/src/recovery_corpus_native.rs",
        "crates/xtask/src/recovery_corpus_native/options.rs",
        "crates/xtask/src/recovery_corpus_native/project.rs",
    ]
    .into_iter()
    .map(|path| Ok((path, sha256(fs::read(workspace.join(path))?))))
    .collect::<Result<_>>()?;
    let documents: BTreeMap<_, _> = pool
        .into_iter()
        .map(|(hash, bytes)| (hash, STANDARD.encode(bytes)))
        .collect();
    let result = json!({"schema":1,"kind":"emitter-project-projection-native","head":head,
        "input_head":input_head,"input_workspace":input_workspace,"library_root":library_root,
        "roster_sha256":sha256(&roster_bytes),"case_ids":ids,"plan_manifest_sha256":plan_manifest_sha256,
        "compiler_sha256":compiler_sha256,"vendor_tree_hash":vendor_tree_hash,"dependencies":dependencies,
        "repetitions":2,"documents":documents,"cases":rows});
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    serde_json::to_writer(&mut file, &result)?;
    file.write_all(b"\n")?;
    Ok(())
}
