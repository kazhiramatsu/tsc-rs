//! Explicit current refusal observations; historical mismatch vectors stay history.
//! Registered errors are measured twice and remain separate from exact promotions.
use super::*;

struct Migration {
    case_id: &'static str,
    old_case_sha256: &'static str,
    old_input_sha256: &'static str,
    new_input_sha256: &'static str,
    retained_vector_sha256: &'static str,
    current_option: &'static str,
    current_observation_sha256: &'static str,
}

// Completed old-input collector: 56721751cdf2368c3af395a789de1aa1fc36203089030468748ab5e728e4ea3e
static CURRENT: &[Migration] = &[
    Migration {
        case_id: "typescript-6.0.3/compiler/sourceMapWithNonCaseSensitiveFileNames.ts#default",
        old_case_sha256: "f860d9b17947d19fcf944a086191f4d733782b2e281508fdcb434f9a15a06909",
        old_input_sha256: "2ab06b77deb11818a34fbb809f2491a3a9be5f925c8f5955df8a41bbe0653e61",
        new_input_sha256: "a7b13d679a286ca29f6c7513cb15d5e53e877be7e124b12c4652a35204b6f3e5",
        retained_vector_sha256: "dd35039af919e84c4dcc8df94da05b04a38aab98e2f34a76aafcf81c4d87f8ef",
        current_option: "useCaseSensitiveFileNames",
        current_observation_sha256: "3b23b78f3efdd8f2b31e56bd9b6b58a9a35d08378fa775e6460f5ffae5756a9a",
    },
    Migration {
        case_id: "typescript-6.0.3/project/mapRootAbsolutePathMixedSubfolderSpecifyOutputFileAndOutputDirectory.json#module%3Damd",
        old_case_sha256: "b67d3eed1b4e168500c2eea7d91616d576a4b9e7a322cfea9d1e33e983c48bec",
        old_input_sha256: "ae407ad2e6d790e387863a2f0e04776e0c58d07db4d73a867f28b576bd9e8b5f",
        new_input_sha256: "24e12aafd23a23a4c1a01ac27da71fef128c98b9010c01655c7421cb54d02e23",
        retained_vector_sha256: "dd35039af919e84c4dcc8df94da05b04a38aab98e2f34a76aafcf81c4d87f8ef",
        current_option: "outDir",
        current_observation_sha256: "488f4cea543aa160aaae51c312d9663ad5e82a6dba86cb18818e884b2e0d3755",
    },
    Migration {
        case_id: "typescript-6.0.3/project/mapRootAbsolutePathMixedSubfolderSpecifyOutputFileAndOutputDirectory.json#module%3Dcommonjs",
        old_case_sha256: "fd6e5948cedac86d176e9567572cc1d0dffb2dc26c4bdcd29577057d9422e8de",
        old_input_sha256: "c587408c5c3245bb60bce8e6b05d2284264fdee36efb2e4edf478b59a2846c2c",
        new_input_sha256: "0c26c593d05283c812f2125d7c6904d199e576890b4d46293a558ca938b98f37",
        retained_vector_sha256: "dd35039af919e84c4dcc8df94da05b04a38aab98e2f34a76aafcf81c4d87f8ef",
        current_option: "outDir",
        current_observation_sha256: "4e69dae7fbb9211c955c799dd8bc9be2f90b4ad942998d72b6e382d1efa5684e",
    },
    Migration {
        case_id: "typescript-6.0.3/project/mapRootRelativePathMixedSubfolderSpecifyOutputFileAndOutputDirectory.json#module%3Damd",
        old_case_sha256: "caa5d367219a56fd7c6dac8e859c31192080e8627761c4bcf6131c9d4721750d",
        old_input_sha256: "e24cdf4534f687b5d8fd5d42494eb60ca994c08b136ccefe7cdd78e28de0ec65",
        new_input_sha256: "097e83eb9e6bd3ebd15da7f48ba2fbd8d47d1347ed2beda2618a6d35c1d5395e",
        retained_vector_sha256: "dd35039af919e84c4dcc8df94da05b04a38aab98e2f34a76aafcf81c4d87f8ef",
        current_option: "outDir",
        current_observation_sha256: "659a5019e482bbf4d0a7c8c705fff2da0e28fe1ba217df6bd5c9ec981ef96b38",
    },
    Migration {
        case_id: "typescript-6.0.3/project/mapRootRelativePathMixedSubfolderSpecifyOutputFileAndOutputDirectory.json#module%3Dcommonjs",
        old_case_sha256: "caa2d6957a7ee9aee8bb812c0068a5f2cd31e59075257f5c475fdf6c97e83c55",
        old_input_sha256: "7c41fbf13e486799fea755eb7a041287b3bfd6a4e980f0f7a0502d4406aeedee",
        new_input_sha256: "f0858a12e8970e6e66a83f62c1a3f450c2143f709a538fb2750485941e2158f2",
        retained_vector_sha256: "dd35039af919e84c4dcc8df94da05b04a38aab98e2f34a76aafcf81c4d87f8ef",
        current_option: "outDir",
        current_observation_sha256: "96aa308038534497e15bcd560b7458e230f86607d64b9d985ebe3036d803e711",
    },
    Migration {
        case_id: "typescript-6.0.3/project/maprootUrlMixedSubfolderSpecifyOutputFileAndOutputDirectory.json#module%3Damd",
        old_case_sha256: "1a01acfefd7b3f9230833db8cdf196df250f9cbecd1358b7c09d3aef3b1a2403",
        old_input_sha256: "128c6856db6d99ba5ef76e104b32e51110d35ca3af6243b820ec9cc5b44b7574",
        new_input_sha256: "f03f69cb7c673279fe084985d15d23e8e95ac94ecb7ab07ff09428c7c02f47a0",
        retained_vector_sha256: "dd35039af919e84c4dcc8df94da05b04a38aab98e2f34a76aafcf81c4d87f8ef",
        current_option: "outDir",
        current_observation_sha256: "d4394adc118e5a9cac29470d17fc5f58803fd686937f03142614663129ea4301",
    },
    Migration {
        case_id: "typescript-6.0.3/project/maprootUrlMixedSubfolderSpecifyOutputFileAndOutputDirectory.json#module%3Dcommonjs",
        old_case_sha256: "6b7c8501294492ff0663f553def470be7f4f3f9fad541b29bf3cb795db4b4205",
        old_input_sha256: "48a033a297492e9f404c96cd4782a4b87e85a409c18de87e2b8bb113ca0088fa",
        new_input_sha256: "5f16cbe80787af7e191201ef6ebcc5ae70cb06b56af0588e9b4a84cc6a415724",
        retained_vector_sha256: "dd35039af919e84c4dcc8df94da05b04a38aab98e2f34a76aafcf81c4d87f8ef",
        current_option: "outDir",
        current_observation_sha256: "032e1b8e6248bcbbdca1710a9e5c7a11a294f317e1056870218de2b3c2e293ed",
    },
    Migration {
        case_id: "typescript-6.0.3/project/maprootUrlsourcerootUrlMixedSubfolderSpecifyOutputFileAndOutputDirectory.json#module%3Damd",
        old_case_sha256: "59832cf02a9fa8ed62f0531a6ad7b339b2860b7ffffd588e2834275893e2e8df",
        old_input_sha256: "5d2727d7baec726923760f2aa56a226ba455f506f8a04b4227660f2d3e183b4c",
        new_input_sha256: "82d09c5acee613b7f4ada5825be6efde895efb5aa4d62c2ecc9a907105159a13",
        retained_vector_sha256: "dd35039af919e84c4dcc8df94da05b04a38aab98e2f34a76aafcf81c4d87f8ef",
        current_option: "outDir",
        current_observation_sha256: "79a96d43a9a550084964352d00bd6e1b79575076b665be1b8c002d861f84886d",
    },
    Migration {
        case_id: "typescript-6.0.3/project/maprootUrlsourcerootUrlMixedSubfolderSpecifyOutputFileAndOutputDirectory.json#module%3Dcommonjs",
        old_case_sha256: "400ec0e61e542dbd43720e224e3aac4d4ab97761dcf44bf5e528dd37d7659c67",
        old_input_sha256: "bd997812104d87825696906a2d5f1fd753030b78776b8412300f1bd345a9dc75",
        new_input_sha256: "56ac064dd882e531630a9359af8b50be6e4f2a4690986cdc6922f9430eeb6d27",
        retained_vector_sha256: "dd35039af919e84c4dcc8df94da05b04a38aab98e2f34a76aafcf81c4d87f8ef",
        current_option: "outDir",
        current_observation_sha256: "6a0ccb56363c209c2ad48f86de96979276ae60a5a1d7ef46a91f613da8b7d205",
    },
    Migration {
        case_id: "typescript-6.0.3/project/sourcemapMixedSubfolderSpecifyOutputFileAndOutputDirectory.json#module%3Damd",
        old_case_sha256: "f80cac2c58ce576fe4509d7aefc073fe7a35291567ca84a8439268531a9ec33f",
        old_input_sha256: "f8968c452e23793d3b09b9046f822f4fd32dd203f08b54a8502e67bf81bd87c8",
        new_input_sha256: "a96d71324e426011e792bbd1a1db698735922d599f624c2221a1a09800d55063",
        retained_vector_sha256: "dd35039af919e84c4dcc8df94da05b04a38aab98e2f34a76aafcf81c4d87f8ef",
        current_option: "outDir",
        current_observation_sha256: "3de081aafcb37fc8aead490b14ac32f79787e496b3d4467a2bcaeea73e8fb1b9",
    },
    Migration {
        case_id: "typescript-6.0.3/project/sourcemapMixedSubfolderSpecifyOutputFileAndOutputDirectory.json#module%3Dcommonjs",
        old_case_sha256: "da2009ff70a0e770d984280a932dd45c2abc5b0341131edd5b8568ae98647229",
        old_input_sha256: "6540a34ee2e0c54ee31800a5a28822920a04a706447512cf6a3d3d80c044ddaf",
        new_input_sha256: "aea63998030963622d7c8ab1385ab2c0e24daecb56cdee3e887b3400d3b597ac",
        retained_vector_sha256: "dd35039af919e84c4dcc8df94da05b04a38aab98e2f34a76aafcf81c4d87f8ef",
        current_option: "outDir",
        current_observation_sha256: "0bcb7a86ff054c6eb10d60ecda65d80f79ed915358fb296182b9195612b124ec",
    },
    Migration {
        case_id: "typescript-6.0.3/project/sourceRootAbsolutePathMixedSubfolderSpecifyOutputFileAndOutputDirectory.json#module%3Damd",
        old_case_sha256: "7ef25330ff4c35665cdffb733264c7ca7d7f698b5914da12a14d54de07bc6e37",
        old_input_sha256: "c7823d9e7dc50ff0adfdc2ae37c83d890fbd72b691f078bcb74e98b97d942f2c",
        new_input_sha256: "3782cd77ce086262e87549fe44690b2dfba37e66a000b2b57ee27f077b278b5e",
        retained_vector_sha256: "dd35039af919e84c4dcc8df94da05b04a38aab98e2f34a76aafcf81c4d87f8ef",
        current_option: "outDir",
        current_observation_sha256: "c4a4cf6093276df99ed7a512b94edf38ef088f960a2c141b5ddadbdd33fec6b5",
    },
    Migration {
        case_id: "typescript-6.0.3/project/sourceRootAbsolutePathMixedSubfolderSpecifyOutputFileAndOutputDirectory.json#module%3Dcommonjs",
        old_case_sha256: "9dcc2f4e146b4306284f8e75f543d647e1f0b8c887ff9f7b2e2db0416ee6cd00",
        old_input_sha256: "54cfd378991680d0684e700b441fa4269a194f9ec5f94593c85b48b9c9ddbb35",
        new_input_sha256: "0047640e838ab1edc1a80ddecdd698a11b2bd8b8b122b1d61a7fb289f924d440",
        retained_vector_sha256: "dd35039af919e84c4dcc8df94da05b04a38aab98e2f34a76aafcf81c4d87f8ef",
        current_option: "outDir",
        current_observation_sha256: "eb647231e8ba27519bb51712510d418efe58a83e6f846570c8ebc8d3786ee3c4",
    },
    Migration {
        case_id: "typescript-6.0.3/project/sourceRootRelativePathMixedSubfolderSpecifyOutputFileAndOutputDirectory.json#module%3Damd",
        old_case_sha256: "32607d6776f2360bb6bbbf0f17708ae5bcb96c355002a112fe9e5012270e73e0",
        old_input_sha256: "e73453f215449bd9eef3807d8ff54241fd83d9d21d3666c85bb846df64645e0c",
        new_input_sha256: "b64e45068659655e501bf13fe870f152ecb0286fa04174bc1d3541c706926902",
        retained_vector_sha256: "dd35039af919e84c4dcc8df94da05b04a38aab98e2f34a76aafcf81c4d87f8ef",
        current_option: "outDir",
        current_observation_sha256: "bf2390cbbcab55978acaa8ffd96e624f759f2575d18c195fbbec0e3e8a81b498",
    },
    Migration {
        case_id: "typescript-6.0.3/project/sourceRootRelativePathMixedSubfolderSpecifyOutputFileAndOutputDirectory.json#module%3Dcommonjs",
        old_case_sha256: "5f7de5544ecbe0c5ab2d119942b39aca06959a38bb50f67a70a8fb53a676639b",
        old_input_sha256: "c22beaddc5b8adb11d08e5492c2e134c80fc9c226186bd428ab95f365dacc13c",
        new_input_sha256: "0f27f360ec00af4f1496c37d6963cdba30bfcf0144d0f2abd649542bf3c826d7",
        retained_vector_sha256: "dd35039af919e84c4dcc8df94da05b04a38aab98e2f34a76aafcf81c4d87f8ef",
        current_option: "outDir",
        current_observation_sha256: "421f2c9619042ee2731eded704ab934bc56206469d52c320789b44081506b297",
    },
    Migration {
        case_id: "typescript-6.0.3/project/sourcerootUrlMixedSubfolderSpecifyOutputFileAndOutputDirectory.json#module%3Damd",
        old_case_sha256: "6649ad91507f69d6a1c5f9a9ac76d869c6792e8a3c75903c0c6e5ee27bd096f8",
        old_input_sha256: "38dfdb5ce312d105afa1d5711fb32232a46b59618bf3250aa96171ca07bdfa31",
        new_input_sha256: "dda1a8c50f7d4a996939ccaeff6d8d401f4613009dfb1bc25cf831d2f4f38e0f",
        retained_vector_sha256: "dd35039af919e84c4dcc8df94da05b04a38aab98e2f34a76aafcf81c4d87f8ef",
        current_option: "outDir",
        current_observation_sha256: "ab257f143a1ae862c1790cc1f5de518e34a90e2fd9d9ee5932e80b7fe2fa4b35",
    },
    Migration {
        case_id: "typescript-6.0.3/project/sourcerootUrlMixedSubfolderSpecifyOutputFileAndOutputDirectory.json#module%3Dcommonjs",
        old_case_sha256: "fe1cdffb97d39411f121459ac16f831555d2f7de7d1e9ff49f7e738e6d5e3758",
        old_input_sha256: "c5d9b762a43aae5fdd27df3411ee4ced62c891cd7dd3032eb1b0c6a4f2dea95c",
        new_input_sha256: "a4eb7c6d590db9eec82539520d6836753a86af7d7048f19ed34565bd61618233",
        retained_vector_sha256: "dd35039af919e84c4dcc8df94da05b04a38aab98e2f34a76aafcf81c4d87f8ef",
        current_option: "outDir",
        current_observation_sha256: "4b1935059acc4e91b395549191298bec1db1bf52feb66f73ecfc7bd46c8a0511",
    },
];

pub(super) enum CaseOutcome {
    Compared(H2VectorCaseOutcome),
    MigratedRefusal(ObservedRefusal),
}

// Keep construction in real named functions so the source-closure validator
// follows calls to this module without treating enum variants as functions.
pub(super) fn compared(value: H2VectorCaseOutcome) -> CaseOutcome {
    CaseOutcome::Compared(value)
}

pub(super) fn migrated(value: ObservedRefusal) -> CaseOutcome {
    CaseOutcome::MigratedRefusal(value)
}

pub(super) struct ObservedRefusal {
    pub case_id: String,
    pub current_option: String,
    observation: Value,
}

fn find(case_id: &str) -> Option<&'static Migration> {
    CURRENT.iter().find(|row| row.case_id == case_id)
}

pub(super) fn registered(case_id: &str) -> bool {
    find(case_id).is_some()
}

pub(super) fn count() -> usize {
    CURRENT.len()
}

fn frozen(workspace: &Path, path: &str, expected: &str) -> Result<Value, Box<dyn Error>> {
    let bytes = fs::read(workspace.join(path))?;
    if sha256(&bytes) != expected {
        return Err(failure(format!(
            "refusal migration artifact changed: {path}"
        )));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

pub(super) fn validate(
    workspace: &Path,
    cases: &[Value],
    listed: &H2LoadedVectorManifest,
) -> Result<(), Box<dyn Error>> {
    if CURRENT.is_empty() {
        return Ok(());
    }
    let census = frozen(
        workspace,
        "ratchets/h2-7de-candidates.v1.json",
        "1af6d75acf8212135a0850c5ff09487a5589de4d0f825ff1f0e9bc8e3f0f141d",
    )?;
    let inputs = frozen(
        workspace,
        "ratchets/h2-7de-candidate-inputs.v1.json",
        "f2e078a6b6d10cd3c6df833584924c18e8f78fe98c1e41621c70e10e215a073a",
    )?;
    let oracle = frozen(
        workspace,
        "ratchets/h2-7de-observations.v1.json",
        "1a1681b2375d27d9012b06e29808aca72aa3e39d1dbc1536b80ba2aadf9e8ce2",
    )?;
    let mut ids = BTreeSet::new();
    for row in CURRENT {
        let old = cases.iter().find(|case| case["case_id"] == row.case_id);
        let new = array(&census, "cases")?
            .iter()
            .find(|case| case["case_id"] == row.case_id);
        let input = array(&inputs, "cases")?
            .iter()
            .find(|case| case["case_id"] == row.case_id);
        let observation = array(&oracle, "cases")?
            .iter()
            .find(|case| case["case_id"] == row.case_id);
        let retained = listed.entries.get(row.case_id);
        let (Some(old), Some(new), Some(input), Some(observation), Some(retained)) =
            (old, new, input, observation, retained)
        else {
            return Err(failure(format!(
                "{}: missing migration identity or retained historical vector",
                row.case_id
            )));
        };
        let owners = match row.current_option {
            "outDir" => json!(["H2.7d", "H2.8a"]),
            "useCaseSensitiveFileNames" => json!(["H2.7d", "H2.8b"]),
            _ => {
                return Err(failure(
                    "unowned migration boundary; requires separate review",
                ))
            }
        };
        if !ids.insert(row.case_id)
            || !listed.vectors_populated
            || h2_6c_de_promotions::find(row.case_id).is_some()
            || old["disposition"] != "admitted-for-execution"
            || old["case_fingerprint_sha256"] != row.old_case_sha256
            || old["observation_input_sha256"] != row.old_input_sha256
            || new["input_sha256"] != row.new_input_sha256
            || observation["input_sha256"] != row.new_input_sha256
            || input["input"]["route"] != "whole-program"
            || old["source"] != new["source"]
            || new["required_slices"] != owners
            || *retained != vectorize_refusal(H2MismatchProfile::H2_6c, "outFile")
            || retained.facet_fingerprint_sha256.as_deref() != Some(row.retained_vector_sha256)
        {
            return Err(failure(format!(
                "{}: invalid pinned refusal migration",
                row.case_id
            )));
        }
    }
    Ok(())
}

/// Both public calls already returned the same UnsupportedCompilerOption variant.
/// Capture what that API actually exposes; unavailable activity/diagnostics/exit
/// are null, never manufactured zero or success observations.
pub(super) fn observe(
    case_id: &str,
    options: &CompilerOptions,
    case_sensitive: bool,
    current_option: &str,
    sink_writes: [usize; 2],
) -> Result<Option<ObservedRefusal>, Box<dyn Error>> {
    let Some(row) = find(case_id) else {
        return Ok(None);
    };
    let active = match current_option {
        "outDir" => options.out_dir.is_some(),
        "useCaseSensitiveFileNames" => !case_sensitive,
        _ => false,
    };
    if row.current_option != current_option
        || !active
        || sink_writes != [0, 0]
        || options
            .out_file
            .as_deref()
            .is_none_or(|path| path.is_empty())
    {
        return Err(failure(format!(
            "{case_id}: current typed refusal boundary differs"
        )));
    }
    let observation = json!({
        "case_id": case_id,
        "entry": "ProgramSession::emit_with_reported_diagnostics_for_harness_with_lib_bundle",
        "repetitions": 2,
        "result": {"kind": "DriverError::Emit(EmitFailure::UnsupportedCompilerOption)", "option": current_option},
        "sink_write_counts": sink_writes,
        "actual_input_facets": {"outFile": options.out_file, "outDir": options.out_dir, "case_sensitive": case_sensitive},
        "emit_result": null, "reported_diagnostics": null, "command_exit": null, "runtime_activity": null
    });
    let actual = sha256(serde_json::to_vec(&observation)?);
    if actual != row.current_observation_sha256 {
        return Err(failure(format!(
            "{case_id}: current refusal fingerprint differs: {actual}; observation={observation}"
        )));
    }
    Ok(Some(ObservedRefusal {
        case_id: case_id.to_owned(),
        current_option: current_option.to_owned(),
        observation,
    }))
}

pub(super) type Partition = (
    Vec<Result<H2VectorCaseOutcome, String>>,
    Vec<ObservedRefusal>,
);

pub(super) fn partition(
    cases: &[Value],
    results: Vec<Result<CaseOutcome, String>>,
) -> Result<Partition, Box<dyn Error>> {
    let expected_ids = cases
        .iter()
        .map(|case| string(case, "case_id"))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if expected_ids.len() != cases.len() {
        return Err(failure(
            "duplicate original case identity in registry partition",
        ));
    }
    let mut observed_ids = BTreeSet::new();
    let mut compared = Vec::new();
    let mut migrations = Vec::new();
    let mut seen = BTreeSet::new();
    for result in results {
        let result = result.map_err(failure)?;
        let case_id = match &result {
            CaseOutcome::Compared(result) => &result.case_id,
            CaseOutcome::MigratedRefusal(result) => &result.case_id,
        };
        if !expected_ids.contains(case_id.as_str()) || !observed_ids.insert(case_id.clone()) {
            return Err(failure(format!(
                "unknown or duplicate observed registry case: {case_id}"
            )));
        }
        match result {
            CaseOutcome::Compared(result) => {
                if registered(&result.case_id) {
                    return Err(failure(
                        "registered refusal returned a compared/success result",
                    ));
                }
                compared.push(Ok(result));
            }
            CaseOutcome::MigratedRefusal(result) => {
                if !registered(&result.case_id) || !seen.insert(result.case_id.clone()) {
                    return Err(failure("duplicate or unknown observed refusal migration"));
                }
                println!("H2.6c current refusal migration: {}", result.observation);
                migrations.push(result);
            }
        }
    }
    if observed_ids.len() != expected_ids.len() {
        return Err(failure(
            "an original case was not executed in the registry partition",
        ));
    }
    if seen.len() != CURRENT.len() {
        return Err(failure("a registered refusal migration was not executed"));
    }
    Ok((compared, migrations))
}

pub(super) fn ordinary_manifest(
    listed: &H2LoadedVectorManifest,
) -> HashMap<String, H2VectorDivergence> {
    listed
        .entries
        .iter()
        .filter(|(id, _)| !registered(id))
        .map(|(id, value)| (id.clone(), value.clone()))
        .collect()
}

pub(super) fn validate_ordinary_results(
    results: &[Result<H2VectorCaseOutcome, String>],
    listed: &HashMap<String, H2VectorDivergence>,
) -> Result<(), Box<dyn Error>> {
    if CURRENT.is_empty() {
        return Ok(());
    }
    for result in results {
        let observed = result.as_ref().map_err(|error| failure(error.clone()))?;
        if !observed.deferred
            && !observed.divergence.is_exact()
            && listed.get(&observed.case_id) != Some(&observed.divergence)
        {
            return Err(failure(format!("{}: ordinary residual changed; refusal migrations cannot authorize vector replacement", observed.case_id)));
        }
    }
    Ok(())
}

/// Build the next historical manifest: retain migrated rows verbatim as prior
/// vectors. These are not inserted into the current observed mismatch stream.
pub(super) fn retained_manifest(
    listed: &H2LoadedVectorManifest,
    ordinary: &[(String, H2VectorDivergence)],
) -> Result<Vec<(String, H2VectorDivergence)>, Box<dyn Error>> {
    let mut retained = ordinary.to_vec();
    for row in CURRENT {
        retained.push((
            row.case_id.to_owned(),
            listed
                .entries
                .get(row.case_id)
                .ok_or_else(|| failure("missing retained migration row"))?
                .clone(),
        ));
    }
    retained.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(retained)
}

pub(super) fn adjust_refusal_totals(
    mut projected: BTreeMap<String, u64>,
) -> Result<BTreeMap<String, u64>, Box<dyn Error>> {
    for row in CURRENT {
        let count = projected
            .get_mut("outFile")
            .ok_or_else(|| failure("missing historical outFile bucket"))?;
        *count = count
            .checked_sub(1)
            .ok_or_else(|| failure("migration refusal underflow"))?;
        *projected.entry(row.current_option.to_owned()).or_default() += 1;
    }
    projected.retain(|_, count| *count != 0);
    Ok(projected)
}
