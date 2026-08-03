use std::collections::BTreeMap;

use rand::{SeedableRng, seq::SliceRandom};
use rand_chacha::ChaCha8Rng;
use serde_json::{Value, json};
use statrs::distribution::{ChiSquared, ContinuousCDF};

use crate::{
    config::Config,
    error::{AppError, Result},
    model::{AnalysisRecord, PrecinctRecord},
};

pub fn turnout_share(
    source: &[PrecinctRecord],
    output: &mut [AnalysisRecord],
    candidate: &str,
    config: &Config,
) -> Result<Value> {
    let mut valid: Vec<(usize, f64, f64)> = source
        .iter()
        .enumerate()
        .filter_map(|(index, record)| {
            let turnout = record
                .calculated_turnout_percent
                .or(record.reported_turnout_percent)?;
            let share = record.candidate_shares.get(candidate).copied().flatten()?;
            (turnout.is_finite() && share.is_finite() && record.valid_contest_votes > 0)
                .then_some((index, turnout, share))
        })
        .collect();
    let minimum = config.statistics.minimum_observations;
    if valid.len() < minimum {
        return Err(AppError::Unavailable(format!(
            "need at least {minimum} valid turnout/share observations; found {}",
            valid.len()
        )));
    }
    let valid_count = valid.len();
    let mut turnouts: Vec<f64> = valid.iter().map(|(_, turnout, _)| *turnout).collect();
    turnouts.sort_by(f64::total_cmp);
    let quantile_index = ((turnouts.len() - 1) as f64 * config.statistics.baseline_turnout_quantile)
        .floor() as usize;
    let baseline_limit = turnouts[quantile_index];
    let reference: Vec<(usize, f64, f64)> = valid
        .iter()
        .copied()
        .filter(|(_, turnout, _)| *turnout <= baseline_limit)
        .collect();
    if reference.len() < minimum {
        return Err(AppError::Unavailable(
            "too few observations remain in the baseline turnout range".into(),
        ));
    }
    let x_mean = reference.iter().map(|(_, x, _)| x).sum::<f64>() / reference.len() as f64;
    let y_mean = reference.iter().map(|(_, _, y)| y).sum::<f64>() / reference.len() as f64;
    let sxx = reference
        .iter()
        .map(|(_, x, _)| (x - x_mean).powi(2))
        .sum::<f64>();
    if sxx <= f64::EPSILON {
        return Err(AppError::Unavailable("turnout has zero variance".into()));
    }
    let slope = reference
        .iter()
        .map(|(_, x, y)| (x - x_mean) * (y - y_mean))
        .sum::<f64>()
        / sxx;
    let intercept = y_mean - slope * x_mean;
    let residual_sum = reference
        .iter()
        .map(|(_, x, y)| (y - (intercept + slope * x)).powi(2))
        .sum::<f64>();
    let residual_df = reference.len().saturating_sub(2);
    if residual_df == 0 {
        return Err(AppError::Unavailable(
            "residual degrees of freedom are zero".into(),
        ));
    }
    let mse = residual_sum / residual_df as f64;
    if !mse.is_finite() || mse <= f64::EPSILON {
        return Err(AppError::Unavailable("residual variance is zero".into()));
    }
    let reference_indices: std::collections::HashSet<usize> =
        reference.iter().map(|(index, _, _)| *index).collect();
    let mut flagged = 0;
    for (index, x, y) in valid.drain(..) {
        let fitted = intercept + slope * x;
        let residual = y - fitted;
        let leverage = 1.0 / reference.len() as f64 + (x - x_mean).powi(2) / sxx;
        let (loo_residual, multiplier) = if reference_indices.contains(&index) {
            let one_minus_h = (1.0 - leverage).max(f64::EPSILON);
            (residual / one_minus_h, 1.0 + leverage / one_minus_h)
        } else {
            (residual, 1.0 + leverage)
        };
        let expected = y - loo_residual;
        let standardized = loo_residual / (mse * multiplier).sqrt();
        let is_flagged = standardized.abs() > config.statistics.studentized_residual_threshold;
        flagged += usize::from(is_flagged);
        output[index]
            .metrics
            .insert("turnout_expected_share".into(), json!(expected));
        output[index]
            .metrics
            .insert("turnout_studentized_residual".into(), json!(standardized));
        output[index]
            .metrics
            .insert("turnout_share_flag".into(), json!(is_flagged));
    }
    Ok(json!({
        "model": "leave-one-out ordinary least-squares linear regression",
        "turnout_denominator": "calculated turnout when available, otherwise mapped reported turnout",
        "candidate_share_denominator": "valid contest votes",
        "baseline_turnout_quantile": config.statistics.baseline_turnout_quantile,
        "baseline_turnout_upper_limit": baseline_limit,
        "baseline_observations": reference.len(),
        "valid_observations": valid_count,
        "studentized_residual_threshold": config.statistics.studentized_residual_threshold,
        "flagged_observations": flagged,
        "limitation": "Exploratory model only; ordinary jurisdiction and demographic heterogeneity can create large residuals."
    }))
}

pub fn vote_share_by_count(
    source: &[PrecinctRecord],
    output: &mut [AnalysisRecord],
    candidate: &str,
) -> Result<Value> {
    let valid: Vec<(usize, f64, f64)> = source
        .iter()
        .enumerate()
        .filter_map(|(index, record)| {
            Some((
                index,
                *record.candidate_votes.get(candidate)? as f64,
                record.candidate_shares.get(candidate).copied().flatten()?,
            ))
        })
        .collect();
    if valid.len() < 3 {
        return Err(AppError::Unavailable(
            "at least three valid vote-count/share observations are required".into(),
        ));
    }
    let x_mean = valid.iter().map(|(_, x, _)| x).sum::<f64>() / valid.len() as f64;
    let y_mean = valid.iter().map(|(_, _, y)| y).sum::<f64>() / valid.len() as f64;
    let sxx = valid
        .iter()
        .map(|(_, x, _)| (x - x_mean).powi(2))
        .sum::<f64>();
    if sxx <= f64::EPSILON {
        return Err(AppError::Unavailable(
            "candidate vote counts have no variation".into(),
        ));
    }
    let slope = valid
        .iter()
        .map(|(_, x, y)| (x - x_mean) * (y - y_mean))
        .sum::<f64>()
        / sxx;
    let intercept = y_mean - slope * x_mean;
    let mut sse = 0.0;
    let syy = valid
        .iter()
        .map(|(_, _, y)| (y - y_mean).powi(2))
        .sum::<f64>();
    for (index, x, y) in &valid {
        let expected = intercept + slope * x;
        let residual = y - expected;
        sse += residual.powi(2);
        output[*index]
            .metrics
            .insert("vote_count_expected_share".into(), json!(expected));
        output[*index]
            .metrics
            .insert("vote_count_trend_residual".into(), json!(residual));
    }
    Ok(json!({
        "model": "ordinary least-squares linear descriptive trend",
        "valid_observations": valid.len(),
        "slope_share_per_100_votes": slope * 100.0,
        "intercept": intercept,
        "r_squared": if syy > 0.0 { 1.0 - sse / syy } else { 0.0 },
        "scope": "descriptive relationship; this method creates no anomaly flag",
        "limitation": "A slope can result from geography, demographics, precinct design, or vote type."
    }))
}

pub fn down_ballot_difference(
    source: &[PrecinctRecord],
    output: &mut [AnalysisRecord],
    candidate: &str,
    config: &Config,
) -> Result<Value> {
    let pairs: Vec<_> = config
        .data
        .schema
        .down_ballot_pairs
        .iter()
        .filter(|pair| pair.presidential_candidate_key == candidate)
        .collect();
    if pairs.is_empty() {
        return Err(AppError::Unavailable(
            "no configured down-ballot comparison is available".into(),
        ));
    }
    let mut summaries = Vec::new();
    for pair in pairs {
        let mut values = Vec::new();
        let mut total_presidential = 0_u128;
        let mut total_down = 0_u128;
        for (index, record) in source.iter().enumerate() {
            let Some(&presidential) = record.candidate_votes.get(candidate) else {
                continue;
            };
            let Some(raw) = record.source.get(&pair.down_ballot_column) else {
                continue;
            };
            let Ok(down) = raw.trim().parse::<u64>() else {
                continue;
            };
            if presidential == 0 {
                continue;
            }
            let difference = presidential as i128 - down as i128;
            let percent = 100.0 * difference as f64 / presidential as f64;
            output[index].metrics.insert(
                format!("down_ballot_difference_votes__{}", pair.key),
                json!(difference),
            );
            output[index].metrics.insert(
                format!("down_ballot_difference_percent__{}", pair.key),
                json!(percent),
            );
            values.push(percent);
            total_presidential += presidential as u128;
            total_down += down as u128;
        }
        if !values.is_empty() {
            values.sort_by(f64::total_cmp);
            summaries.push(json!({
                "key": pair.key,
                "label": pair.label,
                "status": "successful",
                "valid_observations": values.len(),
                "aggregate_difference_votes": total_presidential as i128 - total_down as i128,
                "aggregate_difference_percent_of_presidential": 100.0 * (total_presidential as f64 - total_down as f64) / total_presidential as f64,
                "median_precinct_difference_percent": median(&values),
                "negative_difference_precincts": values.iter().filter(|value| **value < 0.0).count()
            }));
        }
    }
    if summaries.is_empty() {
        return Err(AppError::Unavailable(
            "no valid positive presidential counts support a comparison".into(),
        ));
    }
    Ok(json!({
        "definition": "100 * (presidential votes - same-party down-ballot votes) / presidential votes",
        "comparisons": summaries,
        "scope": "descriptive comparison; this method creates no anomaly flag",
        "limitation": "Roll-off, split-ticket voting, candidate effects, eligibility, and vote-type composition can produce differences."
    }))
}

pub fn digit_diagnostics(source: &[PrecinctRecord], config: &Config) -> Result<Value> {
    let candidates = &config.data.schema.candidates;
    let chi = ChiSquared::new(9.0).map_err(|error| AppError::Unavailable(error.to_string()))?;
    let mut unavailable = Vec::new();
    let mut successful = Vec::new();
    for candidate in candidates {
        let values: Vec<u64> = source
            .iter()
            .filter_map(|record| record.candidate_votes.get(&candidate.key).copied())
            .collect();
        if values.len() < config.statistics.minimum_observations {
            unavailable.push(json!({"candidate": candidate.key, "status": "unavailable", "reason": "insufficient observations"}));
            continue;
        }
        let mut counts = [0_usize; 10];
        for value in &values {
            counts[(value % 10) as usize] += 1;
        }
        let expected = values.len() as f64 / 10.0;
        let statistic = counts
            .iter()
            .map(|count| (*count as f64 - expected).powi(2) / expected)
            .sum::<f64>();
        successful.push((
            candidate.key.clone(),
            statistic,
            1.0 - chi.cdf(statistic),
            values.len(),
        ));
    }
    if successful.is_empty() {
        return Err(AppError::Unavailable(
            "no digit diagnostic met its sample-size precondition".into(),
        ));
    }
    let adjusted = holm_adjust(&successful.iter().map(|item| item.2).collect::<Vec<_>>());
    let mut tests = unavailable;
    tests.extend(successful.into_iter().zip(adjusted).map(
        |((candidate, statistic, p_value, sample_size), adjusted_p_value)| {
            json!({
                "candidate": candidate,
                "status": "successful",
                "test": "last_digit_uniformity",
                "statistic": statistic,
                "p_value": p_value,
                "adjusted_p_value": adjusted_p_value,
                "significant": adjusted_p_value < config.statistics.alpha,
                "sample_size": sample_size,
                "expected": "uniform digits 0-9"
            })
        },
    ));
    Ok(json!({
        "tests": tests,
        "correction": "Holm family-wise error correction across successful digit tests",
        "scope": "dataset-level diagnostic; values are not precinct-level anomaly scores",
        "limitation": "Digit patterns can reflect reporting rules, precinct sizes, and administrative processes."
    }))
}

pub fn spatial(
    source: &[PrecinctRecord],
    output: &mut [AnalysisRecord],
    candidate: &str,
    config: &Config,
) -> Result<Value> {
    let valid: Vec<(usize, f64, f64, f64)> = source
        .iter()
        .enumerate()
        .filter_map(|(index, record)| {
            Some((
                index,
                record.longitude?,
                record.latitude?,
                record.candidate_shares.get(candidate).copied().flatten()?,
            ))
        })
        .collect();
    if valid.len() < 3 {
        return Err(AppError::Unavailable(
            "at least three records with coordinates and candidate shares are required".into(),
        ));
    }
    let n = valid.len();
    let k = config.statistics.spatial_neighbors.min(n - 1);
    let mut weights = vec![vec![0.0; n]; n];
    for i in 0..n {
        let mut distances: Vec<(usize, f64)> = (0..n)
            .filter(|j| *j != i)
            .map(|j| {
                let dx = valid[i].1 - valid[j].1;
                let dy = valid[i].2 - valid[j].2;
                (j, dx * dx + dy * dy)
            })
            .collect();
        distances.sort_by(|left, right| left.1.total_cmp(&right.1).then(left.0.cmp(&right.0)));
        for (j, _) in distances.into_iter().take(k) {
            weights[i][j] = 1.0 / k as f64;
        }
    }
    let values: Vec<f64> = valid.iter().map(|item| item.3).collect();
    let mean = values.iter().sum::<f64>() / n as f64;
    let centered: Vec<f64> = values.iter().map(|value| value - mean).collect();
    let denominator = centered.iter().map(|value| value * value).sum::<f64>();
    if denominator <= f64::EPSILON {
        return Err(AppError::Unavailable(
            "spatial variable has zero variance".into(),
        ));
    }
    let lag = matrix_vector(&weights, &centered);
    let global_i = dot(&centered, &lag) / denominator;
    let local: Vec<f64> = centered
        .iter()
        .zip(&lag)
        .map(|(value, lag)| value * lag / (denominator / n as f64))
        .collect();
    let permutations = config.statistics.spatial_permutations;
    let mut rng = ChaCha8Rng::seed_from_u64(config.statistics.random_seed);
    let mut global_extreme = 0_usize;
    let mut local_extreme = vec![0_usize; n];
    let mut permuted = centered.clone();
    for _ in 0..permutations {
        permuted.shuffle(&mut rng);
        let permuted_lag = matrix_vector(&weights, &permuted);
        let permuted_global = dot(&permuted, &permuted_lag) / denominator;
        global_extreme += usize::from(permuted_global.abs() >= global_i.abs());
        for i in 0..n {
            let permuted_local = centered[i] * permuted_lag[i] / (denominator / n as f64);
            local_extreme[i] += usize::from(permuted_local.abs() >= local[i].abs());
        }
    }
    let raw_p: Vec<f64> = local_extreme
        .iter()
        .map(|count| (*count + 1) as f64 / (permutations + 1) as f64)
        .collect();
    let adjusted = benjamini_hochberg(&raw_p);
    let mut significant = 0;
    for (position, (source_index, _, _, _)) in valid.iter().enumerate() {
        let pattern = match (centered[position] >= 0.0, lag[position] >= 0.0) {
            (true, true) => "high-high cluster",
            (false, false) => "low-low cluster",
            (true, false) => "high-low spatial outlier",
            (false, true) => "low-high spatial outlier",
        };
        let is_significant = adjusted[position] < config.statistics.alpha;
        significant += usize::from(is_significant);
        output[*source_index]
            .metrics
            .insert("local_moran_i".into(), json!(local[position]));
        output[*source_index]
            .metrics
            .insert("local_moran_adjusted_p".into(), json!(adjusted[position]));
        output[*source_index]
            .metrics
            .insert("spatial_pattern".into(), json!(pattern));
        output[*source_index]
            .metrics
            .insert("spatial_significant".into(), json!(is_significant));
    }
    Ok(json!({
        "weights": format!("K-nearest-neighbor fallback (k={k})"),
        "valid_observations": n,
        "excluded_for_missing_coordinates_or_value": source.len() - n,
        "global_moran_i": global_i,
        "global_expected_i": -1.0 / (n - 1) as f64,
        "global_permutation_p": (global_extreme + 1) as f64 / (permutations + 1) as f64,
        "permutations": permutations,
        "local_correction": "Benjamini-Hochberg false-discovery-rate correction",
        "significant_local_patterns": significant,
        "limitation": "Spatial association may reflect ordinary geographic political clustering."
    }))
}

pub fn robust_multivariate(
    source: &[PrecinctRecord],
    output: &mut [AnalysisRecord],
    candidate: &str,
    config: &Config,
) -> Result<Value> {
    if !config.anomaly.enabled {
        return Err(AppError::Unavailable("disabled by configuration".into()));
    }
    let rows: Vec<(usize, Vec<f64>)> = source
        .iter()
        .enumerate()
        .filter_map(|(index, record)| {
            let turnout = record
                .calculated_turnout_percent
                .or(record.reported_turnout_percent)?
                / 100.0;
            let share = record.candidate_shares.get(candidate).copied().flatten()?;
            let ballots = record.ballots_cast? as f64;
            let contest = record.valid_contest_votes as f64;
            Some((
                index,
                vec![
                    turnout,
                    share,
                    (ballots + 1.0).ln(),
                    (contest + 1.0).ln(),
                    if ballots > 0.0 {
                        contest / ballots
                    } else {
                        0.0
                    },
                ],
            ))
        })
        .collect();
    if rows.len() < config.statistics.minimum_observations {
        return Err(AppError::Unavailable(format!(
            "need at least {} complete observations for robust multivariate scoring; found {}",
            config.statistics.minimum_observations,
            rows.len()
        )));
    }
    let width = rows[0].1.len();
    let mut medians = vec![0.0; width];
    let mut mads = vec![0.0; width];
    for column in 0..width {
        let mut values: Vec<f64> = rows.iter().map(|(_, row)| row[column]).collect();
        values.sort_by(f64::total_cmp);
        medians[column] = median(&values);
        let mut deviations: Vec<f64> = values
            .iter()
            .map(|value| (value - medians[column]).abs())
            .collect();
        deviations.sort_by(f64::total_cmp);
        mads[column] = median(&deviations);
    }
    let mut flagged = 0;
    for (index, row) in rows {
        let usable: Vec<f64> = row
            .iter()
            .enumerate()
            .filter(|(column, _)| mads[*column] > f64::EPSILON)
            .map(|(column, value)| {
                ((value - medians[column]).abs() / (1.4826 * mads[column])).min(12.0)
            })
            .collect();
        if usable.is_empty() {
            continue;
        }
        let mean_robust_z = usable.iter().sum::<f64>() / usable.len() as f64;
        let score = 1.0 - (-mean_robust_z / 3.0).exp();
        let is_flagged = score >= config.anomaly.flag_threshold;
        flagged += usize::from(is_flagged);
        output[index]
            .metrics
            .insert("robust_anomaly_score".into(), json!(score));
        output[index]
            .metrics
            .insert("robust_anomaly_flag".into(), json!(is_flagged));
    }
    Ok(json!({
        "model": "deterministic median/MAD multivariate distance",
        "features": ["turnout_fraction", "candidate_share", "log_ballots", "log_valid_contest_votes", "contest_vote_rate"],
        "score_scale": "1 - exp(-mean absolute robust z / 3), bounded to [0, 1]",
        "flag_threshold": config.anomaly.flag_threshold,
        "flagged_observations": flagged,
        "limitation": "This is an unsupervised ranking aid, not a probability of fraud or error."
    }))
}

fn median(sorted: &[f64]) -> f64 {
    let middle = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    }
}

fn matrix_vector(matrix: &[Vec<f64>], vector: &[f64]) -> Vec<f64> {
    matrix.iter().map(|row| dot(row, vector)).collect()
}

fn dot(left: &[f64], right: &[f64]) -> f64 {
    left.iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .sum()
}

pub fn benjamini_hochberg(values: &[f64]) -> Vec<f64> {
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|left, right| values[*left].total_cmp(&values[*right]));
    let mut adjusted = vec![1.0_f64; values.len()];
    let mut running = 1.0_f64;
    for (reverse_rank, &index) in order.iter().enumerate().rev() {
        let rank = reverse_rank + 1;
        running = running.min(values[index] * values.len() as f64 / rank as f64);
        adjusted[index] = running.min(1.0);
    }
    adjusted
}

pub fn holm_adjust(values: &[f64]) -> Vec<f64> {
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|left, right| values[*left].total_cmp(&values[*right]));
    let mut adjusted = vec![1.0_f64; values.len()];
    let mut running = 0.0_f64;
    for (rank, &index) in order.iter().enumerate() {
        running = running.max(values[index] * (values.len() - rank) as f64);
        adjusted[index] = running.min(1.0);
    }
    adjusted
}

pub fn empty_metrics(source: &[PrecinctRecord], candidate: &str) -> Vec<AnalysisRecord> {
    source
        .iter()
        .map(|record| AnalysisRecord {
            precinct_id: record.precinct_id.clone(),
            jurisdiction: record.jurisdiction.clone(),
            precinct: record.precinct.clone(),
            candidate_share: record.candidate_shares.get(candidate).copied().flatten(),
            metrics: BTreeMap::new(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{ingestion::ingest_bytes, sample::sample_csv};

    use super::*;

    fn records(rows: usize) -> (Config, Vec<PrecinctRecord>, Vec<AnalysisRecord>) {
        let mut config = Config::default();
        config.statistics.minimum_observations = 3;
        config.statistics.spatial_permutations = 9;
        let csv = sample_csv(rows, 42);
        let ingestion = ingest_bytes(csv.as_bytes(), "sample.csv", &config).unwrap();
        let output = empty_metrics(&ingestion.records, "candidate_a");
        (config, ingestion.records, output)
    }

    #[test]
    fn bh_is_monotone_in_rank() {
        let adjusted = benjamini_hochberg(&[0.01, 0.04, 0.03, 0.8]);
        assert!((adjusted[0] - 0.04).abs() < 1e-12);
        assert!((adjusted[2] - 0.05333333333333334).abs() < 1e-12);
        assert!((adjusted[1] - 0.05333333333333334).abs() < 1e-12);
        assert_eq!(adjusted[3], 0.8);
    }

    #[test]
    fn holm_is_monotone_in_rank() {
        let adjusted = holm_adjust(&[0.01, 0.04, 0.03]);
        assert!((adjusted[0] - 0.03).abs() < 1e-12);
        assert!((adjusted[2] - 0.06).abs() < 1e-12);
        assert!((adjusted[1] - 0.06).abs() < 1e-12);
    }

    #[test]
    fn turnout_share_reports_each_degenerate_precondition() {
        let (config, mut source, mut output) = records(8);
        assert!(turnout_share(&source[..2], &mut output[..2], "candidate_a", &config).is_err());

        for record in &mut source {
            record.calculated_turnout_percent = Some(50.0);
        }
        assert!(turnout_share(&source, &mut output, "candidate_a", &config).is_err());

        let (mut config, mut source, mut output) = records(8);
        config.statistics.minimum_observations = 5;
        config.statistics.baseline_turnout_quantile = 0.5;
        assert!(turnout_share(&source, &mut output, "candidate_a", &config).is_err());

        config.statistics.minimum_observations = 3;
        config.statistics.baseline_turnout_quantile = 1.0;
        for record in &mut source {
            record
                .candidate_shares
                .insert("candidate_a".into(), Some(0.5));
        }
        assert!(turnout_share(&source, &mut output, "candidate_a", &config).is_err());

        let (mut config, source, mut output) = records(2);
        config.statistics.minimum_observations = 2;
        config.statistics.baseline_turnout_quantile = 1.0;
        assert!(turnout_share(&source, &mut output, "candidate_a", &config).is_err());
    }

    #[test]
    fn vote_count_trend_covers_small_constant_and_constant_share_data() {
        let (_config, source, mut output) = records(5);
        assert!(vote_share_by_count(&source[..2], &mut output[..2], "candidate_a").is_err());

        let mut constant_votes = source.clone();
        for record in &mut constant_votes {
            record.candidate_votes.insert("candidate_a".into(), 10);
        }
        assert!(vote_share_by_count(&constant_votes, &mut output, "candidate_a").is_err());

        let mut constant_share = source;
        for record in &mut constant_share {
            record
                .candidate_shares
                .insert("candidate_a".into(), Some(0.5));
        }
        let diagnostic = vote_share_by_count(&constant_share, &mut output, "candidate_a").unwrap();
        assert_eq!(diagnostic["r_squared"], 0.0);
    }

    #[test]
    fn down_ballot_skips_missing_malformed_and_zero_comparisons() {
        let (config, mut source, mut output) = records(6);
        assert!(down_ballot_difference(&source, &mut output, "missing", &config).is_err());
        source[0].source.remove("Votes_Down_Ballot_A");
        source[1]
            .source
            .insert("Votes_Down_Ballot_A".into(), "bad".into());
        source[2].candidate_votes.insert("candidate_a".into(), 0);
        let diagnostic =
            down_ballot_difference(&source, &mut output, "candidate_a", &config).unwrap();
        assert!(
            diagnostic["comparisons"].as_array().unwrap()[0]["valid_observations"]
                .as_u64()
                .unwrap()
                > 0
        );

        for record in &mut source {
            record.source.remove("Votes_Down_Ballot_A");
        }
        assert!(down_ballot_difference(&source, &mut output, "candidate_a", &config).is_err());
    }

    #[test]
    fn digit_spatial_and_robust_methods_report_unavailable_boundaries() {
        let (mut config, mut source, mut output) = records(5);
        config.statistics.minimum_observations = 10;
        assert!(digit_diagnostics(&source, &config).is_err());
        assert!(spatial(&source[..2], &mut output[..2], "candidate_a", &config).is_err());

        config.statistics.minimum_observations = 3;
        for record in &mut source {
            record
                .candidate_shares
                .insert("candidate_a".into(), Some(0.5));
        }
        assert!(spatial(&source, &mut output, "candidate_a", &config).is_err());

        config.anomaly.enabled = false;
        assert!(robust_multivariate(&source, &mut output, "candidate_a", &config).is_err());
        config.anomaly.enabled = true;
        config.statistics.minimum_observations = 10;
        assert!(robust_multivariate(&source, &mut output, "candidate_a", &config).is_err());
    }

    #[test]
    fn robust_scoring_handles_zero_ballots_and_all_constant_features() {
        let (mut config, mut source, mut output) = records(5);
        for record in &mut source {
            record.ballots_cast = Some(0);
            record.valid_contest_votes = 0;
            record.calculated_turnout_percent = Some(0.0);
            record.reported_turnout_percent = Some(0.0);
            record
                .candidate_shares
                .insert("candidate_a".into(), Some(0.5));
        }
        config.statistics.minimum_observations = 3;
        let diagnostic = robust_multivariate(&source, &mut output, "candidate_a", &config).unwrap();
        assert_eq!(diagnostic["flagged_observations"], 0);
        assert!(
            output
                .iter()
                .all(|record| !record.metrics.contains_key("robust_anomaly_score"))
        );
    }
}
