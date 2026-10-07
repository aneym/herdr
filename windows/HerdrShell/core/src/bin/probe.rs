//! `herdr-shell-probe`: exercise the shell core against a live herdr.
//!
//! Endpoints come from `HERDR_SOCKET_PATH` (the API socket; the client socket is derived).
//!
//!   herdr-shell-probe api workspace-list
//!   herdr-shell-probe api session-snapshot
//!   herdr-shell-probe attach <terminal-id> --observe [--seconds N] [--cols C] [--rows R]
//!
//! `attach` without `--observe` is refused: the probe never writes to a pane.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use herdr_shell_core::api::ApiClient;
use herdr_shell_core::attach::{AttachClient, AttachEvent, AttachMode};
use herdr_shell_core::endpoint::{Endpoint, SOCKET_PATH_ENV_VAR};

const USAGE: &str = "usage:\n  herdr-shell-probe api workspace-list|session-snapshot\n  herdr-shell-probe attach <terminal-id> --observe [--seconds N] [--cols C] [--rows R]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("herdr-shell-probe: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let api_endpoint = Endpoint::api_from_env()
        .ok_or_else(|| format!("{SOCKET_PATH_ENV_VAR} is not set\n{USAGE}"))?;
    match args.first().map(String::as_str) {
        Some("api") => run_api(&api_endpoint, args.get(1).map(String::as_str)),
        Some("attach") => run_attach(
            &api_endpoint
                .client_for_api()
                .ok_or("TCP endpoints require an explicit client endpoint")?,
            &args[1..],
        ),
        _ => Err(USAGE.to_owned()),
    }
}

fn run_api(endpoint: &Endpoint, command: Option<&str>) -> Result<(), String> {
    let client = ApiClient::new(endpoint.clone());
    let method = match command {
        Some("workspace-list") => "workspace.list",
        Some("session-snapshot") => "session.snapshot",
        _ => return Err(USAGE.to_owned()),
    };
    let result = client
        .request(method, serde_json::json!({}))
        .map_err(|err| err.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&result).map_err(|err| err.to_string())?
    );
    Ok(())
}

fn run_attach(endpoint: &Endpoint, args: &[String]) -> Result<(), String> {
    let terminal_id = args
        .first()
        .filter(|arg| !arg.starts_with("--"))
        .ok_or_else(|| USAGE.to_owned())?;
    let mut observe = false;
    let mut seconds = 3u64;
    let mut cols = 120u16;
    let mut rows = 40u16;
    let mut rest = args[1..].iter();
    while let Some(flag) = rest.next() {
        let mut value = |name: &str| {
            rest.next()
                .ok_or_else(|| format!("{name} needs a value"))
                .cloned()
        };
        match flag.as_str() {
            "--observe" => observe = true,
            "--seconds" => seconds = parse(&value("--seconds")?)?,
            "--cols" => cols = parse(&value("--cols")?)?,
            "--rows" => rows = parse(&value("--rows")?)?,
            other => return Err(format!("unknown flag {other}\n{USAGE}")),
        }
    }
    if !observe {
        return Err("the probe only observes; pass --observe".into());
    }

    let client = AttachClient::connect(endpoint, terminal_id, cols, rows, AttachMode::Observe)
        .map_err(|err| {
            format!(
                "attach to {terminal_id} via {}: {err}",
                format!("{endpoint:?}")
            )
        })?;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut bytes: Vec<u8> = Vec::new();
    let (mut frames, mut mode_changes, mut bells) = (0u64, 0u64, 0u64);
    let mut closed = None;
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match client.recv_timeout(remaining.min(Duration::from_millis(100))) {
            Some(AttachEvent::Bytes(chunk)) => {
                frames += 1;
                bytes.extend_from_slice(&chunk);
            }
            Some(AttachEvent::ModeChange { .. }) => mode_changes += 1,
            Some(AttachEvent::Bell { count }) => bells += u64::from(count),
            Some(AttachEvent::Clipboard { data }) => eprintln!("clipboard: {data}"),
            Some(AttachEvent::Notice { message }) => eprintln!("notice: {message}"),
            Some(AttachEvent::Closed { reason }) => {
                closed = Some(reason);
                break;
            }
            None => {}
        }
    }
    let _ = client.detach();
    println!("terminal: {terminal_id}");
    println!("frames: {frames}");
    println!("bytes: {}", bytes.len());
    println!("mode_changes: {mode_changes}");
    println!("bells: {bells}");
    println!(
        "first_200: {}",
        bytes
            .iter()
            .take(200)
            .flat_map(|b| std::ascii::escape_default(*b))
            .map(char::from)
            .collect::<String>()
    );
    if let Some(reason) = closed {
        println!("closed: {reason}");
    }
    if bytes.is_empty() {
        return Err("no terminal bytes received".into());
    }
    Ok(())
}

fn parse<T: std::str::FromStr>(value: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("invalid number {value:?}"))
}
