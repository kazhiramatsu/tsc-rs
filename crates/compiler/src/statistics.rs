//! `--diagnostics` and `--extendedDiagnostics` (tsgo
//! execute/tsc/statistics.go): a compilation's counts and times, or a
//! build's aggregate, as tsgo's two-column table.
//!
//! Counts follow tsgo's per-program and per-checker sums. Bind time is the
//! residual bind/adoption phase after loading; eager bind work overlaps
//! parsing. Emit time is the synchronous emit gate/tail, while eager emit
//! work overlaps checking. Memory used is the operating system's current
//! process footprint; allocation counts are not available.

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
    /// tsgo's `Identifiers`, `Symbols`, `Types` and `Instantiations` rows:
    /// the identifier nodes of the program's files, the binder's and
    /// checkers' symbols, the checkers' types and instantiations, from the
    /// check's work counters.
    pub(crate) identifiers: usize,
    pub(crate) symbols: usize,
    pub(crate) types: usize,
    pub(crate) instantiations: usize,
    pub(crate) memory_used: Option<u64>,
    pub(crate) config_time: Duration,
    pub(crate) parse_time: Duration,
    pub(crate) bind_time: Duration,
    pub(crate) check_time: Duration,
    pub(crate) emit_time: Duration,
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
        self.identifiers += other.identifiers;
        self.symbols += other.symbols;
        self.types += other.types;
        self.instantiations += other.instantiations;
        if let Some(bytes) = other.memory_used {
            *self.memory_used.get_or_insert(0) += bytes;
        }
        self.config_time += other.config_time;
        self.parse_time += other.parse_time;
        self.bind_time += other.bind_time;
        self.check_time += other.check_time;
        self.emit_time += other.emit_time;
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
        table.add(
            &format!("{prefix}Identifiers"),
            self.identifiers.to_string(),
        );
        table.add(&format!("{prefix}Symbols"), self.symbols.to_string());
        table.add(&format!("{prefix}Types"), self.types.to_string());
        table.add(
            &format!("{prefix}Instantiations"),
            self.instantiations.to_string(),
        );
        if let Some(bytes) = self.memory_used {
            table.add(
                &format!("{prefix}Memory used"),
                format!("{}K", bytes / 1024),
            );
        }
        if !self.config_time.is_zero() {
            table.add_duration(&format!("{prefix}Config time"), self.config_time);
        }
        table.add_duration(&format!("{prefix}Parse time"), self.parse_time);
        if !self.bind_time.is_zero() {
            table.add_duration(&format!("{prefix}Bind time"), self.bind_time);
        }
        if !self.check_time.is_zero() {
            table.add_duration(&format!("{prefix}Check time"), self.check_time);
        }
        if !self.emit_time.is_zero() {
            table.add_duration(&format!("{prefix}Emit time"), self.emit_time);
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
            "Files:              12\nLines:            3456\nIdentifiers:         0\nSymbols:             0\nTypes:               0\nInstantiations:      0\nParse time:     0.025s\nTotal time:     1.250s\n"
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
            "Projects in scope:             2\nProjects built:                1\nTimestamps only updates:       0\nAggregate Files:              12\nAggregate Lines:            3456\nAggregate Identifiers:         0\nAggregate Symbols:             0\nAggregate Types:               0\nAggregate Instantiations:      0\nAggregate Parse time:     0.025s\nAggregate Total time:     0.000s\n"
        );
    }

    #[test]
    fn lines_count_every_terminator() {
        assert_eq!(line_count(""), 1);
        assert_eq!(line_count("a\nb\r\nc\rd\u{2028}e"), 5);
    }

    #[test]
    fn measured_phase_rows_follow_tsgo_order_and_aggregate() {
        let statistics = Statistics {
            memory_used: Some(2048),
            config_time: Duration::from_millis(1),
            parse_time: Duration::from_millis(2),
            bind_time: Duration::from_millis(3),
            check_time: Duration::from_millis(4),
            emit_time: Duration::from_millis(5),
            total_time: Duration::from_millis(15),
            ..Statistics::default()
        };
        let report = statistics.report(None);
        let names: Vec<_> = report
            .lines()
            .map(|line| line.split_once(':').unwrap().0)
            .collect();
        assert_eq!(
            names,
            [
                "Files",
                "Lines",
                "Identifiers",
                "Symbols",
                "Types",
                "Instantiations",
                "Memory used",
                "Config time",
                "Parse time",
                "Bind time",
                "Check time",
                "Emit time",
                "Total time"
            ]
        );
        let mut aggregate = Statistics::default();
        aggregate.aggregate(&statistics);
        aggregate.aggregate(&statistics);
        assert_eq!(aggregate.bind_time, Duration::from_millis(6));
        assert_eq!(aggregate.emit_time, Duration::from_millis(10));
        assert_eq!(aggregate.memory_used, Some(4096));
        assert!(report
            .lines()
            .any(|line| line.starts_with("Memory used:") && line.ends_with("2K")));
        let empty = Statistics::default().report(None);
        assert!(!empty.contains("Bind time:"));
        assert!(!empty.contains("Emit time:"));
        assert!(empty.contains("Types:"));
    }
}
