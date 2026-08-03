use std::collections::{BTreeMap, HashMap, HashSet};

use sha2::{Digest, Sha256};

use crate::{
    config::{CandidateConfig, Config, ContestSchema},
    error::{AppError, Result},
    model::{
        ExcludedRecord, IngestionResult, PrecinctRecord, Provenance, Severity, ValidationReport,
    },
};

const LEGACY_COLUMNS: [&str; 8] = [
    "County",
    "Precinct",
    "Registered_Dem",
    "Registered_Rep",
    "Votes_Harris",
    "Votes_Trump",
    "Total_Votes",
    "Turnout_Percent",
];

#[derive(Debug)]
struct ParsedRow {
    source_row: usize,
    source: BTreeMap<String, String>,
    record: PrecinctRecord,
    errors: Vec<(String, String, Option<String>)>,
    warnings: Vec<(String, String, Option<String>)>,
}

pub fn ingest_bytes(payload: &[u8], source_name: &str, config: &Config) -> Result<IngestionResult> {
    if payload.is_empty() {
        return Err(AppError::Validation("the CSV is empty".into()));
    }
    if payload.len() > config.data.max_file_size_mb * 1024 * 1024 {
        return Err(AppError::Validation(format!(
            "input is {} bytes; configured maximum is {} MiB",
            payload.len(),
            config.data.max_file_size_mb
        )));
    }

    let mut reader = csv::ReaderBuilder::new()
        .flexible(false)
        .from_reader(payload);
    let headers = reader.headers()?.clone();
    if headers.is_empty() {
        return Err(AppError::Validation("the CSV has no columns".into()));
    }
    let mut seen = HashSet::new();
    let duplicates: Vec<_> = headers
        .iter()
        .filter(|name| !seen.insert((*name).to_owned()))
        .collect();
    if !duplicates.is_empty() {
        return Err(AppError::Validation(format!(
            "duplicate CSV headers are ambiguous: {}",
            duplicates.join(", ")
        )));
    }
    let header_set: HashSet<&str> = headers.iter().collect();
    let configured = &config.data.schema;
    let configured_required = configured
        .candidates
        .iter()
        .map(|candidate| candidate.column.as_str())
        .chain([
            configured.jurisdiction.as_str(),
            configured.precinct.as_str(),
            configured.valid_contest_votes.as_str(),
        ])
        .all(|name| header_set.contains(name));
    let legacy = !configured_required
        && LEGACY_COLUMNS
            .iter()
            .all(|column| header_set.contains(column));
    let schema = if legacy {
        legacy_schema()
    } else {
        configured.clone()
    };
    validate_mapping(&header_set, &schema)?;

    let mut rows = Vec::new();
    for (zero_index, raw) in reader.records().enumerate() {
        let raw = raw?;
        let source_row = zero_index + 2;
        let source: BTreeMap<String, String> = headers
            .iter()
            .zip(raw.iter())
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect();
        rows.push(parse_row(source_row, source, &schema, legacy, config));
    }
    if rows.is_empty() {
        return Err(AppError::Validation("the CSV contains no data rows".into()));
    }

    let mut id_rows: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, row) in rows.iter().enumerate() {
        if !row.record.precinct_id.is_empty() {
            id_rows
                .entry(row.record.precinct_id.clone())
                .or_default()
                .push(index);
        }
    }
    for indices in id_rows.values().filter(|indices| indices.len() > 1) {
        for &index in indices {
            let id = rows[index].record.precinct_id.clone();
            rows[index].errors.push((
                "duplicate_precinct_id".into(),
                format!("duplicate precinct identifier: {id}"),
                None,
            ));
        }
    }

    let mut report = ValidationReport {
        source_name: source_name.to_owned(),
        original_rows: rows.len(),
        ..ValidationReport::default()
    };
    let mut records = Vec::new();
    let mut excluded = Vec::new();
    for row in rows {
        for (code, message, column) in &row.warnings {
            report.add(
                code,
                message,
                Severity::Warning,
                Some(row.source_row),
                column.as_deref(),
                Some(&row.record.precinct_id),
            );
        }
        if row.errors.is_empty() {
            records.push(row.record);
        } else {
            let mut reasons = Vec::new();
            for (code, message, column) in &row.errors {
                report.add(
                    code,
                    message,
                    Severity::Error,
                    Some(row.source_row),
                    column.as_deref(),
                    Some(&row.record.precinct_id),
                );
                if !reasons.contains(code) {
                    reasons.push(code.clone());
                }
            }
            excluded.push(ExcludedRecord {
                source_row: row.source_row,
                source: row.source,
                reasons,
            });
        }
    }
    report.accepted_rows = records.len();
    report.excluded_rows = excluded.len();
    let candidate_labels = schema
        .candidates
        .iter()
        .map(|candidate| (candidate.key.clone(), candidate.label.clone()))
        .collect();
    Ok(IngestionResult {
        records,
        excluded,
        report,
        provenance: Provenance {
            source_name: source_name.to_owned(),
            sha256: hex::encode(Sha256::digest(payload)),
            source_schema: if legacy {
                "legacy_harris_trump".into()
            } else {
                "configured".into()
            },
            source_columns: headers.iter().map(str::to_owned).collect(),
        },
        candidate_labels,
    })
}

fn validate_mapping(headers: &HashSet<&str>, schema: &ContestSchema) -> Result<()> {
    let required = schema
        .candidates
        .iter()
        .map(|candidate| candidate.column.as_str())
        .chain([
            schema.jurisdiction.as_str(),
            schema.precinct.as_str(),
            schema.valid_contest_votes.as_str(),
        ]);
    let missing: Vec<_> = required
        .filter(|column| !headers.contains(column))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(AppError::Validation(format!(
            "mapped columns are absent: {}",
            missing.join(", ")
        )))
    }
}

fn legacy_schema() -> ContestSchema {
    ContestSchema {
        jurisdiction: "County".into(),
        precinct: "Precinct".into(),
        vote_type: None,
        registered_voters: None,
        active_registered_voters: None,
        ballots_cast: Some("Total_Votes".into()),
        valid_contest_votes: "Total_Votes".into(),
        write_in_votes: None,
        undervotes: None,
        overvotes: None,
        latitude: Some("Lat".into()),
        longitude: Some("Lon".into()),
        reported_turnout: Some("Turnout_Percent".into()),
        candidates: vec![
            CandidateConfig {
                column: "Votes_Harris".into(),
                label: "Kamala Harris".into(),
                key: "harris".into(),
            },
            CandidateConfig {
                column: "Votes_Trump".into(),
                label: "Donald Trump".into(),
                key: "trump".into(),
            },
        ],
        down_ballot_pairs: vec![],
        contest_votes_may_exceed_ballots: false,
        ballots_may_exceed_registration: false,
    }
}

fn parse_row(
    source_row: usize,
    source: BTreeMap<String, String>,
    schema: &ContestSchema,
    legacy: bool,
    config: &Config,
) -> ParsedRow {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let jurisdiction = text(&source, &schema.jurisdiction);
    let precinct = text(&source, &schema.precinct);
    if jurisdiction.is_empty() {
        errors.push((
            "missing_identifier".into(),
            "missing jurisdiction".into(),
            Some(schema.jurisdiction.clone()),
        ));
    }
    if precinct.is_empty() {
        errors.push((
            "missing_identifier".into(),
            "missing precinct".into(),
            Some(schema.precinct.clone()),
        ));
    }
    let vote_type = schema.vote_type.as_ref().and_then(|column| {
        let value = text(&source, column);
        (!value.is_empty()).then_some(value)
    });
    let precinct_id = if let Some(kind) = &vote_type {
        format!("{jurisdiction}::{precinct}::{kind}")
    } else {
        format!("{jurisdiction}::{precinct}")
    };

    let mut count = |column: Option<&str>, required: bool| -> Option<u64> {
        let column = column?;
        match parse_u64(source.get(column).map(String::as_str).unwrap_or("")) {
            Ok(Some(value)) => Some(value),
            Ok(None) if !required => None,
            Ok(None) => {
                errors.push((
                    "missing_critical_count".into(),
                    format!("missing count: {column}"),
                    Some(column.into()),
                ));
                None
            }
            Err(message) => {
                errors.push((
                    "invalid_count".into(),
                    format!("{column}: {message}"),
                    Some(column.into()),
                ));
                None
            }
        }
    };
    let valid_contest_votes = count(Some(&schema.valid_contest_votes), true).unwrap_or(0);
    let ballots_required = schema
        .ballots_cast
        .as_ref()
        .is_some_and(|column| source.contains_key(column));
    let ballots_cast = count(schema.ballots_cast.as_deref(), ballots_required);
    let mut registered_voters = count(schema.registered_voters.as_deref(), false);
    let active_registered_voters = count(schema.active_registered_voters.as_deref(), false);
    let write_in_votes = count(schema.write_in_votes.as_deref(), false);
    let undervotes = count(schema.undervotes.as_deref(), false);
    let overvotes = count(schema.overvotes.as_deref(), false);
    let mut candidate_votes = BTreeMap::new();
    for candidate in &schema.candidates {
        let value = count(Some(&candidate.column), true).unwrap_or(0);
        candidate_votes.insert(candidate.key.clone(), value);
    }
    for pair in &schema.down_ballot_pairs {
        if source.contains_key(&pair.down_ballot_column) {
            let _ = count(Some(&pair.down_ballot_column), false);
        }
    }
    if legacy {
        let dem = parse_u64(
            source
                .get("Registered_Dem")
                .map(String::as_str)
                .unwrap_or(""),
        );
        let rep = parse_u64(
            source
                .get("Registered_Rep")
                .map(String::as_str)
                .unwrap_or(""),
        );
        match (dem, rep) {
            (Ok(Some(dem)), Ok(Some(rep))) => {
                registered_voters = dem.checked_add(rep);
                if registered_voters.is_none() {
                    errors.push((
                        "invalid_count".into(),
                        "legacy registration total overflows an unsigned 64-bit count".into(),
                        None,
                    ));
                }
                warnings.push((
                    "legacy_registration_derived".into(),
                    "Registered voters were derived as Democratic plus Republican registration for legacy compatibility; this assumption is not portable".into(),
                    None,
                ));
            }
            _ => errors.push((
                "invalid_count".into(),
                "legacy party registration is invalid".into(),
                None,
            )),
        }
    }

    let candidate_total: u128 = candidate_votes
        .values()
        .map(|value| *value as u128)
        .sum::<u128>()
        + write_in_votes.unwrap_or(0) as u128;
    if candidate_total > valid_contest_votes as u128 {
        errors.push((
            "candidate_votes_exceed_contest".into(),
            "candidate and write-in votes exceed valid contest votes".into(),
            None,
        ));
    }
    if let Some(ballots) = ballots_cast {
        if !schema.contest_votes_may_exceed_ballots && valid_contest_votes > ballots {
            errors.push((
                "contest_votes_exceed_ballots".into(),
                "valid contest votes exceed ballots cast".into(),
                None,
            ));
        }
        if let Some(registered) = registered_voters
            && !schema.ballots_may_exceed_registration
            && ballots > registered
        {
            errors.push((
                "ballots_exceed_registration".into(),
                "ballots cast exceed registered voters".into(),
                None,
            ));
        }
    }

    let latitude = parse_coordinate(
        &source,
        schema.latitude.as_deref(),
        -90.0,
        90.0,
        "latitude",
        &mut warnings,
    );
    let longitude = parse_coordinate(
        &source,
        schema.longitude.as_deref(),
        -180.0,
        180.0,
        "longitude",
        &mut warnings,
    );
    let reported_turnout_percent = schema.reported_turnout.as_ref().and_then(|column| {
        let raw = source.get(column).map(String::as_str).unwrap_or("").trim();
        if raw.is_empty() {
            return None;
        }
        match raw.parse::<f64>() {
            Ok(value) if value.is_finite() && (0.0..=100.0).contains(&value) => Some(value),
            _ => {
                errors.push((
                    "invalid_reported_turnout".into(),
                    "reported turnout must be numeric and in [0, 100]".into(),
                    Some(column.clone()),
                ));
                None
            }
        }
    });
    let calculated_turnout_percent = match (ballots_cast, registered_voters) {
        (Some(ballots), Some(registered)) if registered > 0 => {
            Some(100.0 * ballots as f64 / registered as f64)
        }
        _ => None,
    };
    if let (Some(reported), Some(calculated)) =
        (reported_turnout_percent, calculated_turnout_percent)
        && (reported - calculated).abs() > config.data.turnout_tolerance_percentage_points
    {
        warnings.push((
            "turnout_mismatch".into(),
            "reported and calculated turnout differ beyond the configured tolerance".into(),
            None,
        ));
    }
    let candidate_shares = candidate_votes
        .iter()
        .map(|(key, votes)| {
            (
                key.clone(),
                (valid_contest_votes > 0).then_some(*votes as f64 / valid_contest_votes as f64),
            )
        })
        .collect();

    ParsedRow {
        source_row,
        source: source.clone(),
        record: PrecinctRecord {
            source,
            source_row,
            jurisdiction,
            precinct,
            vote_type,
            precinct_id,
            registered_voters,
            active_registered_voters,
            ballots_cast,
            valid_contest_votes,
            candidate_votes,
            candidate_shares,
            write_in_votes,
            undervotes,
            overvotes,
            latitude,
            longitude,
            reported_turnout_percent,
            calculated_turnout_percent,
        },
        errors,
        warnings,
    }
}

fn text(source: &BTreeMap<String, String>, column: &str) -> String {
    source
        .get(column)
        .map_or("", String::as_str)
        .trim()
        .to_owned()
}

fn parse_u64(raw: &str) -> std::result::Result<Option<u64>, String> {
    let value = raw.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if let Ok(integer) = value.parse::<u64>() {
        return Ok(Some(integer));
    }
    match value.parse::<f64>() {
        Ok(number) if number.is_finite() && number < 0.0 => Err("count is negative".into()),
        Ok(number) if number.is_finite() && number.fract() != 0.0 => {
            Err("count is not an integer".into())
        }
        _ => Err("count is not a nonnegative integer".into()),
    }
}

fn parse_coordinate(
    source: &BTreeMap<String, String>,
    column: Option<&str>,
    low: f64,
    high: f64,
    label: &str,
    warnings: &mut Vec<(String, String, Option<String>)>,
) -> Option<f64> {
    let column = column?;
    let raw = source.get(column).map(String::as_str).unwrap_or("").trim();
    if raw.is_empty() {
        warnings.push((
            "invalid_or_missing_coordinate".into(),
            format!("{label} is missing; spatial analysis will exclude this row"),
            Some(column.into()),
        ));
        return None;
    }
    match raw.parse::<f64>() {
        Ok(value) if value.is_finite() && (low..=high).contains(&value) => Some(value),
        _ => {
            warnings.push((
                "invalid_or_missing_coordinate".into(),
                format!(
                    "{label} is outside [{low}, {high}]; spatial analysis will exclude this row"
                ),
                Some(column.into()),
            ));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_csv() -> Vec<u8> {
        b"Jurisdiction,Precinct,Registered_Voters,Ballots_Cast,Valid_Contest_Votes,Votes_Candidate_A,Votes_Candidate_B,Latitude,Longitude,Reported_Turnout_Percent\nA,1,100,80,78,40,38,42,-85,80\nA,2,100,70,69,30,39,42.1,-85.1,70\n".to_vec()
    }

    #[test]
    fn accepts_generalized_and_preserves_source() {
        let result = ingest_bytes(&valid_csv(), "memory.csv", &Config::default()).unwrap();
        assert_eq!(result.records.len(), 2);
        assert_eq!(result.records[0].source["Jurisdiction"], "A");
        assert_eq!(result.provenance.source_schema, "configured");
        assert_eq!(result.records[0].calculated_turnout_percent, Some(80.0));
    }

    #[test]
    fn excludes_invalid_and_duplicate_rows() {
        let csv = b"Jurisdiction,Precinct,Valid_Contest_Votes,Votes_Candidate_A,Votes_Candidate_B\nA,1,10,11,0\nA,1,10,5,5\n";
        let result = ingest_bytes(csv, "bad.csv", &Config::default()).unwrap();
        assert!(result.records.is_empty());
        assert_eq!(result.excluded.len(), 2);
        assert!(result.report.errors() >= 3);
    }

    #[test]
    fn rejects_duplicate_headers() {
        let csv = b"Jurisdiction,Precinct,Valid_Contest_Votes,Votes_Candidate_A,Votes_Candidate_A,Votes_Candidate_B\nA,1,10,5,5,5\n";
        assert!(ingest_bytes(csv, "bad.csv", &Config::default()).is_err());
    }

    #[test]
    fn legacy_adapter_is_explicit() {
        let csv = b"County,Precinct,Registered_Dem,Registered_Rep,Votes_Harris,Votes_Trump,Total_Votes,Turnout_Percent\nA,1,40,60,45,35,80,80\n";
        let result = ingest_bytes(csv, "legacy.csv", &Config::default()).unwrap();
        assert_eq!(result.provenance.source_schema, "legacy_harris_trump");
        assert_eq!(result.records[0].registered_voters, Some(100));
    }

    #[test]
    fn configured_schema_wins_for_hybrid_input() {
        let csv = b"Jurisdiction,County,Precinct,Valid_Contest_Votes,Votes_Candidate_A,Votes_Candidate_B,Registered_Dem,Registered_Rep,Votes_Harris,Votes_Trump,Total_Votes,Turnout_Percent\nConfigured,Legacy,1,10,4,6,40,60,45,35,80,80\n";
        let result = ingest_bytes(csv, "hybrid.csv", &Config::default()).unwrap();
        assert_eq!(result.provenance.source_schema, "configured");
        assert_eq!(result.records[0].jurisdiction, "Configured");
    }

    #[test]
    fn malformed_optional_down_ballot_count_excludes_row() {
        let csv = b"Jurisdiction,Precinct,Valid_Contest_Votes,Votes_Candidate_A,Votes_Candidate_B,Votes_Down_Ballot_A\nA,1,10,4,6,not-a-count\n";
        let result = ingest_bytes(csv, "bad-down-ballot.csv", &Config::default()).unwrap();
        assert!(result.records.is_empty());
        assert!(result.excluded[0].reasons.contains(&"invalid_count".into()));
    }

    #[test]
    fn rejects_empty_oversized_header_only_and_unmapped_inputs() {
        let config = Config::default();
        assert!(ingest_bytes(b"", "empty.csv", &config).is_err());
        assert!(ingest_bytes(b"\n", "no-columns.csv", &config).is_err());
        assert!(
            ingest_bytes(
                b"Jurisdiction,Precinct,Valid_Contest_Votes,Votes_Candidate_A,Votes_Candidate_B\n",
                "headers.csv",
                &config
            )
            .is_err()
        );
        assert!(ingest_bytes(b"wrong,columns\n1,2\n", "wrong.csv", &config).is_err());

        let mut tiny = Config::default();
        tiny.data.max_file_size_mb = 0;
        assert!(ingest_bytes(b"x", "large.csv", &tiny).is_err());
    }

    #[test]
    fn reports_missing_identifiers_vote_types_and_distinct_count_errors() {
        let mut config = Config::default();
        config.data.schema.vote_type = Some("Vote_Type".into());
        let csv = b"Jurisdiction,Precinct,Vote_Type,Valid_Contest_Votes,Votes_Candidate_A,Votes_Candidate_B\n,1,Mail,10,-1,11\nA,,Election Day,10,1.5,nope\n";
        let result = ingest_bytes(csv, "invalid.csv", &config).unwrap();
        assert!(result.records.is_empty());
        assert!(
            result
                .excluded
                .iter()
                .all(|row| row.reasons.contains(&"missing_identifier".into()))
        );
        assert!(
            result
                .report
                .issues
                .iter()
                .any(|issue| issue.message.contains("negative"))
        );
        assert!(
            result
                .report
                .issues
                .iter()
                .any(|issue| issue.message.contains("not an integer"))
        );
        assert!(
            result
                .report
                .issues
                .iter()
                .any(|issue| issue.message.contains("nonnegative integer"))
        );
    }

    #[test]
    fn legacy_invalid_and_overflow_registration_are_excluded() {
        let csv = b"County,Precinct,Registered_Dem,Registered_Rep,Votes_Harris,Votes_Trump,Total_Votes,Turnout_Percent\nA,1,bad,10,4,6,10,50\nA,2,18446744073709551615,1,4,6,10,50\n";
        let result = ingest_bytes(csv, "legacy.csv", &Config::default()).unwrap();
        assert!(result.records.is_empty());
        assert!(
            result
                .excluded
                .iter()
                .all(|row| row.reasons.contains(&"invalid_count".into()))
        );
    }

    #[test]
    fn turnout_coordinate_and_zero_denominator_states_are_explicit() {
        let csv = b"Jurisdiction,Precinct,Registered_Voters,Ballots_Cast,Valid_Contest_Votes,Votes_Candidate_A,Votes_Candidate_B,Latitude,Longitude,Reported_Turnout_Percent\nA,1,100,80,78,40,38,,200,10\nA,2,0,0,0,0,0,42,-85,\nA,3,100,70,69,30,39,42.1,-85.1,70\nA,4,100,70,69,30,39,42.1,-85.1,invalid\n";
        let result = ingest_bytes(csv, "quality.csv", &Config::default()).unwrap();
        assert_eq!(result.records.len(), 3);
        assert_eq!(result.excluded.len(), 1);
        assert!(
            result
                .report
                .issues
                .iter()
                .filter(|issue| issue.severity == Severity::Warning)
                .count()
                >= 3
        );
        assert_eq!(result.records[1].candidate_shares["candidate_a"], None);
        assert_eq!(result.records[1].calculated_turnout_percent, None);
        assert!(
            result
                .report
                .issues
                .iter()
                .any(|issue| issue.code == "turnout_mismatch")
        );
    }

    #[test]
    fn schema_allowances_permit_documented_total_relationships() {
        let mut config = Config::default();
        config.data.schema.contest_votes_may_exceed_ballots = true;
        config.data.schema.ballots_may_exceed_registration = true;
        let csv = b"Jurisdiction,Precinct,Registered_Voters,Ballots_Cast,Valid_Contest_Votes,Votes_Candidate_A,Votes_Candidate_B\nA,1,5,10,11,5,6\n";
        let result = ingest_bytes(csv, "allowed.csv", &config).unwrap();
        assert_eq!(result.records.len(), 1);
    }
}
