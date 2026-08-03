use std::path::PathBuf;

use rmcp::{
    Json, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    CAUTION,
    config::Config,
    ingestion::ingest_bytes,
    sample::sample_csv,
    workflow::{self, ALL_METHODS},
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SampleRequest {
    #[serde(default = "default_rows")]
    pub rows: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ValidateRequest {
    pub csv_text: String,
    #[serde(default = "default_preview")]
    pub preview_rows: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AnalyzeRequest {
    pub csv_text: String,
    #[serde(default = "default_candidate")]
    pub candidate_key: String,
    pub methods: Option<Vec<String>>,
    #[serde(default = "default_records")]
    pub max_records: usize,
}

fn default_rows() -> usize {
    120
}
fn default_preview() -> usize {
    10
}
fn default_records() -> usize {
    100
}
fn default_candidate() -> String {
    "candidate_a".into()
}

#[derive(Debug, Clone)]
pub struct ElectionMcp {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    config: Config,
}

impl ElectionMcp {
    pub fn new(config: Config) -> Self {
        Self {
            tool_router: Self::tool_router(),
            config,
        }
    }
}

#[tool_router]
impl ElectionMcp {
    #[tool(description = "Return server health, supported methods, and interpretation limits")]
    fn health(&self) -> Json<Value> {
        Json(json!({
            "status": "healthy",
            "version": env!("CARGO_PKG_VERSION"),
            "available_methods": ALL_METHODS,
            "caution": CAUTION
        }))
    }

    #[tool(description = "Return bounded, deterministic fictional generalized-schema CSV data")]
    fn sample_csv(
        &self,
        Parameters(request): Parameters<SampleRequest>,
    ) -> Result<Json<Value>, String> {
        if !(1..=1000).contains(&request.rows) {
            return Err("rows must be between 1 and 1000".into());
        }
        Ok(Json(json!({
            "filename": "fictional_michigan_compatible_sample.csv",
            "csv": sample_csv(request.rows, self.config.statistics.random_seed),
            "fictional": true,
            "caution": CAUTION
        })))
    }

    #[tool(
        description = "Validate CSV text and return provenance, issues, exclusions, and a bounded preview"
    )]
    fn validate_csv(
        &self,
        Parameters(request): Parameters<ValidateRequest>,
    ) -> Result<Json<Value>, String> {
        if request.preview_rows > 100 {
            return Err("preview_rows must be between 0 and 100".into());
        }
        match ingest_bytes(request.csv_text.as_bytes(), "mcp.csv", &self.config) {
            Ok(result) => Ok(Json(json!({
                "status": if result.excluded.is_empty() { "valid" } else { "valid_with_exclusions" },
                "schema": result.provenance.source_schema,
                "candidate_choices": result.candidate_labels,
                "provenance": result.provenance,
                "validation": result.report,
                "accepted_preview": result.records.into_iter().take(request.preview_rows).collect::<Vec<_>>(),
                "excluded_preview": result.excluded.into_iter().take(request.preview_rows).collect::<Vec<_>>(),
                "caution": CAUTION
            }))),
            Err(error) => Ok(Json(
                json!({"status": "invalid", "message": error.to_string(), "caution": CAUTION}),
            )),
        }
    }

    #[tool(
        description = "Validate and analyze CSV text with explicitly named, independently reported methods"
    )]
    fn analyze_csv(
        &self,
        Parameters(request): Parameters<AnalyzeRequest>,
    ) -> Result<Json<Value>, String> {
        if request.max_records > 500 {
            return Err("max_records must be between 0 and 500".into());
        }
        let ingestion = match ingest_bytes(request.csv_text.as_bytes(), "mcp.csv", &self.config) {
            Ok(result) => result,
            Err(error) => {
                return Ok(Json(
                    json!({"status": "invalid", "message": error.to_string(), "caution": CAUTION}),
                ));
            }
        };
        let methods = request.methods.unwrap_or_else(|| {
            ALL_METHODS
                .iter()
                .map(|method| (*method).to_owned())
                .collect()
        });
        let run = workflow::analyze(&ingestion, &request.candidate_key, &methods, &self.config)
            .map_err(|error| error.to_string())?;
        let total = run.records.len();
        let records: Vec<_> = run
            .records
            .iter()
            .take(request.max_records)
            .cloned()
            .collect();
        Ok(Json(json!({
            "status": "complete_with_method_statuses",
            "method_statuses": run.statuses,
            "metadata": {
                "candidate_key": run.candidate_key,
                "candidate_label": run.candidate_label,
                "input_schema": run.input_schema,
                "random_seed": run.random_seed,
                "input_rows": run.input_rows,
                "analysis_rows": run.analysis_rows,
                "excluded_rows": run.excluded_rows
            },
            "diagnostics": run.diagnostics,
            "records_returned": records.len(),
            "records_total": total,
            "records": records,
            "caution": CAUTION
        })))
    }

    #[tool(
        description = "Prepare bounded computed context and constraints for an LLM explanatory summary"
    )]
    fn narrative_context(
        &self,
        Parameters(request): Parameters<AnalyzeRequest>,
    ) -> Result<Json<Value>, String> {
        let result = self.analyze_csv(Parameters(AnalyzeRequest {
            max_records: 0,
            ..request
        }))?;
        Ok(Json(json!({
            "label": "Context for an AI-generated explanatory summary",
            "analysis": result.0,
            "required_interpretation": CAUTION,
            "prohibited_claims": [
                "proof of fraud or manipulation",
                "confirmation of an election outcome",
                "statistical significance not present in computed diagnostics",
                "audit priority without ballot evidence and an authorized audit design"
            ]
        })))
    }
}

#[tool_handler]
impl ServerHandler for ElectionMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("precinct-election-analysis-rs", env!("CARGO_PKG_VERSION"))
                    .with_title("Precinct Election Analysis — Rust")
                    .with_description("Validated, exploratory precinct election diagnostics"),
            )
            .with_instructions(CAUTION)
    }
}

pub async fn run_stdio(config_path: Option<PathBuf>) -> anyhow::Result<()> {
    let config = Config::load(config_path.as_deref())?;
    let service = ElectionMcp::new(config)
        .serve(rmcp::transport::stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_contracts_are_bounded() {
        let server = ElectionMcp::new(Config::default());
        let tools = server.tool_router.list_all();
        let names: std::collections::HashSet<_> =
            tools.iter().map(|tool| tool.name.as_ref()).collect();
        assert_eq!(names.len(), 5);
        assert!(names.contains("health"));
        assert!(names.contains("analyze_csv"));
    }
}
