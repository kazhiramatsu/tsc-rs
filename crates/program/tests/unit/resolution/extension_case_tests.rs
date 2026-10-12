use super::*;

/// The case-insensitive extension match compares an ASCII tail without
/// folding the whole path; it agrees with folding both sides
/// (toFileNameLowerCase) everywhere, including non-ASCII paths and paths
/// shorter than the extension.
#[test]
fn case_insensitive_extension_matches_agree_with_folding_the_whole_path() {
    let extensions = [
        ModuleExtension::Ts,
        ModuleExtension::Dts,
        ModuleExtension::Tsx,
        ModuleExtension::Json,
        ModuleExtension::Arbitrary(".d.CSS.ts".into()),
    ];
    let paths = [
        "/Work/Lib.D.TS",
        "/work/lib.d.ts",
        "/work/lib.ts",
        "/work/Lib.TSX",
        "/wörk/Lib.D.Ts",
        "/work/a.D.css.TS",
        "/work/aé",
        "/work/İ.TS",
        "ts",
        "",
        "/work/data.JSON",
    ];
    for extension in &extensions {
        for path in paths {
            let folded = to_file_name_lower_case_js(JsStr::from(path));
            let expected = folded
                .as_js()
                .ends_with_js(to_file_name_lower_case_js(extension.as_js()).as_js());
            assert_eq!(
                extension.matches_path_with_case(path, false),
                expected,
                "{path} against {:?}",
                extension.as_js()
            );
        }
    }
}
