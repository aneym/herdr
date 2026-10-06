//! Client for herdr's newline-delimited JSON API socket.
//!
//! Each request uses its own connection: write one `{"id","method","params"}` line, read
//! one response line. `events.subscribe` keeps its connection open and streams events.

use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::sync::atomic::{AtomicU64, Ordering};

use interprocess::local_socket::Stream as LocalStream;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::endpoint::Endpoint;

#[derive(Debug)]
pub enum ApiError {
    Io(io::Error),
    /// The line was not valid JSON or did not have the expected shape.
    InvalidResponse(String),
    /// herdr answered with `{"error": {"code", "message"}}`.
    Server {
        code: String,
        message: String,
    },
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::Io(err) => write!(f, "api i/o: {err}"),
            ApiError::InvalidResponse(detail) => write!(f, "invalid api response: {detail}"),
            ApiError::Server { code, message } => write!(f, "herdr api error {code}: {message}"),
        }
    }
}

impl std::error::Error for ApiError {}

impl From<io::Error> for ApiError {
    fn from(err: io::Error) -> Self {
        ApiError::Io(err)
    }
}

#[derive(Debug)]
pub struct ApiClient {
    endpoint: Endpoint,
    next_id: AtomicU64,
}

impl ApiClient {
    pub fn new(endpoint: Endpoint) -> Self {
        Self {
            endpoint,
            next_id: AtomicU64::new(1),
        }
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Sends one request and returns its `result` object.
    pub fn request(&self, method: &str, params: Value) -> Result<Value, ApiError> {
        let mut reader = self.send(method, params)?;
        let response = read_json_line(&mut reader)?
            .ok_or_else(|| ApiError::InvalidResponse("connection closed before response".into()))?;
        into_result(response)
    }

    /// `session.snapshot`: workspaces, tabs, panes, and focus in one call.
    pub fn session_snapshot(&self) -> Result<SessionSnapshot, ApiError> {
        let result = self.request("session.snapshot", json!({}))?;
        let snapshot = result
            .get("snapshot")
            .cloned()
            .ok_or_else(|| ApiError::InvalidResponse("session_snapshot without snapshot".into()))?;
        serde_json::from_value(snapshot).map_err(|err| ApiError::InvalidResponse(err.to_string()))
    }

    /// `workspace.list`.
    pub fn workspace_list(&self) -> Result<Vec<Workspace>, ApiError> {
        let result = self.request("workspace.list", json!({}))?;
        let workspaces = result
            .get("workspaces")
            .cloned()
            .ok_or_else(|| ApiError::InvalidResponse("workspace_list without workspaces".into()))?;
        serde_json::from_value(workspaces).map_err(|err| ApiError::InvalidResponse(err.to_string()))
    }

    /// `events.subscribe`. `subscriptions` is herdr's subscription array, for example
    /// `[{"type": "workspace.created"}]`. The stream yields event envelopes after the
    /// `subscription_started` acknowledgement.
    pub fn subscribe(&self, subscriptions: Value) -> Result<EventStream, ApiError> {
        let mut reader = self.send(
            "events.subscribe",
            json!({ "subscriptions": subscriptions }),
        )?;
        let ack = read_json_line(&mut reader)?
            .ok_or_else(|| ApiError::InvalidResponse("connection closed before ack".into()))?;
        let result = into_result(ack)?;
        if result.get("type").and_then(Value::as_str) != Some("subscription_started") {
            return Err(ApiError::InvalidResponse(format!(
                "expected subscription_started, got {result}"
            )));
        }
        Ok(EventStream { reader })
    }

    fn send(&self, method: &str, params: Value) -> Result<BufReader<LocalStream>, ApiError> {
        let id = format!(
            "herdr-shell:{}",
            self.next_id.fetch_add(1, Ordering::Relaxed)
        );
        let mut line = serde_json::to_vec(&json!({ "id": id, "method": method, "params": params }))
            .map_err(|err| ApiError::InvalidResponse(err.to_string()))?;
        line.push(b'\n');
        let mut stream = self.endpoint.connect()?;
        stream.write_all(&line)?;
        stream.flush()?;
        Ok(BufReader::new(stream))
    }
}

/// Live `events.subscribe` stream. Each item is one event envelope
/// (`{"event": ..., "data": ...}`); a server error line ends the stream with `Err`.
pub struct EventStream {
    reader: BufReader<LocalStream>,
}

impl Iterator for EventStream {
    type Item = Result<Value, ApiError>;

    fn next(&mut self) -> Option<Self::Item> {
        match read_json_line(&mut self.reader) {
            Ok(Some(value)) if value.get("error").is_some() => Some(into_result(value)),
            Ok(Some(value)) => Some(Ok(value)),
            Ok(None) => None,
            Err(err) => Some(Err(err)),
        }
    }
}

fn read_json_line(reader: &mut impl BufRead) -> Result<Option<Value>, ApiError> {
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        if !line.trim().is_empty() {
            break;
        }
    }
    serde_json::from_str(&line)
        .map(Some)
        .map_err(|err| ApiError::InvalidResponse(err.to_string()))
}

fn into_result(mut response: Value) -> Result<Value, ApiError> {
    if let Some(error) = response.get("error") {
        return Err(ApiError::Server {
            code: error
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            message: error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        });
    }
    response
        .get_mut("result")
        .map(Value::take)
        .ok_or_else(|| ApiError::InvalidResponse(format!("response without result: {response}")))
}

/// The fields the Shell reads from herdr `WorkspaceInfo`; everything else stays in `extra`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Workspace {
    pub workspace_id: String,
    #[serde(default)]
    pub number: u64,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub pane_count: u64,
    #[serde(default)]
    pub tab_count: u64,
    #[serde(default)]
    pub active_tab_id: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The fields the Shell reads from herdr `TabInfo`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Tab {
    pub tab_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub number: u64,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub focused: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The fields the Shell reads from herdr `PaneInfo`; `terminal_id` is the attach target.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Pane {
    pub pane_id: String,
    pub terminal_id: String,
    pub workspace_id: String,
    pub tab_id: String,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// herdr `SessionSnapshot`, with layouts and agents left as raw JSON.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SessionSnapshot {
    pub version: String,
    pub protocol: u32,
    #[serde(default)]
    pub focused_workspace_id: Option<String>,
    #[serde(default)]
    pub focused_tab_id: Option<String>,
    #[serde(default)]
    pub focused_pane_id: Option<String>,
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
    #[serde(default)]
    pub tabs: Vec<Tab>,
    #[serde(default)]
    pub panes: Vec<Pane>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
