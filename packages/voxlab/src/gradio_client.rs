//! A minimal, self-contained Gradio 5 client (blocking).
//!
//! Covers just what LocalDub needs against a `/gradio_api` server:
//!   - connect to a base URL
//!   - upload a local file (`handle_file`, mirrors `@gradio/client`'s handle_file)
//!   - run a prediction (`predict`, which performs the two-request dance
//!     POST `/gradio_api/call/{api}` -> event_id, then
//!     GET  `/gradio_api/call/{api}/{event_id}` SSE stream until `event: complete`)
//!
//! This avoids pulling in an external, incomplete Gradio client crate.

use serde::Deserialize;
use serde_json::Value;
use std::io::BufRead;
use std::time::Duration;

/// A Gradio `FileData` reference. `path` is the server-side path; `url` is the
/// absolute fetchable URL. Either may be empty depending on context.
#[derive(Debug, Clone, Default)]
pub struct FileData {
    pub path: String,
    pub url: String,
}

#[derive(Debug)]
pub struct GradioClient {
    base_url: String,
    client: reqwest::blocking::Client,
}

impl GradioClient {
    /// Connect to a Gradio server. `base_url` should have no trailing slash.
    pub fn connect(base_url: &str) -> anyhow::Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(180))
            .build()
            .map_err(|e| anyhow::anyhow!("build http client: {e}"))?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
        })
    }

    /// Upload a local file to the server and return its `FileData` reference.
    /// Mirrors `@gradio/client`'s `handle_file(localPath)`.
    pub fn handle_file(&self, local_path: &str) -> anyhow::Result<FileData> {
        let upload_url = format!("{}/gradio_api/upload", self.base_url);
        let bytes = std::fs::read(local_path)
            .map_err(|e| anyhow::anyhow!("read upload file {local_path}: {e}"))?;
        let part = reqwest::blocking::multipart::Part::bytes(bytes).file_name("file");
        let form = reqwest::blocking::multipart::Form::new().part("files", part);

        let resp = self
            .client
            .post(&upload_url)
            .multipart(form)
            .send()
            .map_err(|e| anyhow::anyhow!("upload failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("upload returned status {}", resp.status()));
        }
        let paths: Vec<String> = resp
            .json()
            .map_err(|e| anyhow::anyhow!("parse upload response: {e}"))?;
        let path = paths
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("upload response contained no path"))?;
        let url = format!("{}/gradio_api/file={}", self.base_url, path);
        Ok(FileData { path, url })
    }

    /// Run a prediction. `data` is the positional argument array for `api_name`
    /// (e.g. `/generate`). Returns the JSON `data` array from the `event: complete`
    /// SSE message.
    pub fn predict(&self, api_name: &str, data: Vec<Value>) -> anyhow::Result<Vec<Value>> {
        let call_url = format!("{}/gradio_api/call{}", self.base_url, api_name);
        let payload = serde_json::json!({ "data": data });

        let resp = self
            .client
            .post(&call_url)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .map_err(|e| anyhow::anyhow!("POST {api_name} failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "POST {api_name} returned status {}",
                resp.status()
            ));
        }
        let event_id: EventIdResp = resp
            .json()
            .map_err(|e| anyhow::anyhow!("parse event_id: {e}"))?;

        let stream_url = format!(
            "{}/gradio_api/call{}/{}",
            self.base_url, api_name, event_id.event_id
        );
        let stream = self
            .client
            .get(&stream_url)
            .send()
            .map_err(|e| anyhow::anyhow!("GET {api_name} stream failed: {e}"))?;
        if !stream.status().is_success() {
            return Err(anyhow::anyhow!(
                "GET {api_name} stream returned status {}",
                stream.status()
            ));
        }

        parse_sse_event_data(stream)
    }

    /// Download a file (e.g. a produced audio URL) into memory.
    pub fn download(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        self.client
            .get(url)
            .send()
            .map_err(|e| anyhow::anyhow!("download {url}: {e}"))?
            .bytes()
            .map(|b| b.to_vec())
            .map_err(|e| anyhow::anyhow!("read download bytes: {e}"))
    }
}

#[derive(Deserialize)]
struct EventIdResp {
    event_id: String,
}

/// Listen to a Gradio SSE stream and return the `data` JSON array carried by the
/// first `event: complete` message. Errors on `event: error`.
fn parse_sse_event_data(stream: reqwest::blocking::Response) -> anyhow::Result<Vec<Value>> {
    let reader = std::io::BufReader::new(stream);
    let mut event = String::new();
    let mut data_lines: Vec<String> = Vec::new();

    for line in reader.lines().map_while(Result::ok) {
        if line.is_empty() {
            if event == "complete" {
                let joined = data_lines.concat();
                return serde_json::from_str(&joined)
                    .map_err(|e| anyhow::anyhow!("parse complete data: {e} (raw: {joined})"));
            } else if event == "error" {
                return Err(anyhow::anyhow!(
                    "gradio error event: {}",
                    data_lines.concat()
                ));
            }
            event.clear();
            data_lines.clear();
            continue;
        }
        if let Some(rest) = line.strip_prefix("event:") {
            event = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim().to_string());
        }
        // ignore other lines (id:, retry:, heartbeat data:null, etc.)
    }

    Err(anyhow::anyhow!(
        "SSE stream ended without a complete/error event"
    ))
}
