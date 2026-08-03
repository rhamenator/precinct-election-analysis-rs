use precinct_election_analysis_rs::{config::Config, ingestion::ingest_bytes, workflow};

#[test]
fn ingests_python_generalized_contract() {
    let payload = include_bytes!("fixtures/python_generalized.csv");
    let mut config = Config::default();
    config.statistics.minimum_observations = 3;
    config.statistics.spatial_permutations = 19;
    let ingestion = ingest_bytes(payload, "python_generalized.csv", &config).unwrap();
    assert_eq!(ingestion.records.len(), 5);
    assert_eq!(ingestion.provenance.source_schema, "configured");
    let run = workflow::analyze(
        &ingestion,
        "candidate_a",
        &[
            "vote_share_by_count".into(),
            "down_ballot_difference".into(),
            "spatial".into(),
        ],
        &config,
    )
    .unwrap();
    assert!(run.statuses.values().all(|status| {
        status.state == precinct_election_analysis_rs::model::MethodState::Successful
    }));
}
