use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub code: String,
    pub message: String,
    pub severity: Severity,
    pub row: Option<usize>,
    pub column: Option<String>,
    pub precinct_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValidationReport {
    pub source_name: String,
    pub original_rows: usize,
    pub accepted_rows: usize,
    pub excluded_rows: usize,
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    pub fn add(
        &mut self,
        code: impl Into<String>,
        message: impl Into<String>,
        severity: Severity,
        row: Option<usize>,
        column: Option<&str>,
        precinct_id: Option<&str>,
    ) {
        self.issues.push(ValidationIssue {
            code: code.into(),
            message: message.into(),
            severity,
            row,
            column: column.map(str::to_owned),
            precinct_id: precinct_id.map(str::to_owned),
        });
    }

    pub fn errors(&self) -> usize {
        self.issues
            .iter()
            .filter(|issue| issue.severity == Severity::Error)
            .count()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrecinctRecord {
    pub source: BTreeMap<String, String>,
    pub source_row: usize,
    pub jurisdiction: String,
    pub precinct: String,
    pub vote_type: Option<String>,
    pub precinct_id: String,
    pub registered_voters: Option<u64>,
    pub active_registered_voters: Option<u64>,
    pub ballots_cast: Option<u64>,
    pub valid_contest_votes: u64,
    pub candidate_votes: BTreeMap<String, u64>,
    pub candidate_shares: BTreeMap<String, Option<f64>>,
    pub write_in_votes: Option<u64>,
    pub undervotes: Option<u64>,
    pub overvotes: Option<u64>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub reported_turnout_percent: Option<f64>,
    pub calculated_turnout_percent: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExcludedRecord {
    pub source_row: usize,
    pub source: BTreeMap<String, String>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    pub source_name: String,
    pub sha256: String,
    pub source_schema: String,
    pub source_columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionResult {
    pub records: Vec<PrecinctRecord>,
    pub excluded: Vec<ExcludedRecord>,
    pub report: ValidationReport,
    pub provenance: Provenance,
    pub candidate_labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MethodState {
    Successful,
    Unavailable,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodStatus {
    pub state: MethodState,
    pub message: String,
    pub diagnostics: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisRecord {
    pub precinct_id: String,
    pub jurisdiction: String,
    pub precinct: String,
    pub candidate_share: Option<f64>,
    pub metrics: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisRun {
    pub created_at: String,
    pub candidate_key: String,
    pub candidate_label: String,
    pub input_schema: String,
    pub random_seed: u64,
    pub requested_methods: Vec<String>,
    pub statuses: BTreeMap<String, MethodStatus>,
    pub diagnostics: BTreeMap<String, serde_json::Value>,
    pub records: Vec<AnalysisRecord>,
    pub input_rows: usize,
    pub analysis_rows: usize,
    pub excluded_rows: usize,
    pub interpretation_warning: String,
}
