//! Scientifically cautious precinct-level election data validation and analysis.

pub mod config;
pub mod error;
pub mod export;
pub mod ingestion;
pub mod mcp;
pub mod model;
pub mod sample;
pub mod statistics;
pub mod web;
pub mod workflow;

pub const CAUTION: &str = "An anomaly is unusual under a stated exploratory model. It is not proof of fraud, manipulation, misconduct, or an incorrect outcome. A risk-limiting audit examines ballot evidence; aggregate precinct diagnostics do not confirm an election outcome.";
