use super::*;
use serde_json::{json, Value};

#[test]
fn lossless_option_names_match_exact_lookup_and_spelling_observations() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../fixtures/utf16-config-names.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let units = case["units"]
            .as_array()
            .unwrap()
            .iter()
            .map(|unit| u16::try_from(unit.as_u64().unwrap()).unwrap())
            .collect::<Vec<_>>();
        let name = tsc_diagnostics::JsString::from_code_units(&units);
        assert_eq!(
            compiler_option_declaration(&name).map(|declaration| declaration.name()),
            case["exact"].as_str(),
            "exact lookup: {units:x?}"
        );
        assert_eq!(
            compiler_option_spelling_suggestion(&name).map(|declaration| declaration.name()),
            case["suggestion"].as_str(),
            "suggestion: {units:x?}"
        );
    }
}

fn shape(
    kind: CompilerOptionValueKind,
    file_path: bool,
    command_line_only: bool,
    tsconfig_only: bool,
    extra_validation: bool,
) -> Value {
    let value_type = match kind {
        CompilerOptionValueKind::Boolean => json!({"kind":"boolean"}),
        CompilerOptionValueKind::Number => json!({"kind":"number"}),
        CompilerOptionValueKind::String => json!({"kind":"string"}),
        CompilerOptionValueKind::Named(values) => {
            json!({"kind":"named","values":values.iter().map(|v|json!([v.name,v.value])).collect::<Vec<_>>()})
        }
        CompilerOptionValueKind::Object(v) => {
            json!({"kind":"object","config_dir":v.allow_config_dir_template_substitution})
        }
        CompilerOptionValueKind::List(v) => {
            let mut element = match v.element_kind {
                CompilerOptionListElementKind::String => shape(
                    CompilerOptionValueKind::String,
                    false,
                    false,
                    false,
                    v.validate_file_spec,
                ),
                CompilerOptionListElementKind::FilePath => shape(
                    CompilerOptionValueKind::String,
                    true,
                    false,
                    false,
                    v.validate_file_spec,
                ),
                CompilerOptionListElementKind::Object => shape(
                    CompilerOptionValueKind::Object(CompilerOptionObjectDescriptor {
                        allow_config_dir_template_substitution: false,
                    }),
                    false,
                    false,
                    false,
                    v.validate_file_spec,
                ),
                CompilerOptionListElementKind::NamedString(values) => {
                    json!({"type":{"kind":"named","values":values.iter().map(|v|json!([v.name,v.value])).collect::<Vec<_>>()},"file_path":false,"command_line_only":false,"tsconfig_only":false,"extra_validation":false})
                }
            };
            element["name"] = json!(v.element_name);
            json!({"kind":"list","element":element,"preserve_falsy":v.preserve_falsy_values,"config_dir":v.allow_config_dir_template_substitution})
        }
    };
    json!({"type":value_type,"file_path":file_path,"command_line_only":command_line_only,"tsconfig_only":tsconfig_only,"extra_validation":extra_validation})
}
#[test]
fn all_conversion_schemas_match_pinned_typescript() {
    let oracle: Value =
        serde_json::from_slice(include_bytes!("../../fixtures/h2-8b-config-catalogue.json"))
            .unwrap();
    assert_eq!(oracle["typescript"], "6.0.3");
    for (name, decls) in [
        ("compiler", COMPILER_OPTION_DECLARATIONS),
        ("watch", WATCH_OPTION_DECLARATIONS),
        ("acquisition", ACQUISITION_OPTION_DECLARATIONS),
    ] {
        let actual = decls
            .iter()
            .map(|d| {
                let mut v = shape(
                    d.value_kind,
                    d.is_file_path,
                    d.is_command_line_only,
                    d.is_tsconfig_only,
                    false,
                );
                v["name"] = json!(d.name);
                v
            })
            .collect::<Vec<_>>();
        assert_eq!(
            json!(actual),
            oracle["groups"][name],
            "{name}: full ordered conversion schema"
        );
        eprintln!(
            "H2.8b-CFG-schema {}",
            json!({"group":name,"declarations":actual.len(),"exact":true})
        );
    }
}
