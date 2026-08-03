use std::{net::SocketAddr, sync::Arc};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{CAUTION, config::Config, ingestion::ingest_bytes, workflow};

#[derive(Clone)]
struct AppState {
    config: Config,
}

#[derive(Deserialize)]
struct AnalyzeBody {
    csv_text: String,
    #[serde(default = "candidate_default")]
    candidate_key: String,
    #[serde(default)]
    methods: Vec<String>,
}

fn candidate_default() -> String {
    "candidate_a".into()
}

pub fn router(config: Config) -> Router {
    let limit = config.data.max_file_size_mb * 1024 * 1024;
    Router::new()
        .route("/", get(index))
        .route("/sample.csv", get(sample))
        .route("/health", get(health))
        .route("/api/validate", post(validate))
        .route("/api/analyze", post(analyze))
        .layer(DefaultBodyLimit::max(limit))
        .with_state(Arc::new(AppState { config }))
}

pub async fn serve(config: Config, bind: Option<SocketAddr>) -> anyhow::Result<()> {
    let address = bind.unwrap_or(config.server.bind.parse()?);
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(%address, "web application listening");
    axum::serve(listener, router(config)).await?;
    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn health() -> Json<Value> {
    Json(json!({"status": "healthy", "version": env!("CARGO_PKG_VERSION"), "caution": CAUTION}))
}

async fn sample(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/csv; charset=utf-8")],
        crate::sample::sample_csv(120, state.config.statistics.random_seed),
    )
}

async fn validate(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AnalyzeBody>,
) -> impl IntoResponse {
    match ingest_bytes(body.csv_text.as_bytes(), "web-upload.csv", &state.config) {
        Ok(result) => (
            StatusCode::OK,
            Json(json!({"status": "valid", "result": result, "caution": CAUTION})),
        ),
        Err(error) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"status": "invalid", "message": error.to_string(), "caution": CAUTION})),
        ),
    }
}

async fn analyze(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AnalyzeBody>,
) -> impl IntoResponse {
    let result = ingest_bytes(body.csv_text.as_bytes(), "web-upload.csv", &state.config).and_then(
        |ingestion| {
            let methods = if body.methods.is_empty() {
                workflow::ALL_METHODS
                    .iter()
                    .map(|method| (*method).to_owned())
                    .collect()
            } else {
                body.methods
            };
            workflow::analyze(&ingestion, &body.candidate_key, &methods, &state.config)
        },
    );
    match result {
        Ok(run) => (
            StatusCode::OK,
            Json(json!({"status": "complete_with_method_statuses", "run": run})),
        ),
        Err(error) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"status": "invalid", "message": error.to_string(), "caution": CAUTION})),
        ),
    }
}

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Precinct Election Analysis — Rust</title><style>
:root{color-scheme:dark;--bg:#08131d;--panel:#102331;--ink:#e8f1f5;--muted:#a9bac5;--accent:#42c3a1;--warn:#ffca6a}*{box-sizing:border-box}body{margin:0;font:16px system-ui;background:radial-gradient(circle at top left,#16354a,var(--bg) 50%);color:var(--ink)}main{max-width:1050px;margin:auto;padding:3rem 1.25rem}h1{font-size:clamp(2rem,6vw,4.5rem);line-height:.95;max-width:800px}p{color:var(--muted);line-height:1.6}.warning{border-left:4px solid var(--warn);padding:1rem;background:#2a251b}.panel{background:color-mix(in srgb,var(--panel) 92%,transparent);padding:1.5rem;border:1px solid #284557;border-radius:14px;margin-top:2rem}label{display:block;margin:.8rem 0 .35rem}input,select,button{font:inherit;padding:.75rem;border-radius:7px;border:1px solid #426071;background:#0a1923;color:var(--ink)}button{background:var(--accent);color:#042019;font-weight:700;cursor:pointer;margin-top:1rem}pre{white-space:pre-wrap;max-height:600px;overflow:auto;background:#061019;padding:1rem;border-radius:8px}</style></head>
<body><main><p>RUST IMPLEMENTATION · VALIDATED WORKFLOW</p><h1>Precinct election diagnostics with explicit limits.</h1><p class="warning">An anomaly is unusual under a stated exploratory model. It is not proof of fraud, manipulation, misconduct, or an incorrect outcome.</p>
<section class="panel"><h2>Analyze a CSV</h2><p><a href="/sample.csv" style="color:var(--accent)">Download a fictional sample CSV</a></p><label for="file">Precinct CSV</label><input id="file" type="file" accept=".csv"><label for="candidate">Candidate key</label><input id="candidate" value="candidate_a"><button id="run">Validate and analyze</button><pre id="result">Choose a CSV to begin.</pre></section></main>
<script>document.querySelector('#run').onclick=async()=>{const file=document.querySelector('#file').files[0],out=document.querySelector('#result');if(!file){out.textContent='Choose a CSV first.';return}out.textContent='Analyzing…';const response=await fetch('/api/analyze',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({csv_text:await file.text(),candidate_key:document.querySelector('#candidate').value,methods:[]})});out.textContent=JSON.stringify(await response.json(),null,2)}</script></body></html>"#;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    async fn response_json(response: axum::response::Response) -> Value {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn index_contains_interpretation_boundary() {
        assert!(index().await.0.contains("not proof of fraud"));
        assert!(INDEX_HTML.contains("/api/analyze"));
    }

    #[tokio::test]
    async fn public_get_routes_return_health_and_deterministic_sample() {
        let app = router(Config::default());
        let response = app
            .clone()
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let health = response_json(response).await;
        assert_eq!(health["status"], "healthy");
        assert!(health["caution"].as_str().unwrap().contains("not proof"));

        let response = app
            .oneshot(Request::get("/sample.csv").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/csv; charset=utf-8"
        );
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            String::from_utf8(bytes.to_vec()).unwrap().lines().count(),
            121
        );
    }

    #[tokio::test]
    async fn validation_endpoint_distinguishes_valid_and_invalid_csv() {
        let app = router(Config::default());
        for (csv_text, expected) in [
            (crate::sample::sample_csv(3, 42), StatusCode::OK),
            ("bad\nvalue\n".into(), StatusCode::UNPROCESSABLE_ENTITY),
        ] {
            let request = Request::post("/api/validate")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({"csv_text": csv_text})).unwrap(),
                ))
                .unwrap();
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), expected);
            let body = response_json(response).await;
            assert_eq!(
                body["status"],
                if expected == StatusCode::OK {
                    "valid"
                } else {
                    "invalid"
                }
            );
        }
    }

    #[tokio::test]
    async fn analysis_endpoint_honors_method_selection_and_reports_errors() {
        let app = router(Config::default());
        let request = Request::post("/api/analyze")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({
                    "csv_text": crate::sample::sample_csv(5, 42),
                    "candidate_key": "candidate_a",
                    "methods": ["vote_share_by_count"]
                }))
                .unwrap(),
            ))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(
            body["run"]["requested_methods"],
            json!(["vote_share_by_count"])
        );

        let request = Request::post("/api/analyze")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({
                    "csv_text": crate::sample::sample_csv(3, 42),
                    "candidate_key": "missing"
                }))
                .unwrap(),
            ))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = response_json(response).await;
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains("unknown candidate")
        );
    }
}
