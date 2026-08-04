use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::json;
use tsc_program::{
    parse_config_root_plan, ConfigHostError, ConfigOptionValueState, ConfigParseHost,
    ConfigRootPlanRequest,
};

#[derive(Default)]
struct MemoryConfigHost {
    files: BTreeMap<String, String>,
}

impl MemoryConfigHost {
    fn with_file(mut self, path: &str, text: String) -> Self {
        self.files.insert(path.to_owned(), text);
        self
    }
}

impl ConfigParseHost for MemoryConfigHost {
    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn file_exists(&self, path: &str) -> Result<bool, ConfigHostError> {
        Ok(self.files.contains_key(path))
    }

    fn read_file(&self, path: &str) -> Result<Option<String>, ConfigHostError> {
        Ok(self.files.get(path).cloned())
    }

    fn read_directory(
        &self,
        _directory: &str,
        _extensions: &[&str],
        _excludes: Option<&[String]>,
        _includes: Option<&[String]>,
        _depth: Option<usize>,
    ) -> Result<Vec<String>, ConfigHostError> {
        Ok(Vec::new())
    }
}

fn push_property(target: &mut String, first: &mut bool, name: &str, value: &str) {
    if !*first {
        target.push(',');
    }
    *first = false;
    write!(target, "\"{name}\":{value}").expect("writing to String cannot fail");
}

fn inherited_config(option_count: usize) -> String {
    let mut text = String::from("{\"compilerOptions\":{");
    let mut first = true;
    push_property(&mut text, &mut first, "allowJs", "true");
    for index in 0..option_count {
        push_property(
            &mut text,
            &mut first,
            &format!("bulk{index:04}"),
            &index.to_string(),
        );
    }
    push_property(&mut text, &mut first, "strict", "true");

    // Reassignment changes the value without moving the first property slot.
    for index in (0..option_count).rev() {
        push_property(
            &mut text,
            &mut first,
            &format!("bulk{index:04}"),
            &(10_000 + index).to_string(),
        );
    }
    text.push_str("},\"files\":[\"base.ts\"]}");
    text
}

fn primary_config(option_count: usize, own_count: usize) -> String {
    let mut text = String::from("{\"extends\":\"./configs/base.json\",\"compilerOptions\":{");
    let mut first = true;

    // New own keys are observed before overrides, but inherited keys retain
    // their earlier slots in the merged public projection.
    push_property(&mut text, &mut first, "ownRevived", "");
    push_property(&mut text, &mut first, "ownGone", "42");
    for index in 0..own_count {
        push_property(
            &mut text,
            &mut first,
            &format!("own{index:04}"),
            &(40_000 + index).to_string(),
        );
    }
    for index in (0..option_count).rev().filter(|index| index % 2 == 0) {
        push_property(
            &mut text,
            &mut first,
            &format!("bulk{index:04}"),
            &(20_000 + index).to_string(),
        );
    }

    // An own undefined value is a raw tombstone. Every second tombstone is
    // restored later and must reuse its original inherited slot.
    for index in (0..option_count).step_by(5) {
        push_property(&mut text, &mut first, &format!("bulk{index:04}"), "");
    }
    for index in (0..option_count).step_by(10) {
        push_property(
            &mut text,
            &mut first,
            &format!("bulk{index:04}"),
            &(30_000 + index).to_string(),
        );
    }
    push_property(&mut text, &mut first, "ownGone", "");
    push_property(&mut text, &mut first, "ownRevived", "50000");
    push_property(&mut text, &mut first, "allowJs", "");
    push_property(&mut text, &mut first, "strict", "");
    push_property(&mut text, &mut first, "allowJs", "false");
    text.push_str("},\"files\":[\"root.ts\"]}");
    text
}

#[test]
fn large_raw_option_merge_preserves_slots_tombstones_and_typed_shadowing() {
    const INHERITED_OPTIONS: usize = 1_024;
    const OWN_OPTIONS: usize = 384;

    let host = MemoryConfigHost::default().with_file(
        "/project/configs/base.json",
        inherited_config(INHERITED_OPTIONS),
    );
    let plan = parse_config_root_plan(
        &host,
        ConfigRootPlanRequest {
            file_name: "/project/tsconfig.json".to_owned(),
            text: primary_config(INHERITED_OPTIONS, OWN_OPTIONS),
            base_path: "/".to_owned(),
        },
    )
    .expect("large compiler-option graph remains recoverable");

    assert_eq!(
        plan.options().typed_value_state("allowJs"),
        ConfigOptionValueState::Value(&json!(false))
    );
    assert_eq!(
        plan.options().typed_value_state("strict"),
        ConfigOptionValueState::Undefined
    );
    assert!(plan.options().get("strict").is_none());

    let entries = plan.options().entries();
    let retained_inherited = (0..INHERITED_OPTIONS)
        .filter(|index| index % 10 != 5)
        .count();
    assert_eq!(entries.len(), 1 + retained_inherited + 1 + OWN_OPTIONS);
    assert_eq!(entries[0].name, "allowJs");
    assert_eq!(entries[0].value, json!(false));

    let mut entry_index = 1;
    for index in 0..INHERITED_OPTIONS {
        let name = format!("bulk{index:04}");
        if index % 10 == 5 {
            assert!(plan.options().get(&name).is_none());
            continue;
        }

        let option = &entries[entry_index];
        assert_eq!(option.name, name);
        let (expected_value, expected_base) = if index % 10 == 0 {
            (30_000 + index, "/project")
        } else if index % 2 == 0 {
            (20_000 + index, "/project")
        } else {
            (10_000 + index, "/project/configs")
        };
        assert_eq!(option.value.as_f64(), Some(expected_value as f64));
        assert_eq!(option.base_path, expected_base);
        assert!(std::ptr::eq(
            option,
            plan.options()
                .get(&option.name)
                .expect("indexed raw lookup")
        ));
        entry_index += 1;
    }

    assert!(plan.options().get("ownGone").is_none());
    let revived = &entries[entry_index];
    assert_eq!(revived.name, "ownRevived");
    assert_eq!(revived.value.as_f64(), Some(50_000.0));
    assert_eq!(revived.base_path, "/project");
    entry_index += 1;

    for index in 0..OWN_OPTIONS {
        let option = &entries[entry_index + index];
        assert_eq!(option.name, format!("own{index:04}"));
        assert_eq!(option.value.as_f64(), Some((40_000 + index) as f64));
        assert_eq!(option.base_path, "/project");
    }
}

#[test]
fn large_invalid_list_orders_conversion_before_notifiers_without_quadratic_repair() {
    const INVALID_ELEMENTS: usize = 1_024;

    let mut text = String::from("{\"compilerOptions\":{\"types\":[");
    let mut conversion_starts = Vec::with_capacity(INVALID_ELEMENTS);
    let mut element_starts = Vec::with_capacity(INVALID_ELEMENTS * 2);
    for index in 0..INVALID_ELEMENTS {
        if index != 0 {
            text.push(',');
        }
        element_starts.push(text.len() as u32);
        conversion_starts.push(text.len() as u32);
        text.push_str("foo,false");
        element_starts.push((text.len() - "false".len()) as u32);
    }
    text.push_str("],\"strict\":true},\"files\":[\"root.ts\"]}");

    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        ConfigRootPlanRequest {
            file_name: "/project/tsconfig.json".to_owned(),
            text,
            base_path: "/".to_owned(),
        },
    )
    .expect("large invalid list remains a bounded partial plan");

    let errors = plan.errors();
    assert_eq!(errors.len(), INVALID_ELEMENTS * 2);
    assert_eq!(
        errors[..INVALID_ELEMENTS]
            .iter()
            .map(|diagnostic| diagnostic.start.unwrap())
            .collect::<Vec<_>>(),
        conversion_starts
    );
    assert_eq!(
        errors[INVALID_ELEMENTS..]
            .iter()
            .map(|diagnostic| diagnostic.start.unwrap())
            .collect::<Vec<_>>(),
        element_starts[..INVALID_ELEMENTS]
    );
    assert!(errors.iter().all(|diagnostic| diagnostic.code() == 5024));
}

#[test]
fn large_paths_map_finalizes_templates_in_one_ordered_pass() {
    const MAPPINGS: usize = 1_024;

    let mut text = String::from("{\"compilerOptions\":{\"paths\":{");
    for index in 0..MAPPINGS {
        if index != 0 {
            text.push(',');
        }
        write!(
            text,
            "\"@pkg{index}/*\":[\"${{configDir}}/generated/{index}/*\",\"relative/{index}/*\",{index}]"
        )
        .expect("writing to String cannot fail");
    }
    text.push_str("}},\"files\":[\"root.ts\"]}");

    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        ConfigRootPlanRequest {
            file_name: "/project/tsconfig.json".to_owned(),
            text,
            base_path: "/".to_owned(),
        },
    )
    .expect("large paths map remains a bounded config plan");

    assert_eq!(plan.options().stored_paths_base_path(), Some("/project"));
    let ConfigOptionValueState::Object(paths) = plan.options().typed_value_state("paths") else {
        panic!("paths is a converted object value")
    };
    let projection = paths.json_projection();
    let paths = projection
        .as_object()
        .expect("paths retains its object shape");
    assert_eq!(paths.len(), MAPPINGS);
    for index in 0..MAPPINGS {
        let values = paths[&format!("@pkg{index}/*")]
            .as_array()
            .expect("paths substitution is an array");
        assert_eq!(values[0], json!(format!("/project/generated/{index}/*")));
        assert_eq!(values[1], json!(format!("relative/{index}/*")));
        assert_eq!(values[2].as_f64(), Some(index as f64));
    }
}
