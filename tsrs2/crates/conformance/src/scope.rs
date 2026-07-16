use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{t0_key, ConformanceResult, GoldenDiag, T0Key};

const SCOPE_SCHEMA: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ScopeStatus {
    Draft,
    Frozen,
}

impl ScopeStatus {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Frozen => "frozen",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ScopeReason {
    HostResolution,
    JsdocSemantics,
    EmitDependent,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ScopeFile {
    schema: u32,
    status: ScopeStatus,
    #[serde(default)]
    exclusions: Vec<ScopeExclusion>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ScopeExclusion {
    fixture: String,
    matrix_key: String,
    file: Option<String>,
    code: u32,
    line: Option<u32>,
    col: Option<u32>,
    reason: ScopeReason,
    evidence: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ScopeKey {
    fixture: String,
    matrix_key: String,
    diagnostic: T0Key,
}

impl ScopeKey {
    fn from_exclusion(exclusion: &ScopeExclusion) -> Self {
        Self {
            fixture: exclusion.fixture.clone(),
            matrix_key: exclusion.matrix_key.clone(),
            diagnostic: T0Key {
                file: exclusion.file.clone(),
                code: exclusion.code,
                line: exclusion.line,
                col: exclusion.col,
            },
        }
    }
}

pub(crate) struct ScopeManifest {
    status: ScopeStatus,
    entries: BTreeMap<ScopeKey, ScopeExclusion>,
    seen: BTreeSet<ScopeKey>,
}

impl ScopeManifest {
    pub(crate) fn load(path: &Path) -> ConformanceResult<Self> {
        let text = fs::read_to_string(path)
            .map_err(|err| format!("failed to read M8 scope manifest {}: {err}", path.display()))?;
        let file: ScopeFile = serde_json::from_str(&text).map_err(|err| {
            format!(
                "failed to parse M8 scope manifest {}: {err}",
                path.display()
            )
        })?;
        if file.schema != SCOPE_SCHEMA {
            return Err(format!(
                "unsupported M8 scope schema {} in {} (expected {SCOPE_SCHEMA})",
                file.schema,
                path.display()
            )
            .into());
        }

        let mut entries = BTreeMap::new();
        for exclusion in file.exclusions {
            if exclusion.fixture.is_empty() {
                return Err("M8 scope exclusion has an empty fixture".into());
            }
            if exclusion.evidence.trim().is_empty() {
                return Err(format!(
                    "M8 scope exclusion {} [{}] {:?}/{} has no evidence",
                    exclusion.fixture, exclusion.matrix_key, exclusion.file, exclusion.code
                )
                .into());
            }
            let key = ScopeKey::from_exclusion(&exclusion);
            if entries.insert(key.clone(), exclusion).is_some() {
                return Err(format!(
                    "duplicate M8 scope exclusion {} [{}] {:?}/{}:{:?}:{:?}",
                    key.fixture,
                    key.matrix_key,
                    key.diagnostic.file,
                    key.diagnostic.code,
                    key.diagnostic.line,
                    key.diagnostic.col
                )
                .into());
            }
        }

        Ok(Self {
            status: file.status,
            entries,
            seen: BTreeSet::new(),
        })
    }

    pub(crate) fn status(&self) -> ScopeStatus {
        self.status
    }

    pub(crate) fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Return exact T0 buckets excluded from the supported-scope
    /// denominator for this case. Exclusions are diagnostic-level and
    /// exact: fixture, matrix key, file, code, line, and column must all
    /// match an oracle semantic/suggestion diagnostic. No wildcard can
    /// silently remove the rest of a fixture from the gate.
    pub(crate) fn exclusions_for_case(
        &mut self,
        fixture: &str,
        matrix_key: &str,
        oracle: &[GoldenDiag],
    ) -> ConformanceResult<BTreeSet<T0Key>> {
        let case_keys = self
            .entries
            .keys()
            .filter(|key| key.fixture == fixture && key.matrix_key == matrix_key)
            .cloned()
            .collect::<Vec<_>>();
        let mut excluded = BTreeSet::new();
        for key in case_keys {
            let matches = oracle
                .iter()
                .filter(|diagnostic| t0_key(diagnostic) == key.diagnostic)
                .collect::<Vec<_>>();
            if matches.is_empty() {
                return Err(format!(
                    "stale M8 scope exclusion {} [{}] {:?}/{}:{:?}:{:?}: oracle diagnostic not found",
                    key.fixture,
                    key.matrix_key,
                    key.diagnostic.file,
                    key.diagnostic.code,
                    key.diagnostic.line,
                    key.diagnostic.col
                )
                .into());
            }
            if matches
                .iter()
                .any(|diagnostic| diagnostic.pass.as_deref() == Some("syntactic"))
            {
                return Err(format!(
                    "M8 scope exclusion {} [{}] {:?}/{} targets a syntactic diagnostic; parser fidelity is always supported scope",
                    key.fixture, key.matrix_key, key.diagnostic.file, key.diagnostic.code
                )
                .into());
            }
            self.seen.insert(key.clone());
            excluded.insert(key.diagnostic);
        }
        Ok(excluded)
    }

    pub(crate) fn finish_full_validation(&self) -> ConformanceResult<()> {
        let unseen = self
            .entries
            .keys()
            .filter(|key| !self.seen.contains(*key))
            .collect::<Vec<_>>();
        if unseen.is_empty() {
            return Ok(());
        }
        let preview = unseen
            .iter()
            .take(5)
            .map(|key| format!("{} [{}]", key.fixture, key.matrix_key))
            .collect::<Vec<_>>()
            .join(", ");
        Err(format!(
            "M8 scope manifest contains {} exclusion(s) outside the full conformance corpus: {preview}",
            unseen.len()
        )
        .into())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{GoldenMessageChain, T0Key};

    fn temp_scope(name: &str, body: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("tsrs2-m8-scope-{name}-{}.json", std::process::id()));
        fs::write(&path, body).unwrap();
        path
    }

    fn oracle(pass: &str) -> GoldenDiag {
        GoldenDiag {
            file: Some("a.ts".to_owned()),
            start: Some(0),
            length: Some(1),
            line: Some(0),
            col: Some(0),
            code: 2307,
            pass: Some(pass.to_owned()),
            category: "error".to_owned(),
            chain: GoldenMessageChain {
                text: "missing".to_owned(),
                code: 2307,
                category: "error".to_owned(),
                next: Vec::new(),
            },
            related: Vec::new(),
            reports_unnecessary: false,
            reports_deprecated: false,
            source: None,
        }
    }

    #[test]
    fn exact_semantic_exclusion_is_selected() {
        let path = temp_scope(
            "exact",
            r#"{
              "schema": 1,
              "status": "draft",
              "exclusions": [{
                "fixture": "conformance/a.ts",
                "matrix_key": "",
                "file": "a.ts",
                "code": 2307,
                "line": 0,
                "col": 0,
                "reason": "host-resolution",
                "evidence": "bare package lookup is outside the batch host"
              }]
            }"#,
        );
        let mut scope = ScopeManifest::load(&path).unwrap();
        let selected = scope
            .exclusions_for_case("conformance/a.ts", "", &[oracle("semantic")])
            .unwrap();
        assert_eq!(
            selected,
            [T0Key {
                file: Some("a.ts".to_owned()),
                code: 2307,
                line: Some(0),
                col: Some(0),
            }]
            .into_iter()
            .collect()
        );
        scope.finish_full_validation().unwrap();
        fs::remove_file(path).ok();
    }

    #[test]
    fn syntactic_exclusions_are_rejected() {
        let path = temp_scope(
            "syntactic",
            r#"{
              "schema": 1,
              "status": "draft",
              "exclusions": [{
                "fixture": "conformance/a.ts",
                "matrix_key": "",
                "file": "a.ts",
                "code": 2307,
                "line": 0,
                "col": 0,
                "reason": "host-resolution",
                "evidence": "invalid test evidence"
              }]
            }"#,
        );
        let mut scope = ScopeManifest::load(&path).unwrap();
        let error = scope
            .exclusions_for_case("conformance/a.ts", "", &[oracle("syntactic")])
            .unwrap_err()
            .to_string();
        assert!(error.contains("syntactic diagnostic"), "{error}");
        fs::remove_file(path).ok();
    }
}
