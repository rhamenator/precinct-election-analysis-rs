use std::io::{Cursor, Write};

use zip::{ZipWriter, write::SimpleFileOptions};

use crate::{
    error::Result,
    model::{AnalysisRun, IngestionResult},
};

pub fn build_bundle(ingestion: &IngestionResult, run: &AnalysisRun) -> Result<Vec<u8>> {
    let cursor = Cursor::new(Vec::new());
    let mut archive = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    archive.start_file("validated_input.csv", options)?;
    archive.write_all(validated_csv(ingestion).as_bytes())?;
    archive.start_file("analysis_results.csv", options)?;
    archive.write_all(analysis_csv(run).as_bytes())?;
    archive.start_file("excluded_records.json", options)?;
    archive.write_all(serde_json::to_vec_pretty(&ingestion.excluded)?.as_slice())?;
    archive.start_file("validation_report.json", options)?;
    archive.write_all(serde_json::to_vec_pretty(&ingestion.report)?.as_slice())?;
    archive.start_file("run.json", options)?;
    archive.write_all(serde_json::to_vec_pretty(run)?.as_slice())?;
    archive.start_file("report.md", options)?;
    archive.write_all(markdown_report(run).as_bytes())?;
    Ok(archive.finish()?.into_inner())
}

fn validated_csv(ingestion: &IngestionResult) -> String {
    let mut writer = csv::Writer::from_writer(Vec::new());
    if let Some(first) = ingestion.records.first() {
        let headers: Vec<_> = first.source.keys().cloned().collect();
        let _ = writer.write_record(&headers);
        for record in &ingestion.records {
            let row: Vec<_> = headers
                .iter()
                .map(|header| record.source.get(header).cloned().unwrap_or_default())
                .collect();
            let _ = writer.write_record(row);
        }
    }
    String::from_utf8(writer.into_inner().unwrap_or_default()).unwrap_or_default()
}

fn analysis_csv(run: &AnalysisRun) -> String {
    let mut metric_names = std::collections::BTreeSet::new();
    for record in &run.records {
        metric_names.extend(record.metrics.keys().cloned());
    }
    let metric_names: Vec<_> = metric_names.into_iter().collect();
    let mut writer = csv::Writer::from_writer(Vec::new());
    let mut headers = vec!["Precinct_ID", "Jurisdiction", "Precinct", "Candidate_Share"];
    headers.extend(metric_names.iter().map(String::as_str));
    let _ = writer.write_record(headers);
    for record in &run.records {
        let mut row = vec![
            record.precinct_id.clone(),
            record.jurisdiction.clone(),
            record.precinct.clone(),
            record
                .candidate_share
                .map(|value| value.to_string())
                .unwrap_or_default(),
        ];
        row.extend(metric_names.iter().map(|name| {
            record
                .metrics
                .get(name)
                .map(|value| match value {
                    serde_json::Value::String(text) => text.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_default()
        }));
        let _ = writer.write_record(row);
    }
    String::from_utf8(writer.into_inner().unwrap_or_default()).unwrap_or_default()
}

pub fn markdown_report(run: &AnalysisRun) -> String {
    let mut report = format!(
        "# Precinct election analysis run\n\n{}\n\nCandidate: {}\nAnalysis rows: {}\nExcluded rows: {}\n\n## Method status\n\n",
        run.interpretation_warning, run.candidate_label, run.analysis_rows, run.excluded_rows
    );
    for (method, status) in &run.statuses {
        report.push_str(&format!(
            "- {method}: {:?} — {}\n",
            status.state, status.message
        ));
    }
    report.push_str("\n## Interpretation\n\nFlags identify observations unusual under a particular exploratory model and require source-data review and context. This workflow is not a ballot audit.\n");
    report
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use crate::{config::Config, ingestion::ingest_bytes, sample::sample_csv, workflow};

    use super::*;

    #[test]
    fn bundle_contains_reproducibility_artifacts() {
        let mut config = Config::default();
        config.statistics.minimum_observations = 5;
        config.statistics.spatial_permutations = 19;
        let sample = sample_csv(20, 42);
        let ingestion = ingest_bytes(sample.as_bytes(), "sample.csv", &config).unwrap();
        let run = workflow::analyze(
            &ingestion,
            "candidate_a",
            &["turnout_share".into(), "robust_multivariate".into()],
            &config,
        )
        .unwrap();
        let bytes = build_bundle(&ingestion, &run).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert!(archive.by_name("run.json").is_ok());
        let mut report = String::new();
        archive
            .by_name("report.md")
            .unwrap()
            .read_to_string(&mut report)
            .unwrap();
        assert!(report.contains("not proof"));
    }
}
