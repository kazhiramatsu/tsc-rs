//! Explicit local measurement, never an acceptance/adoption path.
use super::*;
use std::io::Write;

const INVENTORY_SHA: &str = "0a0dc275dc75a4730717ab17dcb0981b96b4ec1d56eb5d72ee8fa2d5d6f32196";

fn pinned(path: &Path, expected: &str) -> Result<Value, Box<dyn Error>> {
    let bytes = fs::read(path)?;
    if sha256(&bytes) != expected {
        return Err(failure(format!(
            "collector input hash changed: {}",
            path.display()
        )));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn chain(value: &MessageChain) -> Value {
    json!({"code":value.code,"category":diagnostic_category(value.category),
        "text":value.text,"next_present":value.next_present,
        "next":value.next.iter().map(chain).collect::<Vec<_>>()})
}

fn diagnostics(values: &[Diagnostic]) -> Value {
    Value::Array(values.iter().map(|value| json!({
        "file":value.file_name,"start":value.start,"length":value.length,
        "message":chain(&value.message),
        "related_information_present":value.related_information_present,
        "related":value.related.iter().map(|related| json!({
            "file":related.file_name,"start":related.start,"length":related.length,
            "message":chain(&related.message)
        })).collect::<Vec<_>>(),
        "canonical_head":value.canonical_head.as_ref().map(|head|json!({"code":head.code,"text":head.text})),
        "reports_unnecessary":value.reports_unnecessary,"reports_deprecated":value.reports_deprecated,
        "source":value.source,"skipped_on_no_emit":value.skipped_on_no_emit
    })).collect())
}

fn writes(sink: &MemoryOutputSink) -> Value {
    Value::Array(sink.writes().iter().enumerate().map(|(index, artifact)| {
        let metadata = match artifact.metadata() {
            None => Value::Null,
            Some(EmitWriteMetadata::Text(text)) => json!({
                "kind":"text","diagnostics":diagnostics(text.diagnostics()),
                "source_map_url_position":text.source_map_url_position().map(|p|p.value())
            }),
            Some(EmitWriteMetadata::BuildInfo(info)) => json!({
                "kind":"build-info","schema_version":info.schema_version(),
                "canonical_json":info.canonical_json()
            }),
        };
        json!({"index":index,"path":artifact.path(),"kind":format!("{:?}",artifact.kind()),
            "callback_utf8_base64":base64::engine::general_purpose::STANDARD.encode(artifact.callback_bytes()),
            "callback_utf8_sha256":sha256(artifact.callback_bytes()),
            "callback_utf8_bytes":artifact.callback_bytes().len(),
            "write_byte_order_mark":artifact.write_byte_order_mark(),
            "materialized_utf8_base64":base64::engine::general_purpose::STANDARD.encode(artifact.materialized_bytes()),
            "materialized_utf8_sha256":sha256(artifact.materialized_bytes()),
            "materialized_utf8_bytes":artifact.materialized_bytes().len(),
            "source_files":artifact.source_files(),"metadata":metadata})
    }).collect())
}

fn vector(value: &H2VectorDivergence) -> Value {
    json!({"writes_diverging":value.writes_diverging,
        "diagnostics_diverging":value.diagnostics_diverging,
        "emit_result_diverging":value.emit_result_diverging,"emit_refused":value.emit_refused,
        "refused_option":value.refused_option,"mismatch_vector":value.mismatch_vector,
        "facet_fingerprint_sha256":value.facet_fingerprint_sha256})
}

fn prepared_identity(program: &PreparedProgram) -> Value {
    let options = program.compiler_options();
    json!({
        "mode":format!("{:?}",program.mode()),"current_directory":program.current_directory().display(),
        "case_sensitive":program.path_context().use_case_sensitive_file_names(),
        // Exhaustive typed Debug snapshots supplement the unchanged frozen input;
        // they are observations, not a new option projector or portable oracle.
        "compiler_options_debug":format!("{options:?}"),
        "program_options_debug":format!("{:?}",program.program_options()),
        "option_facets":{"outFile":options.out_file,"outDir":options.out_dir,
            "rootDir":options.root_dir,"importHelpers":options.import_helpers,
            "declaration":options.declaration,"declarationMap":options.declaration_map,
            "emitDeclarationOnly":options.emit_declaration_only,"noEmit":options.no_emit,
            "noEmitOnError":options.no_emit_on_error},
        "roots":program.roots().iter().map(|root|json!({
            "path":root.path().display(),"source_id":root.source().map(|id|id.raw()),
            "missing_diagnostic":root.missing_diagnostic().map(|d|diagnostics(std::slice::from_ref(d)))
        })).collect::<Vec<_>>(),
        "library_source_ids":program.library_files().iter().map(|id|id.raw()).collect::<Vec<_>>(),
        "source_files":program.source_files().iter().enumerate().map(|(id,source)|json!({
            "source_id":id,"path":source.path().display(),"utf8_sha256":sha256(source.text()),
            "utf8_bytes":source.text().len(),"may_be_emitted":source.may_be_emitted(),
            "may_emit_forced_declaration":source.may_emit_forced_declaration(),
            "is_external_module":source.is_external_module()
        })).collect::<Vec<_>>(),
        "auxiliary_files":program.auxiliary_files().map(|source|json!({
            "path":source.path().display(),"utf8_sha256":sha256(source.text()),"utf8_bytes":source.text().len()
        })).collect::<Vec<_>>()
    })
}

type RawResult = Result<(EmitOutcome, Vec<Diagnostic>), DriverError>;

fn observation(expected: &Value, result: &RawResult, sink: &MemoryOutputSink) -> Value {
    let mut record = json!({"writes":writes(sink),"write_count":sink.writes().len(),
        "activity":null,"emit_result":null,"reported_diagnostics":null,
        "old_comparison":null,"comparison_error":null,"old_command_projection":null});
    match result {
        Ok((outcome, reported)) => {
            record["result_kind"] = json!("success");
            let counters = outcome.h2_activity();
            record["activity"] = json!({
                "B":counters.runtime_slice(H2RuntimeSlice::H2_7b),
                "C":counters.runtime_slice(H2RuntimeSlice::H2_7c),
                "D":counters.runtime_slice(H2RuntimeSlice::H2_7d),
                "E":counters.runtime_slice(H2RuntimeSlice::H2_7e),
                "all_slices":H2RuntimeSlice::ALL.into_iter().map(|slice|
                    (slice.name().to_owned(),counters.runtime_slice(slice))).collect::<BTreeMap<_,_>>(),
                "all_counters_debug":format!("{counters:?}")
            });
            record["emit_result"] = json!({"emit_skipped":outcome.emit_skipped(),
                "diagnostics":diagnostics(outcome.diagnostics()),
                "emitted_files":emitted_files_value(outcome.emitted_files()),
                "source_maps":source_maps_value(outcome.source_maps())});
            record["reported_diagnostics"] = diagnostics(reported);
            // Exactly the legacy comparator's projection; not a CLI subprocess.
            record["old_command_projection"] = json!({"status_writes":[],
                "exit_code":if outcome.emit_skipped() && !reported.is_empty(){1}else if !reported.is_empty(){2}else{0}});
            match vectorize_successful_observation(
                H2MismatchProfile::H2_6c,
                expected,
                sink,
                outcome,
                reported,
                &expected["emit_result"]["emitted_files"],
            ) {
                Ok(divergence) => {
                    record["comparison_kind"] = json!(if divergence.is_exact() {
                        "exact"
                    } else {
                        "mismatch"
                    });
                    record["old_comparison"] = vector(&divergence);
                }
                Err(error) => {
                    record["comparison_kind"] = json!("comparison-error");
                    record["comparison_error"] = json!(error.to_string());
                }
            }
        }
        Err(error) => {
            record["result_kind"] = json!("error");
            record["error"] = json!({"debug":format!("{error:?}"),"display":error.to_string()});
            record["activity_unavailable_reason"] =
                json!("the unchanged consuming Program API returns no EmitOutcome on Err");
            record["partial_writes"] = json!(!sink.writes().is_empty());
            if let DriverError::Emit(EmitFailure::UnsupportedCompilerOption { option }) = error {
                record["error"]["kind"] =
                    json!("DriverError::Emit(EmitFailure::UnsupportedCompilerOption)");
                record["error"]["option"] = json!(option);
                record["comparison_kind"] = json!("typed-refusal");
                record["old_comparison"] =
                    vector(&vectorize_refusal(H2MismatchProfile::H2_6c, option));
                // The old refusal vector encodes the typed error only. Partial
                // writes remain a separate fatal acceptance condition, if any.
                record["old_refusal_zero_writes_guard_passes"] = json!(sink.writes().is_empty());
            } else {
                record["comparison_kind"] = json!("unrepresented-error");
            }
        }
    }
    record
}

fn collect_case(
    workspace: &Path,
    case: &Value,
    inputs: &H2_6cExecutionInputs,
) -> Result<Value, Box<dyn Error>> {
    let expected = compact_typescript_observation(case)?;
    // Same preparation, Program clone, scoped lib reuse, public emit calls and
    // mismatch helper as execute_h2_6c_case. Its acceptance guards stay intact.
    let first_program = prepare_h2_6c_case(workspace, case, inputs)?;
    let prepared = prepared_identity(&first_program);
    let second_program = first_program.clone();
    let first_session = ProgramSession::new(first_program);
    let bundle = first_session.prepare_harness_lib_bundle()?;
    let mut first_sink = MemoryOutputSink::new();
    let first = first_session.emit_with_reported_diagnostics_for_harness_with_lib_bundle(
        &mut first_sink,
        bundle.as_ref(),
    );
    let mut second_sink = MemoryOutputSink::new();
    let second = ProgramSession::new(second_program)
        .emit_with_reported_diagnostics_for_harness_with_lib_bundle(
            &mut second_sink,
            bundle.as_ref(),
        );
    let first_observation = observation(expected, &first, &first_sink);
    let second_observation = observation(expected, &second, &second_sink);
    let stable =
        first == second && first_sink == second_sink && first_observation == second_observation;
    Ok(
        json!({"prepared":prepared,"native_and_json_repetitions_equal":stable,
        "observations":[first_observation,second_observation]}),
    )
}

#[test]
#[ignore = "explicit local legacy177 measurement; no acceptance or adoption"]
fn collect_h2_6c_de_legacy177() -> Result<(), Box<dyn Error>> {
    let started = std::time::Instant::now();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let inventory_path = PathBuf::from(
        std::env::var_os("TSRS_H2_6C_DE_LEGACY_INVENTORY").ok_or_else(|| {
            failure("set TSRS_H2_6C_DE_LEGACY_INVENTORY to the pinned 177-ID inventory")
        })?,
    );
    let output_path = PathBuf::from(
        std::env::var_os("TSRS_H2_6C_DE_COLLECTOR_OUTPUT").ok_or_else(|| {
            failure("set TSRS_H2_6C_DE_COLLECTOR_OUTPUT to a new local target JSON path")
        })?,
    );
    if !output_path.is_absolute()
        || !output_path
            .components()
            .any(|part| part.as_os_str() == "target")
    {
        return Err(failure(
            "collector output must be an absolute path under a target directory",
        ));
    }
    let inventory = pinned(&inventory_path, INVENTORY_SHA)?;
    let census = pinned(
        &workspace.join("ratchets/h2-7de-candidates.v1.json"),
        "1af6d75acf8212135a0850c5ff09487a5589de4d0f825ff1f0e9bc8e3f0f141d",
    )?;
    // These joins verify provenance only. Neither new input nor new success is
    // used to prepare or judge the old Program.
    drop(pinned(
        &workspace.join("ratchets/h2-7de-candidate-inputs.v1.json"),
        "f2e078a6b6d10cd3c6df833584924c18e8f78fe98c1e41621c70e10e215a073a",
    )?);
    drop(pinned(
        &workspace.join("ratchets/h2-7de-observations.v1.json"),
        "1a1681b2375d27d9012b06e29808aca72aa3e39d1dbc1536b80ba2aadf9e8ce2",
    )?);
    let qualification_bytes = fs::read(workspace.join(H2_6C_QUALIFICATION_RELATIVE_PATH))?;
    let qualification: Value = serde_json::from_slice(&qualification_bytes)?;
    let cases = validate_h2_6c_qualification(&qualification)?;
    let old_by_id = cases
        .iter()
        .map(|case| Ok((string(case, "case_id")?, case)))
        .collect::<Result<BTreeMap<_, _>, Box<dyn Error>>>()?;
    let new_by_id = array(&census, "cases")?
        .iter()
        .map(|case| Ok((string(case, "case_id")?, case)))
        .collect::<Result<BTreeMap<_, _>, Box<dyn Error>>>()?;
    let rows = array(&inventory, "cases")?;
    let ids = rows
        .iter()
        .map(|row| string(row, "case_id"))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let overlap = old_by_id
        .keys()
        .copied()
        .filter(|id| new_by_id.contains_key(id))
        .collect::<BTreeSet<_>>();
    if rows.len() != 177 || ids.len() != 177 || ids != overlap || inventory["adopted_rows"] != 0 {
        return Err(failure(
            "collector must join the complete unchanged legacy177 inventory",
        ));
    }
    let listed = load_h2_6c_divergence_manifest_state(&workspace, false)?;
    let mut eligible = 0usize;
    for row in rows {
        let id = string(row, "case_id")?;
        let old = old_by_id[id];
        let new = new_by_id[id];
        let previous = listed
            .entries
            .get(id)
            .ok_or_else(|| failure(format!("{id}: missing old vector")))?;
        if old["case_fingerprint_sha256"] != row["old_case_sha256"]
            || old["observation_input_sha256"] != row["old_input_sha256"]
            || new["input_sha256"] != row["new_input_sha256"]
            || old["source"] != new["source"]
            || old["disposition"] != "admitted-for-execution"
            || new["required_slices"] != row["remaining_slices"]
            || previous
                != &vectorize_refusal(H2MismatchProfile::H2_6c, string(row, "old_refused_option")?)
            || previous.facet_fingerprint_sha256.as_deref()
                != Some(string(row, "old_refusal_vector_sha256")?)
        {
            return Err(failure(format!(
                "{id}: original identity or old typed vector changed"
            )));
        }
        eligible += usize::from(row["eligible_for_exact_promotion_review"] == true);
    }
    if eligible != 160 {
        return Err(failure("legacy177 must remain eligible160/later17"));
    }
    let inputs = H2_6cExecutionInputs::load(&workspace)?;
    let mut output = std::io::BufWriter::new(
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output_path)?,
    );
    // Stream each whole case to retain progress if a later process aborts.
    // The document is valid JSON only once the final summary has been written.
    write!(output,"{{\"kind\":\"h2-6c-de-legacy177-local-measurement\",\"registered_rows\":0,\"qualification_sha256\":{},\"inventory_sha256\":{},\"cases\":[",
        json!(sha256(&qualification_bytes)),json!(INVENTORY_SHA))?;
    let mut totals = BTreeMap::<String, u64>::new();
    let mut owner_totals = BTreeMap::<String, BTreeMap<String, u64>>::new();
    let mut typed_options = BTreeMap::<String, u64>::new();
    let mut success_activity = BTreeMap::<String, u64>::new();
    let mut activity_unavailable_ids = Vec::new();
    let mut unstable = Vec::new();
    let mut setup_failures = Vec::new();
    let mut comparison_failures = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let id = string(row, "case_id")?;
        let old = old_by_id[id];
        let mut record = json!({"case_id":id,"identity":row,"source":old["source"],
            "old_input":old["input"],"old_files":old["files"],
            "old_typescript_run_fingerprints":old["typescript_run_fingerprints"],
            "old_expected_tuple_sha256":sha256(serde_json::to_vec(compact_typescript_observation(old)?)?),
            "old_h2_7b_expected_members":inputs.h2_7b_expected_members.get(id).copied().unwrap_or(0),
            "retained_historical_vector":vector(&listed.entries[id])});
        let case_started = std::time::Instant::now();
        match collect_case(&workspace, old, &inputs) {
            Ok(measured) => {
                if measured["native_and_json_repetitions_equal"] != true {
                    unstable.push(id.to_owned());
                }
                let kind = measured["observations"][0]["comparison_kind"]
                    .as_str()
                    .unwrap_or("unknown");
                *totals.entry(kind.to_owned()).or_default() += 1;
                if kind == "comparison-error" {
                    comparison_failures.push(id.to_owned());
                }
                let owner_group = if row["eligible_for_exact_promotion_review"] == true {
                    "eligible160"
                } else {
                    "later17"
                };
                *owner_totals
                    .entry(owner_group.to_owned())
                    .or_default()
                    .entry(kind.to_owned())
                    .or_default() += 1;
                if let Some(option) = measured["observations"][0]["error"]["option"].as_str() {
                    *typed_options.entry(option.to_owned()).or_default() += 1;
                }
                if measured["observations"][0]["activity"].is_null() {
                    activity_unavailable_ids.push(id.to_owned());
                } else {
                    for name in ["B", "C", "D", "E"] {
                        *success_activity.entry(name.to_owned()).or_default() += measured
                            ["observations"][0]["activity"][name]
                            .as_u64()
                            .expect("actual success counter");
                    }
                }
                record["measurement"] = measured;
            }
            Err(error) => {
                setup_failures.push(id.to_owned());
                record["measurement"] = json!({"kind":"setup-error","message":error.to_string(),
                    "observations":[],"native_and_json_repetitions_equal":null});
                *totals.entry("setup-error".to_owned()).or_default() += 1;
            }
        }
        record["execution_seconds"] = json!(case_started.elapsed().as_secs_f64());
        if index != 0 {
            write!(output, ",")?;
        }
        serde_json::to_writer(&mut output, &record)?;
        output.flush()?;
        eprintln!("legacy collector {}/177: {id}", index + 1);
    }
    let summary = json!({"candidate_ids":177,"eligible_ids":160,"later_owner_ids":17,"collector_seconds":started.elapsed().as_secs_f64(),
        "repetitions_per_prepared_case":2,"counts_by_first_observation":totals,
        "counts_by_owner_and_first_observation":owner_totals,"typed_option_counts":typed_options,
        "success_activity_totals_first_repetition":success_activity,
        "activity_unavailable_on_error_ids":activity_unavailable_ids,
        "unstable_ids":unstable,"setup_failure_ids":setup_failures,"comparison_failure_ids":comparison_failures,
        "registered_rows":0,"adoption":false,
        "error_activity":"unavailable from the unchanged consuming API; never inferred zero"});
    write!(output, "],\"summary\":{summary}}}\n")?;
    output.flush()?;
    eprintln!(
        "legacy177 measurement: {summary}; saved {}",
        output_path.display()
    );
    if !unstable.is_empty() || !setup_failures.is_empty() || !comparison_failures.is_empty() {
        return Err(failure(
            "collector saved unstable/setup/comparison-failed records; this is not acceptance evidence",
        ));
    }
    Ok(())
}

// Reuse the original old input/host/library-prefix path and complete comparison.
// This regression creates no measurement artifact or admission entry.
#[test]
fn nonbundle_declaration_maps_dispose_javascript_parse_metadata() -> Result<(), Box<dyn Error>> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let qualification = pinned(
        &workspace.join(H2_6C_QUALIFICATION_RELATIVE_PATH),
        "af689ec23311d9cc733f2606e2acfd1d346edbc51bc512bb185c0c3111f1d8ec",
    )?;
    let cases = validate_h2_6c_qualification(&qualification)?;
    let inputs = H2_6cExecutionInputs::load(&workspace)?;
    for target in ["es2015", "es2022", "esnext"] {
        let id = format!(
            "typescript-6.0.3/conformance/esDecorators/classDeclaration/esDecorators-classDeclaration-sourceMap.ts#target%3D{target}"
        );
        let case = cases
            .iter()
            .find(|case| case["case_id"] == id)
            .ok_or_else(|| failure(format!("missing original {id}")))?;
        let observed = collect_case(&workspace, case, &inputs)?;
        assert_eq!(
            observed["prepared"]["option_facets"]["outFile"],
            Value::Null
        );
        assert_eq!(observed["native_and_json_repetitions_equal"], true, "{id}");
        let repetitions = array(&observed, "observations")?;
        assert_eq!(repetitions.len(), 2);
        for run in repetitions {
            assert_eq!(
                run["comparison_kind"], "exact",
                "{id}: {}",
                run["old_comparison"]
            );
            assert_eq!(run["activity"]["D"], 0, "{id}");
            assert_eq!(run["activity"]["E"], 1, "{id}");
        }
    }
    Ok(())
}
