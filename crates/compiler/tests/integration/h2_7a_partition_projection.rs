use serde::Deserialize;

const OWNER_INVENTORY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-7a-owner-inventory.v1.json"
));

#[derive(Debug, Deserialize)]
struct Inventory {
    rows: Vec<InventoryRow>,
}

#[derive(Debug, Deserialize)]
struct InventoryRow {
    surface: String,
    name: String,
    kind: String,
    disposition: String,
    partition: Option<String>,
    target_rung: Option<String>,
}

#[test]
fn h2_7a_partition_projection() {
    let inventory: Inventory =
        serde_json::from_slice(OWNER_INVENTORY).expect("owner inventory is valid JSON");
    let actual = inventory
        .rows
        .into_iter()
        .filter(|row| row.surface == "resolver-declaration-subset" && row.kind == "member")
        .map(|row| {
            (
                row.name,
                row.kind,
                row.disposition,
                row.partition,
                row.target_rung,
            )
        })
        .collect::<Vec<_>>();

    // h2-7a-m-2.md §2(a)/§2(c) is the ratified ordered partition.
    let expected = [
        member(
            "isDefinitelyReferenceToGlobalSymbolObject",
            "checker-native-resolver",
            Some("m-2"),
            Some("h2-7a-m-2"),
        ),
        member(
            "isSymbolAccessible",
            "checker-native-resolver",
            Some("m-2"),
            Some("h2-7a-m-2"),
        ),
        member(
            "isEntityNameVisible",
            "checker-native-resolver",
            Some("m-2"),
            Some("h2-7a-m-2"),
        ),
        member(
            "isDeclarationVisible",
            "checker-native-resolver",
            Some("m-2"),
            Some("h2-7a-m-2"),
        ),
        member(
            "isOptionalParameter",
            "checker-native-resolver",
            Some("m-2"),
            Some("h2-7a-m-2"),
        ),
        member(
            "isImplementationOfOverload",
            "checker-native-resolver",
            Some("m-2"),
            Some("h2-7a-m-2"),
        ),
        member(
            "requiresAddingImplicitUndefined",
            "checker-native-resolver",
            Some("m-2"),
            Some("h2-7a-m-2"),
        ),
        member(
            "isExpandoFunctionDeclaration",
            "checker-native-resolver",
            Some("m-2"),
            Some("h2-7a-m-2"),
        ),
        member(
            "getPropertiesOfContainerFunction",
            "checker-native-resolver",
            Some("m-2"),
            Some("h2-7a-m-2"),
        ),
        member("getEnumMemberValue", "existing-resolver-api", None, None),
        member(
            "createTypeOfDeclaration",
            "node-builder-dependent-resolver",
            Some("m-3-head"),
            Some("h2-7a-m-3"),
        ),
        member(
            "createReturnTypeOfSignatureDeclaration",
            "node-builder-dependent-resolver",
            Some("m-3-head"),
            Some("h2-7a-m-3"),
        ),
        member(
            "createTypeOfExpression",
            "node-builder-dependent-resolver",
            Some("m-3-head"),
            Some("h2-7a-m-3"),
        ),
        member(
            "isLiteralConstDeclaration",
            "checker-native-resolver",
            Some("m-2"),
            Some("h2-7a-m-2"),
        ),
        member(
            "createLiteralConstValue",
            "node-builder-dependent-resolver",
            Some("m-3-head"),
            Some("h2-7a-m-3"),
        ),
        member(
            "isLateBound",
            "checker-native-resolver",
            Some("m-2"),
            Some("h2-7a-m-2"),
        ),
        member(
            "getDeclarationStatementsForSourceFile",
            "node-builder-dependent-resolver",
            Some("m-3-head"),
            Some("h2-7a-m-3"),
        ),
        member(
            "createLateBoundIndexSignatures",
            "node-builder-dependent-resolver",
            Some("m-3-head"),
            Some("h2-7a-m-3"),
        ),
        member(
            "symbolToDeclarations",
            "node-builder-internal-api-surface",
            Some("m-3-head"),
            Some("h2-7a-m-3"),
        ),
        member(
            "isImportRequiredByAugmentation",
            "checker-native-resolver",
            Some("m-2"),
            Some("h2-7a-m-2"),
        ),
    ];

    assert_eq!(actual, expected);
}

fn member(
    name: &str,
    disposition: &str,
    partition: Option<&str>,
    target_rung: Option<&str>,
) -> (String, String, String, Option<String>, Option<String>) {
    (
        name.to_owned(),
        "member".to_owned(),
        disposition.to_owned(),
        partition.map(str::to_owned),
        target_rung.map(str::to_owned),
    )
}
