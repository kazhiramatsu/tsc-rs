//! Strada's compiler test-file parser (`harnessIO.makeUnitsFromTest` and the
//! `// @name: value` directive scan), which Go's native runner keeps.

use crate::HarnessResult;

use super::{error, CompilerLink, OrderedSetting};

#[derive(Clone, Debug)]
pub(super) struct ParsedUnit {
    pub(super) name: String,
    pub(super) file_options: Vec<OrderedSetting>,
    pub(super) content: Option<String>,
}

pub(super) fn extract_compiler_settings(content: &str) -> Vec<OrderedSetting> {
    let mut settings = Vec::new();
    for start in multiline_starts(content) {
        if let Some((name, value)) = parse_option_at(&content[start..]) {
            set_ordered(&mut settings, name, value);
        }
    }
    settings
}

fn multiline_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, ch) in content.char_indices() {
        if matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
            let next = index + ch.len_utf8();
            if next < content.len() {
                starts.push(next);
            }
        }
    }
    starts
}

fn parse_option_at(text: &str) -> Option<(String, String)> {
    let mut offset = 0;
    consume_exact(text, &mut offset, "//")?;
    skip_js_whitespace(text, &mut offset);
    consume_exact(text, &mut offset, "@")?;
    let name_start = offset;
    while let Some((ch, width)) = next_char(text, offset) {
        if !ch.is_ascii_alphanumeric() && ch != '_' {
            break;
        }
        offset += width;
    }
    if offset == name_start {
        return None;
    }
    let name = text[name_start..offset].to_owned();
    skip_js_whitespace(text, &mut offset);
    consume_exact(text, &mut offset, ":")?;
    skip_js_whitespace(text, &mut offset);
    let value_end = find_cr_or_lf(&text[offset..]).map_or(text.len(), |relative| offset + relative);
    Some((name, js_trim(&text[offset..value_end]).to_owned()))
}

fn parse_link_at(text: &str) -> Option<CompilerLink> {
    let mut offset = 0;
    consume_exact(text, &mut offset, "//")?;
    skip_js_whitespace(text, &mut offset);
    consume_exact(text, &mut offset, "@link")?;
    skip_js_whitespace(text, &mut offset);
    consume_exact(text, &mut offset, ":")?;
    skip_js_whitespace(text, &mut offset);
    let value_end = find_cr_or_lf(&text[offset..]).map_or(text.len(), |relative| offset + relative);
    let value = &text[offset..value_end];
    let arrow = value.rfind("->")?;
    Some(CompilerLink {
        target: js_trim(&value[..arrow]).to_owned(),
        link_path: js_trim(&value[arrow + 2..]).to_owned(),
    })
}

fn parse_first_option(text: &str) -> Option<(String, String)> {
    multiline_starts(text)
        .into_iter()
        .find_map(|start| parse_option_at(&text[start..]))
}

fn parse_first_link(text: &str) -> Option<CompilerLink> {
    multiline_starts(text)
        .into_iter()
        .find_map(|start| parse_link_at(&text[start..]))
}

fn find_cr_or_lf(text: &str) -> Option<usize> {
    text.char_indices()
        .find_map(|(index, ch)| matches!(ch, '\r' | '\n').then_some(index))
}

pub(super) fn make_units_from_test(
    code: &str,
    fixture_path: &str,
) -> HarnessResult<(Vec<ParsedUnit>, Vec<CompilerLink>)> {
    let mut units = Vec::new();
    let mut links = Vec::new();
    let mut current_content: Option<String> = None;
    let mut current_options = Vec::new();
    let mut current_name: Option<String> = None;

    for line in split_content_by_newlines(code) {
        if let Some(link) = parse_first_link(line) {
            links.push(link);
            continue;
        }
        if let Some((name, value)) = parse_first_option(line) {
            set_ordered(&mut current_options, name.clone(), value.clone());
            if !name.eq_ignore_ascii_case("filename") {
                continue;
            }

            if current_name.as_deref().is_some_and(|name| !name.is_empty()) {
                // Go's parser keeps each unit's content in a string builder
                // (`ParseTestFilesAndSymlinksWithOptions`,
                // testrunner/test_case_parser.go:199-216), so a unit without
                // content lines is an empty file.
                units.push(ParsedUnit {
                    name: current_name
                        .take()
                        .expect("truthy file name must be present"),
                    file_options: std::mem::take(&mut current_options),
                    content: Some(current_content.take().unwrap_or_default()),
                });
                current_name = Some(value);
            } else {
                current_name = Some(value);
                if current_content
                    .as_deref()
                    .is_some_and(|content| !content.is_empty() && !only_trivia(content))
                {
                    return Err(error(format!(
                        "compiler fixture {fixture_path:?} contains non-comment content before its first @filename directive"
                    )));
                }
                current_content = Some(String::new());
            }
            continue;
        }
        append_content_line(&mut current_content, line);
    }

    let name = if !units.is_empty()
        || current_name
            .as_deref()
            .is_some_and(|current_name| !current_name.is_empty())
    {
        current_name.unwrap_or_default()
    } else {
        base_file_name(fixture_path).to_owned()
    };
    units.push(ParsedUnit {
        name,
        file_options: current_options,
        content: Some(current_content.unwrap_or_default()),
    });

    Ok((units, links))
}

fn split_content_by_newlines(content: &str) -> impl Iterator<Item = &str> {
    content
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}

fn append_content_line(content: &mut Option<String>, line: &str) {
    let content = content.get_or_insert_with(String::new);
    if !content.is_empty() {
        content.push('\n');
    }
    content.push_str(line);
}

pub(super) fn is_config_file_name(path: &str) -> bool {
    matches!(
        base_file_name(path).to_ascii_lowercase().as_str(),
        "tsconfig.json" | "jsconfig.json"
    )
}

fn set_ordered(settings: &mut Vec<OrderedSetting>, name: String, value: String) {
    if let Some(existing) = settings.iter_mut().find(|setting| setting.name == name) {
        existing.value = value;
    } else {
        settings.push(OrderedSetting { name, value });
    }
}

fn base_file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn consume_exact(text: &str, offset: &mut usize, expected: &str) -> Option<()> {
    text.get(*offset..)?.starts_with(expected).then(|| {
        *offset += expected.len();
    })
}

fn next_char(text: &str, offset: usize) -> Option<(char, usize)> {
    text.get(offset..)?
        .chars()
        .next()
        .map(|ch| (ch, ch.len_utf8()))
}

fn skip_js_whitespace(text: &str, offset: &mut usize) {
    while let Some((ch, width)) = next_char(text, *offset) {
        if !is_js_whitespace(ch) {
            break;
        }
        *offset += width;
    }
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

fn is_js_whitespace(ch: char) -> bool {
    matches!(
        ch,
        '\u{0009}'
            | '\u{000a}'
            | '\u{000b}'
            | '\u{000c}'
            | '\u{000d}'
            | '\u{0020}'
            | '\u{00a0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

fn only_trivia(text: &str) -> bool {
    let mut offset = 0;
    while offset < text.len() {
        skip_js_whitespace(text, &mut offset);
        if offset == text.len() {
            return true;
        }
        let rest = &text[offset..];
        if rest.starts_with("//") || offset == 0 && rest.starts_with("#!") {
            offset += find_cr_or_lf(rest).unwrap_or(rest.len());
            continue;
        }
        if let Some(comment) = rest.strip_prefix("/*") {
            let Some(end) = comment.find("*/") else {
                return true;
            };
            offset += 2 + end + 2;
            continue;
        }
        return false;
    }
    true
}
