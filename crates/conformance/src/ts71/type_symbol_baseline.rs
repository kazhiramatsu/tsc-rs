//! The `.types` and `.symbols` baselines of TypeScript's native compiler
//! runner (`tsc/internal/testutil/tsbaseline/type_symbol_baseline.go`): the
//! layout of one walk's results over the case's files. The walk itself (the
//! nodes visited and the type or symbol text of each) is the runner's; this
//! module renders what it produced exactly as `generateBaseline` and
//! `iterateBaseline` do.

use super::errors_baseline::remove_test_path_prefixes;

/// One line of a baseline: the 0-based line of the node's first token, the
/// node's source text and its type or symbol text
/// (`typeWriterResult`, type_symbol_baseline.go:285-291; `underline` is
/// never set).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WalkResult {
    pub line: usize,
    pub source_text: String,
    pub text: String,
}

/// One file of the baseline: the unit name of the header line, the content
/// whose lines are interleaved with the results, and the results in walk
/// order.
pub(super) struct WalkedFile<'a> {
    pub unit_name: &'a str,
    pub content: &'a str,
    pub results: &'a [WalkResult],
}

/// `generateBaseline` (type_symbol_baseline.go:144-193): the header
/// `//// [tests/cases/<suite>/<case>] ////`, a blank line and every file's
/// chunk; `None` (`baseline.NoContent`) when there is no file at all.
pub(super) fn render(header: &str, files: &[WalkedFile<'_>]) -> Result<Option<String>, String> {
    let mut body = String::new();
    for file in files {
        body.push_str(&iterate(file)?);
    }
    if body.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!("//// [{header}] ////\r\n\r\n{body}")))
}

/// `iterateBaseline` (type_symbol_baseline.go:195-261) for one file: the
/// `=== unit ===` line, then the code lines up to each result's line
/// (joined by CRLF) followed by the `>text : type` line, the remaining code
/// lines, and a final CRLF. A blank line precedes a block of code lines
/// unless the first of them is a bracket line or blank. The chunk goes
/// through `removeTestPathPrefixes`.
fn iterate(file: &WalkedFile<'_>) -> Result<String, String> {
    let mut text = format!("=== {} ===\r\n", file.unit_name);
    let code_lines = code_lines(file.content);
    let mut last_written: Option<usize> = None;
    for result in file.results {
        if result.line >= code_lines.len() {
            return Err(format!(
                "{}: a result on line {} of {} lines",
                file.unit_name,
                result.line,
                code_lines.len()
            ));
        }
        match last_written {
            None => {
                text.push_str(&code_lines[..=result.line].join("\r\n"));
                text.push_str("\r\n");
            }
            Some(last) if last != result.line => {
                if last > result.line {
                    return Err(format!(
                        "{}: a result on line {} after one on line {last}",
                        file.unit_name, result.line
                    ));
                }
                if !block_follows_without_blank(&code_lines, last + 1) {
                    text.push_str("\r\n");
                }
                text.push_str(&code_lines[last + 1..=result.line].join("\r\n"));
                text.push_str("\r\n");
            }
            Some(_) => {}
        }
        last_written = Some(result.line);
        // lineDelimiter (`\r?\n`) removed from the node's text.
        let line_text = result.source_text.replace("\r\n", "").replace('\n', "");
        text.push('>');
        text.push_str(&line_text);
        text.push_str(" : ");
        text.push_str(&result.text);
        text.push_str("\r\n");
    }
    let next = last_written.map_or(0, |last| last + 1);
    if next < code_lines.len() {
        if !block_follows_without_blank(&code_lines, next) {
            text.push_str("\r\n");
        }
        text.push_str(&code_lines[next..].join("\r\n"));
    }
    text.push_str("\r\n");
    Ok(remove_test_path_prefixes(&text))
}

/// Whether the code block starting at `index` is written without a blank
/// line before it: its first line is a bracket line (`^\s*[{|}]\s*$`, Go
/// RE2's ASCII `\s`) or blank (`strings.TrimSpace`, Unicode).
fn block_follows_without_blank(code_lines: &[&str], index: usize) -> bool {
    index < code_lines.len()
        && (is_bracket_line(code_lines[index]) || code_lines[index].trim().is_empty())
}

fn is_bracket_line(line: &str) -> bool {
    let ascii_space = |c: char| matches!(c, '\t' | '\n' | '\x0C' | '\r' | ' ');
    let trimmed = line.trim_matches(ascii_space);
    matches!(trimmed, "{" | "|" | "}")
}

/// `codeLinesRegexp.Split` (`[\r  ]|\r?\n`): Go's leftmost-first
/// alternation matches a lone `\r` before `\r\n`, so a CRLF yields an empty
/// line between its halves (a unit's content holds none: the case parser
/// rejoins its lines with LF).
fn code_lines(content: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, character) in content.char_indices() {
        if matches!(character, '\r' | '\n' | '\u{2028}' | '\u{2029}') {
            lines.push(&content[start..index]);
            start = index + character.len_utf8();
        }
    }
    lines.push(&content[start..]);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(line: usize, source: &str, text: &str) -> WalkResult {
        WalkResult {
            line,
            source_text: source.to_owned(),
            text: text.to_owned(),
        }
    }

    #[test]
    fn code_lines_are_split_on_every_ecma_line_break_like_go() {
        assert_eq!(code_lines("a\nb"), ["a", "b"]);
        assert_eq!(code_lines("a\r\nb"), ["a", "", "b"]);
        assert_eq!(code_lines("a\u{2028}b\u{2029}c\r"), ["a", "b", "c", ""]);
        assert_eq!(code_lines(""), [""]);
    }

    #[test]
    fn bracket_lines_use_ascii_whitespace_only() {
        assert!(is_bracket_line("  }  "));
        assert!(is_bracket_line("\t|"));
        assert!(!is_bracket_line("\u{a0}}"));
        assert!(!is_bracket_line("{}"));
        assert!(!is_bracket_line(""));
    }

    #[test]
    fn a_file_is_laid_out_like_2d_arrays_types() {
        // tests/cases/compiler/2dArrays.ts after the directive line is
        // removed: the class lines, blank lines between members.
        let content = "class Cell {\n}\n\nclass Ship {\n    isSunk: boolean = false;\n}\n";
        let results = [
            result(0, "Cell", "Cell"),
            result(3, "Ship", "Ship"),
            result(4, "isSunk", "boolean"),
            result(4, "false", "false"),
        ];
        let rendered = render(
            "tests/cases/compiler/2dArrays.ts",
            &[WalkedFile {
                unit_name: "2dArrays.ts",
                content,
                results: &results,
            }],
        )
        .expect("laid out")
        .expect("content");
        assert_eq!(
            rendered,
            "//// [tests/cases/compiler/2dArrays.ts] ////\r\n\r\n\
             === 2dArrays.ts ===\r\n\
             class Cell {\r\n>Cell : Cell\r\n}\r\n\r\n\
             class Ship {\r\n>Ship : Ship\r\n\r\n    isSunk: boolean = false;\r\n>isSunk : boolean\r\n>false : false\r\n}\r\n\r\n"
        );
    }

    #[test]
    fn a_file_without_results_keeps_its_lines_after_a_blank_line() {
        let rendered = render(
            "tests/cases/compiler/asiReturn.ts",
            &[WalkedFile {
                unit_name: "asiReturn.ts",
                content: "return\n",
                results: &[],
            }],
        )
        .expect("laid out")
        .expect("content");
        assert_eq!(
            rendered,
            "//// [tests/cases/compiler/asiReturn.ts] ////\r\n\r\n=== asiReturn.ts ===\r\n\r\nreturn\r\n\r\n"
        );
    }

    #[test]
    // `};` is not a bracket line (one bracket only), so a blank line precedes it.
    fn multi_line_node_text_loses_its_line_breaks_and_paths_their_prefixes() {
        let content = "var x = {\n  a: 1\n};\n";
        let results = [
            result(0, "x", "{ a: number; }"),
            result(0, "{\n  a: 1\n}", "{ a: number; }"),
            result(1, "a", "number"),
            result(1, "1", "1"),
        ];
        let rendered = render(
            "tests/cases/compiler/x.ts",
            &[WalkedFile {
                unit_name: "/.src/x.ts",
                content,
                results: &results,
            }],
        )
        .expect("laid out")
        .expect("content");
        assert_eq!(
            rendered,
            "//// [tests/cases/compiler/x.ts] ////\r\n\r\n=== x.ts ===\r\n\
             var x = {\r\n>x : { a: number; }\r\n>{  a: 1} : { a: number; }\r\n\
             \r\n  a: 1\r\n>a : number\r\n>1 : 1\r\n\r\n};\r\n\r\n"
        );
    }

    #[test]
    fn no_files_is_no_content_and_out_of_range_lines_are_errors() {
        assert_eq!(render("h", &[]).expect("laid out"), None);
        let results = [result(3, "x", "t")];
        assert!(render(
            "h",
            &[WalkedFile {
                unit_name: "f.ts",
                content: "x\n",
                results: &results,
            }]
        )
        .is_err());
    }
}
