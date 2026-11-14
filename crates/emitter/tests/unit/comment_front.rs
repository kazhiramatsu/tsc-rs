//! Comment boundaries preserve canonical collection and logical cursor work.
use super::*;
use crate::token_cursor::{cursor_work, reset_cursor_work};

#[test]
fn empty_comment_scans_keep_crlf_cursor_work_and_stop_at_operators() {
    let source = "x  \r\n  value";
    for (trailing, expected_end) in [
        (true, source.find("\r\n").unwrap() + 2),
        (false, source.find("value").unwrap()),
    ] {
        reset_cursor_work();
        assert!(collect_source_comment_ranges(source, 1, trailing).is_empty());
        assert_eq!(cursor_work().source_bytes, (expected_end - 1) as u64);
        assert_eq!(cursor_work().emissions, 0);
    }
    reset_cursor_work();
    assert!(collect_source_comment_ranges("x \t/ value", 1, true).is_empty());
    assert_eq!(cursor_work().source_bytes, 2);
}

#[test]
fn comment_runs_keep_pending_newlines_and_leading_collection_boundaries() {
    for separator in [" ", "\u{2028} "] {
        let source = format!("x /* keep */{separator}// next\r\nvalue");
        let block_start = source.find("/*").unwrap();
        let block_end = source.find("*/").unwrap() + 2;
        let line_start = source.find("//").unwrap();
        let line_end = source.find("\r\n").unwrap();
        assert_eq!(
            collect_source_comment_ranges(&source, 1, true),
            vec![
                SourceCommentRange {
                    start: block_start,
                    end: block_end,
                    kind: SourceCommentKind::Block,
                    has_trailing_new_line: separator.starts_with('\u{2028}'),
                },
                SourceCommentRange {
                    start: line_start,
                    end: line_end,
                    kind: SourceCommentKind::Line,
                    has_trailing_new_line: true,
                },
            ]
        );
        assert!(collect_source_comment_ranges(&source, 1, false).is_empty());
    }
}

#[test]
fn shebangs_unterminated_comments_and_invalid_boundaries_keep_their_ranges() {
    let source = "#!/bin/tool\r\n/* keep */\nvalue";
    assert_eq!(
        collect_source_comment_ranges(source, 0, false),
        vec![SourceCommentRange {
            start: source.find("/*").unwrap(),
            end: source.find("*/").unwrap() + 2,
            kind: SourceCommentKind::Block,
            has_trailing_new_line: true,
        }]
    );
    let source = "/* unfinished";
    assert_eq!(
        collect_source_comment_ranges(source, 0, false),
        vec![SourceCommentRange {
            start: 0,
            end: source.len(),
            kind: SourceCommentKind::Block,
            has_trailing_new_line: false,
        }]
    );
    reset_cursor_work();
    for position in [1, "é value".len(), usize::MAX] {
        assert!(collect_source_comment_ranges("é value", position, false).is_empty());
    }
    assert_eq!(cursor_work().source_bytes, 0);
}
