#[derive(Clone, Debug, Eq, PartialEq)]
struct PackageVersion {
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: Vec<String>,
}

impl PackageVersion {
    const fn stable(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
            prerelease: Vec::new(),
        }
    }

    fn increment(&self, field: VersionField) -> Self {
        match field {
            VersionField::Major => Self::stable(self.major + 1, 0, 0),
            VersionField::Minor => Self::stable(self.major, self.minor + 1, 0),
            VersionField::Patch => Self::stable(self.major, self.minor, self.patch + 1),
        }
    }

    fn with_zero_prerelease(mut self) -> Self {
        self.prerelease = vec!["0".to_owned()];
        self
    }

    fn compare(&self, other: &Self) -> std::cmp::Ordering {
        self.major
            .cmp(&other.major)
            .then_with(|| self.minor.cmp(&other.minor))
            .then_with(|| self.patch.cmp(&other.patch))
            .then_with(|| compare_package_prerelease(&self.prerelease, &other.prerelease))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VersionField {
    Major,
    Minor,
    Patch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VersionComparatorOperator {
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct VersionComparator {
    operator: VersionComparatorOperator,
    operand: PackageVersion,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PartialPackageVersion {
    version: PackageVersion,
    major_wildcard: bool,
    minor_wildcard: bool,
    patch_wildcard: bool,
}

fn compare_package_prerelease(left: &[String], right: &[String]) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    if left.is_empty() || right.is_empty() {
        return match (left.is_empty(), right.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => unreachable!(),
        };
    }
    for (left, right) in left.iter().zip(right) {
        if left == right {
            continue;
        }
        let left_number = package_numeric_identifier(left);
        let right_number = package_numeric_identifier(right);
        return match (left_number, right_number) {
            (Some(left), Some(right)) => left.cmp(&right),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => left.cmp(right),
        };
    }
    left.len().cmp(&right.len())
}

fn package_numeric_identifier(text: &str) -> Option<u64> {
    if text == "0" || (!text.starts_with('0') && text.bytes().all(|byte| byte.is_ascii_digit())) {
        text.parse().ok()
    } else {
        None
    }
}

fn package_version_component(text: &str) -> Option<(u64, bool)> {
    if matches!(text, "*" | "x" | "X") {
        return Some((0, true));
    }
    package_numeric_identifier(text).map(|value| (value, false))
}

fn valid_package_version_identifiers(text: &str) -> bool {
    !text.is_empty()
        && text.split('.').all(|identifier| {
            !identifier.is_empty()
                && identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

/// tsc-port: parsePartial @6.0.3
/// tsc-hash: 4b8d5d206f7ab16042e56130ecfbf4039e125f3adc73097ad8124a3dde2d09d6
/// tsc-span: _tsc.js:2370-2382
fn parse_partial_package_version(text: &str) -> Option<PartialPackageVersion> {
    let (without_build, build) = text
        .split_once('+')
        .map_or((text, None), |(head, tail)| (head, Some(tail)));
    if build.is_some_and(|build| build.contains('+') || !valid_package_version_identifiers(build)) {
        return None;
    }
    let (core, prerelease) = without_build
        .split_once('-')
        .map_or((without_build, None), |(head, tail)| (head, Some(tail)));
    if prerelease.is_some_and(|prerelease| !valid_package_version_identifiers(prerelease)) {
        return None;
    }
    let parts = core.split('.').collect::<Vec<_>>();
    if parts.is_empty()
        || parts.len() > 3
        || (parts.len() < 3 && (prerelease.is_some() || build.is_some()))
    {
        return None;
    }
    let (major, major_wildcard) = package_version_component(parts[0])?;
    let (minor, minor_wildcard) = parts
        .get(1)
        .map_or(Some((0, true)), |part| package_version_component(part))?;
    let (patch, patch_wildcard) = parts
        .get(2)
        .map_or(Some((0, true)), |part| package_version_component(part))?;
    Some(PartialPackageVersion {
        version: PackageVersion {
            major,
            minor: if major_wildcard { 0 } else { minor },
            patch: if major_wildcard || minor_wildcard {
                0
            } else {
                patch
            },
            prerelease: prerelease
                .map(|value| value.split('.').map(str::to_owned).collect())
                .unwrap_or_default(),
        },
        major_wildcard,
        minor_wildcard,
        patch_wildcard,
    })
}

fn push_version_comparator(
    comparators: &mut Vec<VersionComparator>,
    operator: VersionComparatorOperator,
    operand: PackageVersion,
) {
    comparators.push(VersionComparator { operator, operand });
}

/// tsc-port: parseComparator @6.0.3
/// tsc-hash: 6ad4a95c3ed7e6850a75dc755d01e26965d56c67d180042037d4b62e006afd7a
/// tsc-span: _tsc.js:2398-2450
fn parse_package_version_comparator(
    text: &str,
    comparators: &mut Vec<VersionComparator>,
) -> Option<()> {
    let (operator, version_text) = if let Some(rest) = text.strip_prefix("<=") {
        (Some(VersionComparatorOperator::LessEqual), rest)
    } else if let Some(rest) = text.strip_prefix(">=") {
        (Some(VersionComparatorOperator::GreaterEqual), rest)
    } else if let Some(rest) = text.strip_prefix('<') {
        (Some(VersionComparatorOperator::Less), rest)
    } else if let Some(rest) = text.strip_prefix('>') {
        (Some(VersionComparatorOperator::Greater), rest)
    } else if let Some(rest) = text.strip_prefix('=') {
        (Some(VersionComparatorOperator::Equal), rest)
    } else if let Some(rest) = text.strip_prefix('~') {
        let partial = parse_partial_package_version(rest)?;
        if !partial.major_wildcard {
            push_version_comparator(
                comparators,
                VersionComparatorOperator::GreaterEqual,
                partial.version.clone(),
            );
            let field = if partial.minor_wildcard {
                VersionField::Major
            } else {
                VersionField::Minor
            };
            push_version_comparator(
                comparators,
                VersionComparatorOperator::Less,
                partial.version.increment(field),
            );
        }
        return Some(());
    } else if let Some(rest) = text.strip_prefix('^') {
        let partial = parse_partial_package_version(rest)?;
        if !partial.major_wildcard {
            push_version_comparator(
                comparators,
                VersionComparatorOperator::GreaterEqual,
                partial.version.clone(),
            );
            let field = if partial.version.major > 0 || partial.minor_wildcard {
                VersionField::Major
            } else if partial.version.minor > 0 || partial.patch_wildcard {
                VersionField::Minor
            } else {
                VersionField::Patch
            };
            push_version_comparator(
                comparators,
                VersionComparatorOperator::Less,
                partial.version.increment(field),
            );
        }
        return Some(());
    } else {
        (None, text)
    };
    let partial = parse_partial_package_version(version_text)?;
    if partial.major_wildcard {
        if matches!(
            operator,
            Some(VersionComparatorOperator::Less | VersionComparatorOperator::Greater)
        ) {
            push_version_comparator(
                comparators,
                VersionComparatorOperator::Less,
                PackageVersion {
                    major: 0,
                    minor: 0,
                    patch: 0,
                    prerelease: vec!["0".to_owned()],
                },
            );
        }
        return Some(());
    }
    match operator {
        Some(
            operator @ (VersionComparatorOperator::Less | VersionComparatorOperator::GreaterEqual),
        ) => {
            let operand = if partial.minor_wildcard || partial.patch_wildcard {
                partial.version.with_zero_prerelease()
            } else {
                partial.version
            };
            push_version_comparator(comparators, operator, operand);
        }
        Some(
            operator @ (VersionComparatorOperator::LessEqual | VersionComparatorOperator::Greater),
        ) => {
            let (operator, operand) = if partial.minor_wildcard {
                (
                    if operator == VersionComparatorOperator::LessEqual {
                        VersionComparatorOperator::Less
                    } else {
                        VersionComparatorOperator::GreaterEqual
                    },
                    partial
                        .version
                        .increment(VersionField::Major)
                        .with_zero_prerelease(),
                )
            } else if partial.patch_wildcard {
                (
                    if operator == VersionComparatorOperator::LessEqual {
                        VersionComparatorOperator::Less
                    } else {
                        VersionComparatorOperator::GreaterEqual
                    },
                    partial
                        .version
                        .increment(VersionField::Minor)
                        .with_zero_prerelease(),
                )
            } else {
                (operator, partial.version)
            };
            push_version_comparator(comparators, operator, operand);
        }
        Some(VersionComparatorOperator::Equal) | None => {
            if partial.minor_wildcard || partial.patch_wildcard {
                push_version_comparator(
                    comparators,
                    VersionComparatorOperator::GreaterEqual,
                    partial.version.clone().with_zero_prerelease(),
                );
                push_version_comparator(
                    comparators,
                    VersionComparatorOperator::Less,
                    partial
                        .version
                        .increment(if partial.minor_wildcard {
                            VersionField::Major
                        } else {
                            VersionField::Minor
                        })
                        .with_zero_prerelease(),
                );
            } else {
                push_version_comparator(
                    comparators,
                    VersionComparatorOperator::Equal,
                    partial.version,
                );
            }
        }
    }
    Some(())
}

fn package_version_comparator_matches(
    version: &PackageVersion,
    comparator: &VersionComparator,
) -> bool {
    use std::cmp::Ordering;

    let ordering = version.compare(&comparator.operand);
    match comparator.operator {
        VersionComparatorOperator::Less => ordering == Ordering::Less,
        VersionComparatorOperator::LessEqual => ordering != Ordering::Greater,
        VersionComparatorOperator::Greater => ordering == Ordering::Greater,
        VersionComparatorOperator::GreaterEqual => ordering != Ordering::Less,
        VersionComparatorOperator::Equal => ordering == Ordering::Equal,
    }
}

/// Tests a TypeScript package version range against the compiler version pinned by this port.
///
/// Returns `None` when `range` is not a valid package version range.
///
/// tsc-port: VersionRange.tryParse/test @6.0.3
/// tsc-hash: 25b6a78b2d413328c4b3ab536f00a67c4098b91089ce2ad5b69db70e404ee7ce
/// tsc-span: _tsc.js:2321-2486
pub fn compiler_version_satisfies(range: &str) -> Option<bool> {
    let compiler = PackageVersion::stable(6, 0, 3);
    let mut alternatives = Vec::new();
    for alternative in range.trim().split("||") {
        let alternative = alternative.trim();
        if alternative.is_empty() {
            continue;
        }
        let words = alternative.split_whitespace().collect::<Vec<_>>();
        let mut comparators = Vec::new();
        if words.len() == 3 && words[1] == "-" {
            let left = parse_partial_package_version(words[0])?;
            let right = parse_partial_package_version(words[2])?;
            if !left.major_wildcard {
                push_version_comparator(
                    &mut comparators,
                    VersionComparatorOperator::GreaterEqual,
                    left.version,
                );
            }
            if !right.major_wildcard {
                let (operator, operand) = if right.minor_wildcard {
                    (
                        VersionComparatorOperator::Less,
                        right.version.increment(VersionField::Major),
                    )
                } else if right.patch_wildcard {
                    (
                        VersionComparatorOperator::Less,
                        right.version.increment(VersionField::Minor),
                    )
                } else {
                    (VersionComparatorOperator::LessEqual, right.version)
                };
                push_version_comparator(&mut comparators, operator, operand);
            }
        } else {
            for word in words {
                parse_package_version_comparator(word, &mut comparators)?;
            }
        }
        alternatives.push(comparators);
    }
    if alternatives.is_empty() {
        return Some(true);
    }
    Some(alternatives.iter().any(|comparators| {
        comparators
            .iter()
            .all(|comparator| package_version_comparator_matches(&compiler, comparator))
    }))
}

#[cfg(test)]
#[path = "../tests/unit/version/tests.rs"]
mod tests;
