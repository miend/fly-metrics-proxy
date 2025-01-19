use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Router};
use reqwest::Client;
use serde;
use serde_json;
use std::{collections::HashMap, error::Error};
use tokio;

#[derive(Clone, Debug, serde::Deserialize)]
struct MetricsTarget {
    endpoint: String,
    labels: HashMap<String, String>,
}

#[derive(Clone)]
struct AppState {
    targets: Vec<MetricsTarget>,
}

fn parse_targets_env(env_var: String) -> Result<Vec<MetricsTarget>, serde_json::Error> {
    let res: Vec<MetricsTarget> = serde_json::from_str(&env_var)?;
    Ok(res)
}

#[tokio::main]
async fn main() {
    let env_var = std::env::var("METRICS_TARGETS").expect("METRICS_TARGETS env var is not set");

    let state = AppState {
        targets: parse_targets_env(env_var).expect("Failed to parse METRICS_TARGETS env var"),
    };

    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();

    println!("Starting metrics proxy server on 0.0.0.0:8080");

    axum::serve(listener, app).await.unwrap();
}

async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    let mut collected_metric_groups: HashMap<String, String> = HashMap::new();

    for target in state.targets {
        let metrics = match get_metrics(&target.endpoint).await {
            Ok(response) => response,
            Err(e) => {
                eprintln!(
                    "Something went wrong while querying metrics from remote endpoints: {}",
                    e
                );
                return (StatusCode::INTERNAL_SERVER_ERROR).into_response();
            }
        };

        // groups of metrics in prometheus format should already be separated by an empty line
        let metric_groups = metrics.trim().split("\n\n");

        // now find the name of the metric for each group, apply labels, and push to hashmap
        for group in metric_groups {
            let group_name: Option<&str> = group.lines().find_map(|line| {
                if line.starts_with("# HELP ") || line.starts_with("# TYPE ") {
                    line.split_whitespace().nth(2)
                } else if !line.starts_with('#') && !line.is_empty() {
                    // the metric should be the first value

                    // if has a label, group name comes before that
                    if let Some(split_line) = line.split_once('{') {
                        Some(split_line.0)
                    } else if let Some(split_line) = line.split_once(' ') {
                        Some(split_line.0)
                    } else {
                        None
                    }
                } else {
                    None
                }
            });

            let labeled_group = apply_labels(target.labels.clone(), group.to_string());

            match group_name {
                Some(name) => {
                    collected_metric_groups
                        .entry(name.to_string())
                        .or_insert("".to_string())
                        .push_str(&format!("\n{}", labeled_group.trim()));
                }
                None => eprintln!("Failed to parse a metric name from a group: {}", group),
            }
        }
    }

    for group in collected_metric_groups.values_mut() {
        let mut lines: Vec<_> = group.lines().collect();
        lines.sort();
        lines.dedup();
        //TODO: maybe think about deduping HELP? Could have different
        //help strings for the same metric from different sources
        *group = lines.join("\n");
    }

    // Join all groups together in a nicely readable way, also sorted alphabetically
    let mut sorted_keys = collected_metric_groups.keys().collect::<Vec<_>>();
    sorted_keys.sort();

    let mut response = String::new();
    for key in sorted_keys {
        if !response.is_empty() {
            response.push_str("\n\n");
        }
        response.push_str(&collected_metric_groups[key].trim());
    }

    response = response.trim().to_string();

    return (StatusCode::OK, [("content-type", "text/plain")], response).into_response();
}

async fn get_metrics(endpoint: &str) -> Result<String, String> {
    let client = Client::new();

    match client.get(endpoint).send().await {
        Ok(response) => match response.text().await {
            Ok(text) => Ok(text),
            Err(e) => Err(format!("Error reading text of metrics: {}", e)),
        },
        Err(e) => {
            let mut source = e.source();
            while let Some(e) = source {
                eprintln!("Caused by: {}", e);
                source = e.source();
            }
            Err(format!("Error fetching metrics: {}", e))
        }
    }
}

fn apply_labels(labels: HashMap<String, String>, metrics: String) -> String {
    let mut modified_lines: Vec<String> = Vec::new();

    for line in metrics.lines() {
        if !line.starts_with("#") {
            let mut formatted_labels = String::new();

            for (key, value) in &labels {
                let mut label = format!("{}=\"{}\"", key, value);

                if formatted_labels.len() > 0 {
                    label.insert_str(0, ",")
                }

                formatted_labels.push_str(&label);
            }

            let (metric, value) = line.split_once(" ").unwrap();

            let modified_metric: String;

            if metric.ends_with("}") {
                // if the metric ends in }, it has labels already, so append ours
                let prefix = metric.strip_suffix("}").unwrap();
                modified_metric = format!("{},{}}}", prefix, formatted_labels);
            } else {
                modified_metric = format!("{}{{{}}}", metric, formatted_labels);
            }

            let modified_line = format!("{} {}", modified_metric, value);
            modified_lines.push(modified_line.to_owned());
        } else {
            // it's metadata, include it unmodified
            modified_lines.push(line.trim().to_owned());
        }
    }
    modified_lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito;

    const METRICS_1: &str = r#"
# TYPE http_requests_total counter
http_requests_total{method="get",code="200"} 512

# HELP queue_length The number of outstanding jobs in the queue
# TYPE queue_length gauge
queue_length 7

# TYPE process_memory_bytes gauge
process_memory_bytes 1.23e+07

# HELP request_time Request time in seconds
# TYPE request_time histogram
request_time_bucket{le="1"} 5
request_time_bucket{le="+Inf"} 7
    "#;

    const METRICS_2: &str = r#"
# HELP queue_length The number of outstanding jobs in the queue
# TYPE queue_length gauge
queue_length 10

# HELP request_time Request time in seconds
# TYPE request_time histogram
request_time_bucket{le="1"} 7
request_time_bucket{le="+Inf"} 10

# TYPE process_memory_bytes gauge
process_memory_bytes 1.23e+07

# TYPE http_requests_total counter
http_requests_total{method="get",code="200"} 1024"#;

    const COMBINED_METRICS_WITH_LABELS: &str = r#"# TYPE http_requests_total counter
http_requests_total{method="get",code="200",app_name="a"} 512
http_requests_total{method="get",code="200",app_name="b"} 1024

# TYPE process_memory_bytes gauge
process_memory_bytes{app_name="a"} 1.23e+07
process_memory_bytes{app_name="b"} 1.23e+07

# HELP queue_length The number of outstanding jobs in the queue
# TYPE queue_length gauge
queue_length{app_name="a"} 7
queue_length{app_name="b"} 10

# HELP request_time Request time in seconds
# TYPE request_time histogram
request_time_bucket{le="+Inf",app_name="a"} 7
request_time_bucket{le="+Inf",app_name="b"} 10
request_time_bucket{le="1",app_name="a"} 5
request_time_bucket{le="1",app_name="b"} 7"#;

    #[tokio::test]
    // It should return the metrics and metadata from both target endpoints:
    // - deduped
    // - sorted alphabetically
    // - same metrics grouped together, separated by newlines
    async fn returns_combines_metrics_with_labels() {
        let mut server = mockito::Server::new_async().await;

        let metrics_1 = server
            .mock("GET", "/metrics1")
            .with_status(200)
            .with_header("content-type", "text/plain")
            .with_body(METRICS_1)
            .create_async()
            .await;

        let metrics_2 = server
            .mock("GET", "/metrics2")
            .with_status(200)
            .with_header("content-type", "text/plain")
            .with_body(METRICS_2)
            .create_async()
            .await;

        let input = format!(
            r#"
            [
                {{
                    "endpoint": "{}/metrics1",
                    "labels": {{"app_name": "a"}}
                }},
                {{
                    "endpoint": "{}/metrics2",
                    "labels": {{"app_name": "b"}}
                }}

            ]
        "#,
            server.url(),
            server.url()
        );

        let state = AppState {
            targets: parse_targets_env(input).expect("Failed to parse METRICS_TARGETS env var"),
        };

        let response = metrics_handler(State(state)).await.into_response();
        let body = response.into_body();
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let body_str = String::from_utf8(bytes.to_vec()).unwrap();
        println!("Here's the body yo:\n{}", body_str);

        metrics_1.assert();
        metrics_2.assert();
        assert_eq!(body_str, COMBINED_METRICS_WITH_LABELS);
    }
}
