use super::*;
use crate::model::{OptionalBool, OptionalString};

fn diagnostic(start: u32, text: &str) -> DiagnosticRecord {
    DiagnosticRecord {
        pass: DiagnosticPass::Semantic,
        file: DiagnosticFile::File {
            path: "main.ts".to_owned(),
        },
        code: 2322,
        line: OptionalU32::Present { value: 0 },
        column: OptionalU32::Present { value: 0 },
        category: DiagnosticCategory::Error,
        start: OptionalU32::Present { value: start },
        length: OptionalU32::Present { value: 1 },
        chain: MessageChain {
            text: text.to_owned(),
            code: 2322,
            category: DiagnosticCategory::Error,
            next_present: false,
            next: Vec::new(),
        },
        related_information_present: false,
        related: Vec::new(),
        reports_unnecessary: OptionalBool::absent(),
        reports_deprecated: OptionalBool::absent(),
        source: OptionalString::absent(),
    }
}

#[test]
fn t0_uses_line_and_column_not_start() {
    let left = diagnostic(0, "head");
    let right = diagnostic(1, "head");
    assert_eq!(
        tier_key(&left, ComparisonTier::T0),
        tier_key(&right, ComparisonTier::T0)
    );
    assert_ne!(
        tier_key(&left, ComparisonTier::T2),
        tier_key(&right, ComparisonTier::T2)
    );
}

#[test]
fn t3_excludes_formatter_sidecars() {
    let left = diagnostic(0, "head");
    let mut right = left.clone();
    right.chain.next_present = true;
    right.related_information_present = true;
    right.reports_unnecessary = OptionalBool::present(true);
    right.reports_deprecated = OptionalBool::present(false);
    right.source = OptionalString::present("ts");
    assert_eq!(
        tier_key(&left, ComparisonTier::T3),
        tier_key(&right, ComparisonTier::T3)
    );
}
