use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Router};
use reqwest::Client;
use serde;
use serde_json;
use tokio;

#[derive(Clone, Debug, serde::Deserialize)]
struct MetricsTarget {
    endpoint: String,
    queue_name: String,
}

#[derive(Clone)]
struct AppState {
    targets: Vec<MetricsTarget>,
}

fn parse_targets_env() -> Result<Vec<MetricsTarget>, serde_json::Error> {
    let env_var = std::env::var("METRICS_TARGETS").expect("METRICS_TARGETS env var is not set");
    let res: Vec<MetricsTarget> = serde_json::from_str(&env_var)?;
    println!("Parsed METRICS_TARGETS: {:?}", res);
    Ok(res)
}

#[tokio::main]
async fn main() {
    let state = AppState {
        targets: parse_targets_env().expect("Failed to parse METRICS_TARGETS env var"),
    };

    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();

    println!("Starting metrics proxy server on 0.0.0.0:8080");

    axum::serve(listener, app).await.unwrap();
}

async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    let mut gathered_metrics: Vec<String> = Vec::new();

    for target in state.targets {
        match get_metric_lines(&target.endpoint, &target.queue_name).await {
            Ok(lines) => {
                for line in lines {
                    if !gathered_metrics.contains(&line) {
                        gathered_metrics.push(line);
                    } else {
                        if !line.starts_with("#") {
                            // Duplicate lines that aren't metadata are considered an error
                            eprintln!("Duplicate metric found: {}", line);
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Error getting metrics: {}", e);
            }
        }
    }

    let response = gathered_metrics.join("\n");

    return (StatusCode::OK, [("content-type", "text/plain")], response).into_response();
}

async fn get_metric_lines(endpoint: &str, queue_name: &str) -> Result<Vec<String>, String> {
    let client = Client::new();

    match client.get(endpoint).send().await {
        Ok(response) => match response.text().await {
            Ok(text) => {
                let mut modified_lines: Vec<String> = Vec::new();
                let lines: Vec<_> = text.lines().map(|s| s.to_owned()).collect();
                for line in &lines {
                    if line.len() < 1 {
                        // discard empty lines
                        continue;
                    } else if !line.starts_with("#") {
                        // add a label to a metric
                        let split_line = line.split_once(" ").unwrap();
                        let modified_line = format!(
                            "{}{} {}",
                            split_line.0,
                            format!("{{app_name=\"{}\"}}", queue_name),
                            split_line.1
                        );
                        modified_lines.push(modified_line.to_owned());
                    } else {
                        // it's metadata, include it unmodified
                        modified_lines.push(line.to_owned());
                    }
                }
                Ok(modified_lines)
            }
            Err(e) => Err(format!("Error reading text of metrics: {}", e)),
        },
        Err(e) => Err(format!("Error fetching metrics: {}", e)),
    }
}
