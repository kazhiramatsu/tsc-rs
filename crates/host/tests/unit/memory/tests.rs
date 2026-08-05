use crate::to_file_name_lower_case;

#[test]
fn vendored_file_name_fold_preserves_special_code_points() {
    assert_eq!(
        to_file_name_lower_case("/W/Ä/İıß/FILE.TS"),
        "/w/ä/İıß/file.ts"
    );
    assert_eq!(to_file_name_lower_case("/W/ẞ/ΣΟΣ.TS"), "/w/ß/σος.ts");
}
