use std::collections::{BTreeMap, HashSet};

use chrono::Utc;
use serde_json::Value;

use crate::{
    CAUTION,
    config::Config,
    error::{AppError, Result},
    model::{AnalysisRun, IngestionResult, MethodState, MethodStatus},
    statistics,
};

pub const ALL_METHODS: [&str; 6] = [
    "turnout_share",
    "vote_share_by_count",
    "down_ballot_difference",
    "digit_diagnostics",
    "spatial",
    "robust_multivariate",
];

pub fn analyze(
    ingestion: &IngestionResult,
    candidate_key: &str,
    methods: &[String],
    config: &Config,
) -> Result<AnalysisRun> {
    let candidate_label = ingestion
        .candidate_labels
        .get(candidate_key)
        .ok_or_else(|| AppError::Validation(format!("unknown candidate key: {candidate_key}")))?
        .clone();
    let known: HashSet<&str> = ALL_METHODS.into_iter().collect();
    let unknown: Vec<_> = methods
        .iter()
        .filter(|method| !known.contains(method.as_str()))
        .collect();
    if !unknown.is_empty() {
        return Err(AppError::Validation(format!(
            "unknown methods: {unknown:?}"
        )));
    }
    let mut records = statistics::empty_metrics(&ingestion.records, candidate_key);
    let mut statuses = BTreeMap::new();
    let mut diagnostics = BTreeMap::new();
    for method in methods {
        let result: Result<Value> = match method.as_str() {
            "turnout_share" => {
                statistics::turnout_share(&ingestion.records, &mut records, candidate_key, config)
            }
            "vote_share_by_count" => {
                statistics::vote_share_by_count(&ingestion.records, &mut records, candidate_key)
            }
            "down_ballot_difference" => statistics::down_ballot_difference(
                &ingestion.records,
                &mut records,
                candidate_key,
                config,
            ),
            "digit_diagnostics" => statistics::digit_diagnostics(&ingestion.records, config),
            "spatial" => {
                statistics::spatial(&ingestion.records, &mut records, candidate_key, config)
            }
            "robust_multivariate" => statistics::robust_multivariate(
                &ingestion.records,
                &mut records,
                candidate_key,
                config,
            ),
            _ => unreachable!(),
        };
        match result {
            Ok(diagnostic) => {
                diagnostics.insert(method.clone(), diagnostic.clone());
                statuses.insert(
                    method.clone(),
                    MethodStatus {
                        state: MethodState::Successful,
                        message: "completed".into(),
                        diagnostics: diagnostic,
                    },
                );
            }
            Err(AppError::Unavailable(message)) => {
                let state = if message == "disabled by configuration" {
                    MethodState::Skipped
                } else {
                    MethodState::Unavailable
                };
                statuses.insert(
                    method.clone(),
                    MethodStatus {
                        state,
                        message,
                        diagnostics: Value::Object(Default::default()),
                    },
                );
            }
            Err(error) => {
                statuses.insert(
                    method.clone(),
                    MethodStatus {
                        state: MethodState::Failed,
                        message: error.to_string(),
                        diagnostics: Value::Object(Default::default()),
                    },
                );
            }
        }
    }
    Ok(AnalysisRun {
        created_at: Utc::now().to_rfc3339(),
        candidate_key: candidate_key.into(),
        candidate_label,
        input_schema: ingestion.provenance.source_schema.clone(),
        random_seed: config.statistics.random_seed,
        requested_methods: methods.to_vec(),
        statuses,
        diagnostics,
        analysis_rows: records.len(),
        records,
        input_rows: ingestion.report.original_rows,
        excluded_rows: ingestion.excluded.len(),
        interpretation_warning: CAUTION.into(),
    })
}

#[cfg(test)]
mod tests {
    use crate::{ingestion::ingest_bytes, sample::sample_csv};

    use super::*;

    #[test]
    fn complete_workflow_has_independent_statuses() {
        let mut config = Config::default();
        config.statistics.minimum_observations = 10;
        config.statistics.spatial_permutations = 49;
        let csv = sample_csv(40, 42);
        let ingestion = ingest_bytes(csv.as_bytes(), "sample.csv", &config).unwrap();
        let methods = ALL_METHODS
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>();
        let run = analyze(&ingestion, "candidate_a", &methods, &config).unwrap();
        assert_eq!(run.records.len(), 40);
        assert!(
            run.statuses
                .values()
                .all(|status| status.state != MethodState::Failed)
        );
        assert!(run.interpretation_warning.contains("not proof"));
    }

    #[test]
    fn missing_turnout_and_coordinates_are_explicitly_unavailable() {
        let mut config = Config::default();
        config.statistics.minimum_observations = 3;
        let csv = b"Jurisdiction,Precinct,Valid_Contest_Votes,Votes_Candidate_A,Votes_Candidate_B\nA,1,10,4,6\nA,2,11,5,6\nB,3,12,7,5\n";
        let ingestion = ingest_bytes(csv, "minimal.csv", &config).unwrap();
        let methods = vec![
            "turnout_share".into(),
            "spatial".into(),
            "vote_share_by_count".into(),
        ];
        let run = analyze(&ingestion, "candidate_a", &methods, &config).unwrap();
        assert_eq!(
            run.statuses["turnout_share"].state,
            MethodState::Unavailable
        );
        assert_eq!(run.statuses["spatial"].state, MethodState::Unavailable);
        assert_eq!(
            run.statuses["vote_share_by_count"].state,
            MethodState::Successful
        );
    }

    #[test]
    fn robust_scores_are_deterministic_and_row_order_invariant() {
        let mut config = Config::default();
        config.statistics.minimum_observations = 5;
        let sample = sample_csv(30, 7);
        let first = ingest_bytes(sample.as_bytes(), "sample.csv", &config).unwrap();
        let mut second = first.clone();
        second.records.reverse();
        let methods = vec!["robust_multivariate".into()];
        let first_run = analyze(&first, "candidate_a", &methods, &config).unwrap();
        let second_run = analyze(&second, "candidate_a", &methods, &config).unwrap();
        let scores = |run: &AnalysisRun| {
            run.records
                .iter()
                .map(|record| {
                    (
                        record.precinct_id.clone(),
                        record.metrics["robust_anomaly_score"].as_f64().unwrap(),
                    )
                })
                .collect::<BTreeMap<_, _>>()
        };
        assert_eq!(scores(&first_run), scores(&second_run));
    }

    #[test]
    fn rejects_unknown_candidate_and_method_and_marks_disabled_analysis_skipped() {
        let mut config = Config::default();
        config.statistics.minimum_observations = 3;
        let csv = sample_csv(5, 42);
        let ingestion = ingest_bytes(csv.as_bytes(), "sample.csv", &config).unwrap();
        assert!(
            analyze(&ingestion, "missing", &[], &config)
                .unwrap_err()
                .to_string()
                .contains("unknown candidate")
        );
        assert!(
            analyze(&ingestion, "candidate_a", &["bogus".into()], &config)
                .unwrap_err()
                .to_string()
                .contains("unknown methods")
        );

        config.anomaly.enabled = false;
        let run = analyze(
            &ingestion,
            "candidate_a",
            &["robust_multivariate".into()],
            &config,
        )
        .unwrap();
        assert_eq!(
            run.statuses["robust_multivariate"].state,
            MethodState::Skipped
        );
    }
}
