use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{path::PathBuf, time::Duration};
use tracing::{info, warn};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FeedbackEvent {
    event_key: String,
    build_id: String,
    project_id: Option<String>,
    gate: String,
    passed: bool,
    source: String,
}

fn clean(value: Option<&Value>, max: usize) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .chars()
        .take(max)
        .collect()
}

fn request_id(value: &Value, names: &[&str]) -> String {
    for name in names {
        let candidate = clean(value.get(*name), 180);
        if !candidate.is_empty() {
            return candidate;
        }
    }
    String::new()
}

fn scan_path(path_and_query: &str) -> bool {
    matches!(
        path_and_query.split('?').next().unwrap_or(path_and_query),
        "/api/v1/scan"
            | "/api/v1/intelligence/scan/github"
            | "/api/v1/intelligence/scan/zip"
    )
}

fn event_from_exchange(
    path_and_query: &str,
    request_body: &[u8],
    response_status: u16,
    response_body: &[u8],
) -> Option<FeedbackEvent> {
    if !scan_path(path_and_query) || !(200..300).contains(&response_status) {
        return None;
    }

    let request: Value = serde_json::from_slice(request_body).ok()?;
    let response: Value = serde_json::from_slice(response_body).ok()?;
    if response.get("status").and_then(Value::as_str) != Some("success") {
        return None;
    }

    let build_id = request_id(
        &request,
        &["omnicode_build_id", "omnicodeBuildId", "buildId"],
    );
    if build_id.is_empty() {
        return None;
    }
    let project_id = request_id(
        &request,
        &[
            "omnicode_project_id",
            "omnicodeProjectId",
            "projectId",
            "project_id",
        ],
    );
    let snapshot_id = clean(response.get("snapshotId"), 120);
    if snapshot_id.is_empty() {
        return None;
    }
    let violations = response
        .get("violationsFound")
        .and_then(Value::as_u64)
        .unwrap_or(u64::MAX);

    Some(FeedbackEvent {
        event_key: format!("ckb:{snapshot_id}:architecture:v1"),
        build_id,
        project_id: if project_id.is_empty() {
            None
        } else {
            Some(project_id)
        },
        gate: "architecture".into(),
        passed: violations == 0,
        source: "ckb".into(),
    })
}

fn endpoint() -> String {
    std::env::var("OMNICODE_FEEDBACK_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            "https://omnicode-pro.vercel.app/api/integrations/learning-feedback".into()
        })
}

fn secret() -> Option<String> {
    std::env::var("OMNICODE_CKB_FEEDBACK_SECRET")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn outbox_dir() -> PathBuf {
    let base = std::env::var("CKB_REALITY_DATA_DIR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "ckb_reality_data".into());
    PathBuf::from(base).join("omnicode_feedback_outbox")
}

fn safe_file_key(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn event_file(event: &FeedbackEvent) -> PathBuf {
    outbox_dir().join(format!("{}.json", safe_file_key(&event.event_key)))
}

async fn persist(event: &FeedbackEvent) -> anyhow::Result<PathBuf> {
    let directory = outbox_dir();
    tokio::fs::create_dir_all(&directory).await?;
    let path = event_file(event);
    let temp = path.with_extension("json.tmp");
    tokio::fs::write(&temp, serde_json::to_vec(event)?).await?;
    tokio::fs::rename(&temp, &path).await?;
    Ok(path)
}

async fn deliver(client: &Client, event: &FeedbackEvent) -> anyhow::Result<bool> {
    let Some(secret) = secret() else {
        return Ok(false);
    };
    let response = client
        .post(endpoint())
        .header("x-omnicode-validation-secret", secret)
        .json(&json!({
            "eventKey": event.event_key,
            "buildId": event.build_id,
            "projectId": event.project_id,
            "gate": event.gate,
            "passed": event.passed,
            "source": event.source,
        }))
        .timeout(Duration::from_secs(12))
        .send()
        .await?;

    if response.status().is_success() {
        return Ok(true);
    }
    warn!(
        "OmniCode architecture feedback returned status {} for {}",
        response.status(),
        event.event_key
    );
    Ok(false)
}

async fn deliver_file(client: &Client, path: PathBuf) {
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(error) => {
            warn!("Unable to read CKB OmniCode feedback outbox item {:?}: {}", path, error);
            return;
        }
    };
    let event: FeedbackEvent = match serde_json::from_slice(&bytes) {
        Ok(event) => event,
        Err(error) => {
            warn!("Invalid CKB OmniCode feedback outbox item {:?}: {}", path, error);
            return;
        }
    };

    match deliver(client, &event).await {
        Ok(true) => {
            let _ = tokio::fs::remove_file(&path).await;
            info!("Delivered CKB architecture feedback {}", event.event_key);
        }
        Ok(false) => {}
        Err(error) => warn!(
            "Unable to deliver CKB architecture feedback {}: {}",
            event.event_key, error
        ),
    }
}

pub fn enqueue_from_exchange(
    client: Client,
    path_and_query: String,
    request_body: Vec<u8>,
    response_status: u16,
    response_body: Vec<u8>,
) {
    let Some(event) = event_from_exchange(
        &path_and_query,
        &request_body,
        response_status,
        &response_body,
    ) else {
        return;
    };

    tokio::spawn(async move {
        match persist(&event).await {
            Ok(path) => deliver_file(&client, path).await,
            Err(error) => warn!(
                "Unable to persist CKB architecture feedback {}: {}",
                event.event_key, error
            ),
        }
    });
}

pub fn start_reconciler(client: Client) {
    tokio::spawn(async move {
        loop {
            let directory = outbox_dir();
            if let Ok(mut entries) = tokio::fs::read_dir(&directory).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    if entry
                        .path()
                        .extension()
                        .and_then(|value| value.to_str())
                        == Some("json")
                    {
                        deliver_file(&client, entry.path()).await;
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}
