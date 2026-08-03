use std::{collections::HashSet, fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub data: DataConfig,
    pub statistics: StatisticsConfig,
    pub anomaly: AnomalyConfig,
    pub server: ServerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DataConfig {
    pub max_file_size_mb: usize,
    pub turnout_tolerance_percentage_points: f64,
    pub schema: ContestSchema,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ContestSchema {
    pub jurisdiction: String,
    pub precinct: String,
    pub vote_type: Option<String>,
    pub registered_voters: Option<String>,
    pub active_registered_voters: Option<String>,
    pub ballots_cast: Option<String>,
    pub valid_contest_votes: String,
    pub write_in_votes: Option<String>,
    pub undervotes: Option<String>,
    pub overvotes: Option<String>,
    pub latitude: Option<String>,
    pub longitude: Option<String>,
    pub reported_turnout: Option<String>,
    pub candidates: Vec<CandidateConfig>,
    pub down_ballot_pairs: Vec<DownBallotConfig>,
    pub contest_votes_may_exceed_ballots: bool,
    pub ballots_may_exceed_registration: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateConfig {
    pub column: String,
    pub label: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownBallotConfig {
    pub presidential_candidate_key: String,
    pub down_ballot_column: String,
    pub label: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StatisticsConfig {
    pub minimum_observations: usize,
    pub baseline_turnout_quantile: f64,
    pub studentized_residual_threshold: f64,
    pub alpha: f64,
    pub spatial_neighbors: usize,
    pub spatial_permutations: usize,
    pub random_seed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnomalyConfig {
    pub enabled: bool,
    pub flag_threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub bind: String,
}

impl Default for DataConfig {
    fn default() -> Self {
        Self {
            max_file_size_mb: 200,
            turnout_tolerance_percentage_points: 1.0,
            schema: ContestSchema::default(),
        }
    }
}

impl Default for ContestSchema {
    fn default() -> Self {
        Self {
            jurisdiction: "Jurisdiction".into(),
            precinct: "Precinct".into(),
            vote_type: None,
            registered_voters: Some("Registered_Voters".into()),
            active_registered_voters: None,
            ballots_cast: Some("Ballots_Cast".into()),
            valid_contest_votes: "Valid_Contest_Votes".into(),
            write_in_votes: Some("Write_In_Votes".into()),
            undervotes: Some("Undervotes".into()),
            overvotes: Some("Overvotes".into()),
            latitude: Some("Latitude".into()),
            longitude: Some("Longitude".into()),
            reported_turnout: Some("Reported_Turnout_Percent".into()),
            candidates: vec![
                CandidateConfig {
                    column: "Votes_Candidate_A".into(),
                    label: "Candidate A".into(),
                    key: "candidate_a".into(),
                },
                CandidateConfig {
                    column: "Votes_Candidate_B".into(),
                    label: "Candidate B".into(),
                    key: "candidate_b".into(),
                },
            ],
            down_ballot_pairs: vec![
                DownBallotConfig {
                    presidential_candidate_key: "candidate_a".into(),
                    down_ballot_column: "Votes_Down_Ballot_A".into(),
                    label: "Candidate A / Down-Ballot A".into(),
                    key: "candidate_a_down_ballot".into(),
                },
                DownBallotConfig {
                    presidential_candidate_key: "candidate_b".into(),
                    down_ballot_column: "Votes_Down_Ballot_B".into(),
                    label: "Candidate B / Down-Ballot B".into(),
                    key: "candidate_b_down_ballot".into(),
                },
            ],
            contest_votes_may_exceed_ballots: false,
            ballots_may_exceed_registration: false,
        }
    }
}

impl Default for StatisticsConfig {
    fn default() -> Self {
        Self {
            minimum_observations: 20,
            baseline_turnout_quantile: 0.9,
            studentized_residual_threshold: 3.0,
            alpha: 0.05,
            spatial_neighbors: 8,
            spatial_permutations: 999,
            random_seed: 42,
        }
    }
}

impl Default for AnomalyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            flag_threshold: 0.75,
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8080".into(),
        }
    }
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let config = match path {
            Some(path) => serde_yaml::from_str(&fs::read_to_string(path)?)?,
            None => Self::default(),
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.data.max_file_size_mb == 0 {
            return Err(AppError::Config("max_file_size_mb must be positive".into()));
        }
        if !self.data.turnout_tolerance_percentage_points.is_finite()
            || self.data.turnout_tolerance_percentage_points < 0.0
        {
            return Err(AppError::Config(
                "turnout tolerance must be finite and nonnegative".into(),
            ));
        }
        if self.data.schema.candidates.is_empty() {
            return Err(AppError::Config(
                "at least one candidate is required".into(),
            ));
        }
        let mut columns = HashSet::new();
        let mut labels = HashSet::new();
        let mut keys = HashSet::new();
        for candidate in &self.data.schema.candidates {
            if candidate.column.trim().is_empty()
                || candidate.label.trim().is_empty()
                || candidate.key.trim().is_empty()
            {
                return Err(AppError::Config(
                    "candidate fields must be non-empty".into(),
                ));
            }
            if !columns.insert(&candidate.column)
                || !labels.insert(&candidate.label)
                || !keys.insert(&candidate.key)
            {
                return Err(AppError::Config(
                    "candidate columns, labels, and keys must be unique".into(),
                ));
            }
        }
        let stats = &self.statistics;
        if stats.minimum_observations < 3
            || !(0.5..=1.0).contains(&stats.baseline_turnout_quantile)
            || !stats.studentized_residual_threshold.is_finite()
            || stats.studentized_residual_threshold <= 0.0
            || !(0.0..1.0).contains(&stats.alpha)
            || stats.spatial_neighbors == 0
            || stats.spatial_permutations == 0
        {
            return Err(AppError::Config("invalid statistical configuration".into()));
        }
        if !self.anomaly.flag_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.anomaly.flag_threshold)
        {
            return Err(AppError::Config(
                "anomaly flag_threshold must be in [0, 1]".into(),
            ));
        }
        self.server
            .bind
            .parse::<std::net::SocketAddr>()
            .map_err(|error| {
                AppError::Config(format!("server.bind must be an IP socket address: {error}"))
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_yaml_keeps_nested_defaults() {
        let config: Config = serde_yaml::from_str("anomaly:\n  enabled: false\n").unwrap();
        config.validate().unwrap();
        assert!(!config.anomaly.enabled);
        assert_eq!(config.statistics.spatial_neighbors, 8);
        assert_eq!(config.data.schema.candidates.len(), 2);
    }

    #[test]
    fn rejects_duplicate_candidate_labels_and_bad_thresholds() {
        let mut config = Config::default();
        config.data.schema.candidates[1].label = config.data.schema.candidates[0].label.clone();
        assert!(config.validate().is_err());
        let mut config = Config::default();
        config.anomaly.flag_threshold = f64::NAN;
        assert!(config.validate().is_err());
    }
}
