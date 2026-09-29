//! h2-7a checker/harness L4 control: the m-1 audit closure.
//!
//! After the P5 flips + the P7 re-mint, the frozen owner inventory
//! carries no `audit-foundation-needed` row: every printer-subgraph and
//! factory/parenthesizer row is `audit-already-exact` with a
//! `crates/emitter/**` anchor (the mint-time header verification covers
//! the 190 rows flipped here; the m-2 partition projection has its own
//! focused test).

use serde_json::Value;

const INVENTORY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-7a-owner-inventory.v1.json"
));

#[test]
fn owner_inventory_audit_is_closed_after_m35() {
    let inventory: Value = serde_json::from_slice(INVENTORY).expect("inventory is valid JSON");
    let audit = &inventory["summary"]["audit"];
    assert_eq!(
        audit["foundation_needed"], 0,
        "every m-1 foundation row flipped by the checker/harness lane"
    );
    assert_eq!(audit["pending"], 0);
    assert_eq!(audit["already_exact"], 308);

    let mut exact = 0usize;
    for row in inventory["rows"].as_array().expect("rows") {
        let surface = row["surface"].as_str().expect("surface");
        if surface != "printer-subgraph" && surface != "factory-parenthesizer" {
            continue;
        }
        assert_eq!(
            row["disposition"], "audit-already-exact",
            "{} still carries {}",
            row["name"], row["disposition"]
        );
        assert!(
            row["target_rung"].is_null(),
            "{} still targets a rung",
            row["name"]
        );
        let anchor = row["rust_anchor"].as_str().expect("anchor string");
        assert!(
            anchor.starts_with("crates/emitter/"),
            "{} anchor {anchor} is outside crates/emitter",
            row["name"]
        );
        exact += 1;
    }
    assert_eq!(exact, 308, "the audit covers exactly the 308 measured rows");
}
