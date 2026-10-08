//! The unified diff tsgo's test baselines print (`baseline.DiffText`): the
//! patience diff of `github.com/peter-evans/patience` v0.3.0 over the texts'
//! lines, in hunks with three lines of context.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Change {
    Delete,
    Insert,
    Equal,
}

impl Change {
    fn symbol(self) -> char {
        match self {
            Self::Delete => '-',
            Self::Insert => '+',
            Self::Equal => ' ',
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Line<'a> {
    text: &'a str,
    change: Change,
}

/// tsgo `baseline.DiffText`: the headers, then one `@@` header and its lines
/// per hunk, joined by newlines (no newline after the last line).
pub(super) fn diff_text(old_name: &str, new_name: &str, old: &str, new: &str) -> String {
    let lines = diff(&split_lines(old), &split_lines(new));
    let mut text = vec![format!("--- {old_name}"), format!("+++ {new_name}")];
    for hunk in hunks(&lines, 3, 3) {
        text.push(format!(
            "@@ -{},{} +{},{} @@",
            hunk.src_start, hunk.src_lines, hunk.dst_start, hunk.dst_lines
        ));
        for line in hunk.lines {
            if line.change == Change::Equal && line.text.is_empty() {
                text.push(String::new());
            } else {
                text.push(format!("{}{}", line.change.symbol(), line.text));
            }
        }
    }
    text.join("\n")
}

/// tsgo `stringutil.SplitLines`: lines ended by `\n`, `\r\n` or `\r`; a final
/// terminator ends the last line without starting another.
fn split_lines(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let (mut start, mut position) = (0, 0);
    while position < bytes.len() {
        match bytes[position] {
            b'\r' if bytes.get(position + 1) == Some(&b'\n') => {
                lines.push(&text[start..position]);
                position += 2;
                start = position;
            }
            b'\r' | b'\n' => {
                lines.push(&text[start..position]);
                position += 1;
                start = position;
            }
            _ => position += 1,
        }
    }
    if start < bytes.len() {
        lines.push(&text[start..]);
    }
    lines
}

fn all<'a>(lines: &[&'a str], change: Change) -> Vec<Line<'a>> {
    lines.iter().map(|&text| Line { text, change }).collect()
}

/// Every line of `a` deleted, then every line of `b` inserted.
fn replaced<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<Line<'a>> {
    let mut lines = all(a, Change::Delete);
    lines.extend(all(b, Change::Insert));
    lines
}

/// patience `Diff`: the shared head and tail are equal; between them, the
/// longest common subsequence of the lines unique to each side anchors a
/// recursive diff of the gaps.
fn diff<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<Line<'a>> {
    if a.is_empty() || b.is_empty() {
        return replaced(a, b);
    }
    let head = a.iter().zip(b).take_while(|(a, b)| a == b).count();
    if head > 0 {
        let mut lines = all(&a[..head], Change::Equal);
        lines.extend(diff(&a[head..], &b[head..]));
        return lines;
    }
    let tail = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    if tail > 0 {
        let mut lines = diff(&a[..a.len() - tail], &b[..b.len() - tail]);
        lines.extend(all(&a[a.len() - tail..], Change::Equal));
        return lines;
    }
    let (unique_a, indices_a) = unique_lines(a);
    let (unique_b, indices_b) = unique_lines(b);
    let anchors = longest_common_subsequence(&unique_a, &unique_b);
    if anchors.is_empty() {
        return replaced(a, b);
    }
    let mut lines = Vec::new();
    let (mut gap_a, mut gap_b) = (0, 0);
    for (anchor_a, anchor_b) in anchors {
        let (index_a, index_b) = (indices_a[anchor_a], indices_b[anchor_b]);
        lines.extend(diff(&a[gap_a..index_a], &b[gap_b..index_b]));
        lines.push(Line {
            text: a[index_a],
            change: Change::Equal,
        });
        gap_a = index_a + 1;
        gap_b = index_b + 1;
    }
    lines.extend(diff(&a[gap_a..], &b[gap_b..]));
    lines
}

/// patience `uniqueElements`: the lines occurring once, with their indices.
fn unique_lines<'a>(lines: &[&'a str]) -> (Vec<&'a str>, Vec<usize>) {
    let mut counts = HashMap::new();
    for &line in lines {
        *counts.entry(line).or_insert(0usize) += 1;
    }
    lines
        .iter()
        .enumerate()
        .filter(|&(_, line)| counts[line] == 1)
        .map(|(index, &line)| (line, index))
        .unzip()
}

/// patience `LCS`: the index pairs of a longest common subsequence, by the
/// dynamic-programming table and its backtrack (a tie moves along `b`).
fn longest_common_subsequence(a: &[&str], b: &[&str]) -> Vec<(usize, usize)> {
    let mut table = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            table[i][j] = if a[i - 1] == b[j - 1] {
                table[i - 1][j - 1] + 1
            } else {
                table[i - 1][j].max(table[i][j - 1])
            };
        }
    }
    let (mut i, mut j) = (a.len(), b.len());
    let mut pairs = Vec::with_capacity(table[i][j]);
    while i > 0 && j > 0 {
        if a[i - 1] == b[j - 1] {
            pairs.push((i - 1, j - 1));
            i -= 1;
            j -= 1;
        } else if table[i - 1][j] > table[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    pairs.reverse();
    pairs
}

#[derive(Clone, Debug, Default)]
struct Hunk<'a> {
    lines: Vec<Line<'a>>,
    src_start: usize,
    src_lines: usize,
    dst_start: usize,
    dst_lines: usize,
}

/// patience `makeHunks`: runs of changed and unchanged lines, each changed
/// run with up to `precontext` lines before it and `postcontext` after; two
/// changes closer than both contexts share a hunk. No hunk when no line
/// differs.
fn hunks<'a>(lines: &[Line<'a>], precontext: usize, postcontext: usize) -> Vec<Hunk<'a>> {
    if lines.iter().all(|line| line.change == Change::Equal) {
        return Vec::new();
    }
    let mut hunks: Vec<Hunk<'a>> = Vec::new();
    let mut update = |block: &Hunk<'a>, last: bool| {
        if block.lines[0].change != Change::Equal {
            match hunks.last_mut() {
                Some(hunk) => {
                    hunk.lines.extend_from_slice(&block.lines);
                    hunk.src_lines += block.src_lines;
                    hunk.dst_lines += block.dst_lines;
                }
                None => hunks.push(block.clone()),
            }
            return;
        }
        let Some(hunk) = hunks.last_mut() else {
            // The first hunk starts with the tail of the unchanged run.
            let context = precontext.min(block.lines.len());
            let skipped = block.lines.len() - context;
            hunks.push(Hunk {
                lines: block.lines[skipped..].to_vec(),
                src_start: skipped + block.src_start,
                src_lines: context,
                dst_start: skipped + block.dst_start,
                dst_lines: context,
            });
            return;
        };
        let joined = if last {
            postcontext
        } else {
            precontext + postcontext
        };
        let mut next = None;
        if block.lines.len() <= joined {
            hunk.lines.extend_from_slice(&block.lines);
            hunk.src_lines += block.lines.len();
            hunk.dst_lines += block.lines.len();
        } else {
            hunk.lines.extend_from_slice(&block.lines[..postcontext]);
            hunk.src_lines += postcontext;
            hunk.dst_lines += postcontext;
            if !last {
                let skipped = block.lines.len() - precontext;
                next = Some(Hunk {
                    lines: block.lines[skipped..].to_vec(),
                    src_start: skipped + block.src_start,
                    src_lines: precontext,
                    dst_start: skipped + block.dst_start,
                    dst_lines: precontext,
                });
            }
        }
        // A hunk that had no line on one side starts where this run does.
        if hunk.src_start == 0 {
            hunk.src_start = block.src_start;
        }
        if hunk.dst_start == 0 {
            hunk.dst_start = block.dst_start;
        }
        hunks.extend(next);
    };
    let mut block = Hunk::default();
    let (mut src_line, mut dst_line) = (0, 0);
    for &line in lines {
        let joins = block.lines.first().is_none_or(|first| {
            first.change == line.change
                || (first.change != Change::Equal && line.change != Change::Equal)
        });
        if !joins {
            update(&block, false);
            block = Hunk::default();
        }
        block.lines.push(line);
        match line.change {
            Change::Delete => {
                src_line += 1;
                block.src_lines += 1;
            }
            Change::Insert => {
                dst_line += 1;
                block.dst_lines += 1;
            }
            Change::Equal => {
                src_line += 1;
                dst_line += 1;
                block.src_lines += 1;
                block.dst_lines += 1;
            }
        }
        if block.src_start == 0 && line.change != Change::Insert {
            block.src_start = src_line;
        }
        if block.dst_start == 0 && line.change != Change::Delete {
            block.dst_start = dst_line;
        }
    }
    update(&block, true);
    hunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_removed_text_is_one_hunk() {
        assert_eq!(
            diff_text("old", "new", "error\n\nFound 1 error.\n\n", ""),
            "--- old\n+++ new\n@@ -1,4 +0,0 @@\n-error\n-\n-Found 1 error.\n-"
        );
    }

    #[test]
    fn distant_changes_are_separate_hunks() {
        let old = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\n";
        let new = "a\nB\nc\nd\ne\nf\ng\nh\ni\nJ\nk\n";
        assert_eq!(
            diff_text("old", "new", old, new),
            concat!(
                "--- old\n+++ new\n",
                "@@ -1,5 +1,5 @@\n a\n-b\n+B\n c\n d\n e\n",
                "@@ -7,5 +7,5 @@\n g\n h\n i\n-j\n+J\n k",
            )
        );
    }

    #[test]
    fn lines_unique_to_one_side_anchor_the_diff() {
        assert_eq!(
            diff_text("old", "new", "x\ny\nz\n", "y\nx\nz\nw\n"),
            "--- old\n+++ new\n@@ -1,3 +1,4 @@\n-x\n y\n+x\n z\n+w"
        );
        assert_eq!(
            diff_text(
                "old",
                "new",
                "1\n2\n3\n4\n5\n6\n7\n8\n",
                "1\n2\n3\nX\n5\n6\n7\n8\nY\n"
            ),
            "--- old\n+++ new\n@@ -1,8 +1,9 @@\n 1\n 2\n 3\n-4\n+X\n 5\n 6\n 7\n 8\n+Y"
        );
    }

    #[test]
    fn equal_texts_have_no_hunk() {
        assert_eq!(
            diff_text("old", "new", "a\nb\n", "a\nb\n"),
            "--- old\n+++ new"
        );
    }
}
