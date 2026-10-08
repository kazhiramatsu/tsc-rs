//! `--diagnostics` and `--extendedDiagnostics` (tsgo
//! execute/tsc/statistics.go): a compilation's counts and times, or a
//! build's aggregate, as tsgo's two-column table.
//!
//! The rows are those the port measures: the files and lines of the
//! program and the phase times (the check time covers the check and the
//! emit, which the port runs as one session). tsgo's identifier, symbol,
//! type, instantiation and memory rows are not reported.

use std::time::Duration;

use crate::system::CommandLineTesting;

/// A build's project counts (tsgo `Statistics.isAggregate`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ProjectCounts {
    pub(crate) in_scope: usize,
    pub(crate) built: usize,
    pub(crate) timestamp_updates: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Statistics {
    pub(crate) projects: Option<ProjectCounts>,
    pub(crate) files: usize,
    pub(crate) lines: usize,
    pub(crate) config_time: Duration,
    pub(crate) parse_time: Duration,
    pub(crate) check_time: Duration,
    pub(crate) total_time: Duration,
}

impl Statistics {
    /// A program's files and lines (tsgo `Program.LineCount`: each file's
    /// line starts).
    pub(crate) fn of_program(prepared: &tsc_program::PreparedProgram) -> Self {
        let lines = prepared
            .source_files()
            .iter()
            .map(|source| line_count(source.text()))
            .sum();
        Self {
            files: prepared.source_files().len(),
            lines,
            ..Self::default()
        }
    }

    /// tsgo `Statistics.Aggregate`.
    pub(crate) fn aggregate(&mut self, other: &Self) {
        self.projects.get_or_insert_with(ProjectCounts::default);
        self.files += other.files;
        self.lines += other.lines;
        self.config_time += other.config_time;
        self.parse_time += other.parse_time;
        self.check_time += other.check_time;
    }

    /// tsgo `Statistics.Report`: the table, between the test harness's
    /// statistics markers.
    pub(crate) fn report(&self, testing: Option<&dyn CommandLineTesting>) -> String {
        let mut output = String::new();
        if let Some(testing) = testing {
            testing.on_statistics_start(&mut output);
        }
        let mut table = Table::default();
        let prefix = if let Some(projects) = self.projects {
            table.add("Projects in scope", projects.in_scope.to_string());
            table.add("Projects built", projects.built.to_string());
            table.add(
                "Timestamps only updates",
                projects.timestamp_updates.to_string(),
            );
            "Aggregate "
        } else {
            ""
        };
        table.add(&format!("{prefix}Files"), self.files.to_string());
        table.add(&format!("{prefix}Lines"), self.lines.to_string());
        if !self.config_time.is_zero() {
            table.add_duration(&format!("{prefix}Config time"), self.config_time);
        }
        table.add_duration(&format!("{prefix}Parse time"), self.parse_time);
        if !self.check_time.is_zero() {
            table.add_duration(&format!("{prefix}Check time"), self.check_time);
        }
        table.add_duration(&format!("{prefix}Total time"), self.total_time);
        output.push_str(&table.print());
        if let Some(testing) = testing {
            testing.on_statistics_end(&mut output);
        }
        output
    }
}

/// tsgo's `table`: names left-aligned after a colon, values right-aligned.
#[derive(Default)]
struct Table {
    rows: Vec<(String, String)>,
}

impl Table {
    fn add(&mut self, name: &str, value: String) {
        self.rows.push((format!("{name}:"), value));
    }

    /// tsgo `formatDuration`: seconds with three decimals.
    fn add_duration(&mut self, name: &str, duration: Duration) {
        self.add(name, format!("{:.3}s", duration.as_secs_f64()));
    }

    fn print(&self) -> String {
        let name_width = self
            .rows
            .iter()
            .map(|(name, _)| name.len())
            .max()
            .unwrap_or(0);
        let value_width = self
            .rows
            .iter()
            .map(|(_, value)| value.len())
            .max()
            .unwrap_or(0);
        self.rows
            .iter()
            .map(|(name, value)| format!("{name:<name_width$} {value:>value_width$}\n"))
            .collect()
    }
}

/// The number of line starts (the line terminators of ECMAScript, `\r\n`
/// counting once, plus the first line).
fn line_count(text: &str) -> usize {
    let mut lines = 1;
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                lines += 1;
            }
            '\n' | '\u{2028}' | '\u{2029}' => lines += 1,
            _ => {}
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_aligns_as_tsgos() {
        let statistics = Statistics {
            files: 12,
            lines: 3456,
            parse_time: Duration::from_millis(25),
            total_time: Duration::from_millis(1250),
            ..Statistics::default()
        };
        assert_eq!(
            statistics.report(None),
            "Files:          12\nLines:        3456\nParse time: 0.025s\nTotal time: 1.250s\n"
        );
        let mut build = Statistics::default();
        build.aggregate(&statistics);
        build.projects = Some(ProjectCounts {
            in_scope: 2,
            built: 1,
            timestamp_updates: 0,
        });
        assert_eq!(
            build.report(None),
            concat!(
                "Projects in scope:            2\n",
                "Projects built:               1\n",
                "Timestamps only updates:      0\n",
                "Aggregate Files:             12\n",
                "Aggregate Lines:           3456\n",
                "Aggregate Parse time:    0.025s\n",
                "Aggregate Total time:    0.000s\n",
            )
        );
    }

    #[test]
    fn lines_count_every_terminator() {
        assert_eq!(line_count(""), 1);
        assert_eq!(line_count("a\nb\r\nc\rd\u{2028}e"), 5);
    }
}
