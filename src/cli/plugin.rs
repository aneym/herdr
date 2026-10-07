use std::collections::HashMap;
use std::fmt;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::api::schema::{
    InstalledPluginInfo, Method, PluginActionInvokeParams, PluginActionListParams,
    PluginInvocationContext, PluginLinkParams, PluginListParams, PluginLogListParams,
    PluginPaneCloseParams, PluginPaneFocusParams, PluginPaneOpenParams, PluginPanePlacement,
    PluginPlatform, PluginSetEnabledParams, PluginSourceInfo, PluginSourceKind, PluginUnlinkParams,
    Request, ResponseResult, SplitDirection, SuccessResponse,
};
use crate::popup_size::PopupSize;

const PLUGIN_BUILD_OUTPUT_MAX_BYTES: usize = 64 * 1024;
const PLUGIN_INSTALL_USAGE: &str =
    "usage: herdr plugin install [--ref REF] [--yes|-y] <owner>/<repo>[/subdir...]";
const PLUGIN_RELOAD_USAGE: &str = "herdr plugin reload <PLUGIN_ID> --pane <PANE> --request <ID> --changelog <LINE> [--json] [--timeout <MS>] [--no-resume]";
const PLUGIN_RELOAD_DEFAULT_TIMEOUT_MS: u64 = 600_000;
/// Exit code for "the agent stayed busy; nothing was restarted, retry later".
const PLUGIN_RELOAD_EXIT_BUSY: i32 = 75;
/// Exit code for "plugin reloaded, but the pane runs no agent to resume" (for
/// example a hibernated pane); the caller wakes it with the changelog line.
const PLUGIN_RELOAD_EXIT_NO_AGENT: i32 = 3;
const PLUGIN_RELOAD_POLL: std::time::Duration = std::time::Duration::from_secs(1);
const PLUGIN_RELOAD_INPUT_QUIET_MS: u64 = 20_000;
/// How long past --timeout to wait for the server to expire a held prompt.
const PLUGIN_RELOAD_EXPIRY_GRACE: std::time::Duration = std::time::Duration::from_secs(5);
const PLUGIN_RELOAD_ACTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

pub(super) fn run_plugin_command(args: &[String]) -> std::io::Result<i32> {
    let Some(subcommand) = args.first().map(|arg| arg.as_str()) else {
        print_plugin_help();
        return Ok(2);
    };

    match subcommand {
        "install" => plugin_install(&args[1..]),
        "uninstall" => plugin_uninstall(&args[1..]),
        "link" => plugin_link(&args[1..]),
        "list" => plugin_list(&args[1..]),
        "config-dir" => plugin_config_dir_command(&args[1..]),
        "unlink" => plugin_unlink(&args[1..]),
        "enable" => plugin_set_enabled(&args[1..], true),
        "disable" => plugin_set_enabled(&args[1..], false),
        "reload" => plugin_reload(&args[1..]),
        "action" => run_plugin_action_command(&args[1..]),
        "log" | "logs" => plugin_log_list(&args[1..]),
        "pane" => run_plugin_pane_command(&args[1..]),
        "help" | "--help" | "-h" => {
            print_plugin_help();
            Ok(0)
        }
        _ => {
            print_plugin_help();
            Ok(2)
        }
    }
}

fn plugin_link(args: &[String]) -> std::io::Result<i32> {
    let Some(path) = args.first() else {
        eprintln!("usage: herdr plugin link <path> [--disabled]");
        return Ok(2);
    };
    let path = normalize_plugin_path_arg(path)?;
    let mut enabled = true;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--disabled" => {
                enabled = false;
                index += 1;
            }
            "--enabled" => {
                enabled = true;
                index += 1;
            }
            other => {
                eprintln!("unknown option: {other}");
                return Ok(2);
            }
        }
    }
    let params = PluginLinkParams {
        path,
        enabled,
        source: None,
    };
    let response = match super::send_request(&Request {
        id: "cli:plugin".into(),
        method: Method::PluginLink(params.clone()),
    }) {
        Ok(response) => response,
        Err(err) if is_connection_error(&err) => offline_plugin_link_response(&params)?,
        Err(err) => return Err(err),
    };
    super::print_response(&response)
}

fn plugin_config_dir_command(args: &[String]) -> std::io::Result<i32> {
    let Some(plugin_id) = args.first() else {
        eprintln!("usage: herdr plugin config-dir <plugin_id>");
        return Ok(2);
    };
    if args.len() != 1 {
        eprintln!("usage: herdr plugin config-dir <plugin_id>");
        return Ok(2);
    }
    let path = crate::plugin_paths::plugin_config_dir(plugin_id);
    crate::plugin_paths::ensure_plugin_user_dirs(plugin_id)?;
    println!("{}", path.display());
    Ok(0)
}

fn plugin_list(args: &[String]) -> std::io::Result<i32> {
    let mut plugin_id = None;
    let mut json = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" => {
                json = true;
                index += 1;
            }
            "--plugin" => {
                let Some(value) = required_value(args, &mut index, "--plugin") else {
                    return Ok(2);
                };
                plugin_id = Some(value);
            }
            other => {
                eprintln!("unknown option: {other}");
                return Ok(2);
            }
        }
    }
    let params = PluginListParams { plugin_id };
    let response = match super::send_request(&Request {
        id: "cli:plugin".into(),
        method: Method::PluginList(params.clone()),
    }) {
        Ok(response) => response,
        Err(err) if is_connection_error(&err) => offline_plugin_list_response(&params)?,
        Err(err) => return Err(err),
    };
    if json {
        return super::print_response(&response);
    }
    print_plugin_list_human(&response)
}

fn plugin_unlink(args: &[String]) -> std::io::Result<i32> {
    let Some(plugin_id) = args.first() else {
        eprintln!("usage: herdr plugin unlink <plugin_id>");
        return Ok(2);
    };
    if args.len() != 1 {
        eprintln!("usage: herdr plugin unlink <plugin_id>");
        return Ok(2);
    }
    print_plugin_response(Method::PluginUnlink(PluginUnlinkParams {
        plugin_id: plugin_id.clone(),
    }))
}

#[derive(Debug)]
struct PluginInstallArgs {
    source: GithubPluginSource,
    requested_ref: Option<String>,
    yes: bool,
}

fn parse_plugin_install_args(args: &[String]) -> Result<PluginInstallArgs, String> {
    let mut source_arg = None;
    let mut requested_ref = None;
    let mut yes = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--ref" => {
                let value = args.get(index + 1).ok_or("missing value for --ref")?;
                requested_ref = Some(value.clone());
                index += 2;
            }
            "--yes" | "-y" => {
                yes = true;
                index += 1;
            }
            other if other.starts_with('-') || source_arg.is_some() => {
                return Err(format!("unknown option: {other}"));
            }
            other => {
                source_arg = Some(other);
                index += 1;
            }
        }
    }
    let source = GithubPluginSource::parse(source_arg.ok_or(PLUGIN_INSTALL_USAGE)?)?;
    Ok(PluginInstallArgs {
        source,
        requested_ref,
        yes,
    })
}

fn plugin_install(args: &[String]) -> std::io::Result<i32> {
    let PluginInstallArgs {
        source,
        requested_ref,
        yes,
    } = match parse_plugin_install_args(args) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("{err}");
            return Ok(2);
        }
    };

    if !yes && !io::stdin().is_terminal() {
        eprintln!("remote plugin install requires --yes when stdin is not interactive");
        return Ok(2);
    }

    let temp_root = create_plugin_temp_dir("install")?;
    let checkout = temp_root.join("checkout");
    let install_result = (|| {
        git_checkout(&source, requested_ref.as_deref(), &checkout)?;
        let resolved_commit = git_output(&checkout, ["rev-parse", "HEAD"])?;
        let manifest_root = source.manifest_root(&checkout);
        let preview_plugin = load_cli_plugin_manifest(&manifest_root, true)?;
        let existing = installed_plugin_info(&preview_plugin.plugin_id)?;
        ensure_replacement_allowed(&preview_plugin, existing.as_ref())?;

        let mut source_info =
            source.to_source_info(requested_ref, resolved_commit, None, current_unix_ms());
        print_install_preview(&preview_plugin, &source_info, existing.as_ref());
        if !yes && !confirm("Install this plugin?")? {
            eprintln!("plugin install cancelled");
            return Ok(0);
        }
        if let Err(err) = run_plugin_build_commands(&preview_plugin, &manifest_root) {
            eprintln!("{err}");
            return Ok(1);
        }
        let post_build_plugin = load_cli_plugin_manifest(&manifest_root, true)?;
        ensure_manifest_unchanged_after_build(&preview_plugin, &post_build_plugin)?;

        let final_checkout = crate::plugin_paths::managed_checkout_path(&preview_plugin.plugin_id);
        let backup_checkout = temp_root.join("previous-checkout");
        let mut backup_moved = false;
        if final_checkout.exists() {
            std::fs::rename(&final_checkout, &backup_checkout)
                .map_err(|err| plugin_checkout_lifecycle_error("replace", &final_checkout, err))?;
            backup_moved = true;
        }
        let install_attempt = (|| {
            if let Some(parent) = final_checkout.parent() {
                std::fs::create_dir_all(parent).map_err(InstallFailure::Rollback)?;
            }
            std::fs::rename(&checkout, &final_checkout)
                .map_err(|err| plugin_checkout_lifecycle_error("install", &final_checkout, err))
                .map_err(InstallFailure::Rollback)?;

            source_info.managed_path = Some(final_checkout.display().to_string());
            let final_manifest_root = source.manifest_root(&final_checkout);
            let mut plugin = load_cli_plugin_manifest(&final_manifest_root, true)
                .map_err(InstallFailure::Rollback)?;
            plugin.source = source_info.clone();
            register_installed_plugin(plugin.clone(), source_info.clone())?;
            Ok::<InstalledPluginInfo, InstallFailure>(plugin)
        })();
        let plugin = match install_attempt {
            Ok(plugin) => plugin,
            Err(InstallFailure::Rollback(err)) => {
                let _ = std::fs::remove_dir_all(&final_checkout);
                if backup_moved && backup_checkout.exists() {
                    let _ = std::fs::rename(&backup_checkout, &final_checkout);
                }
                return Err(err);
            }
            Err(InstallFailure::KeepCheckout(err)) => return Err(err),
        };
        println!("Installed {} from {}.", plugin.plugin_id, source.display());
        println!(
            "Config: {}",
            crate::plugin_paths::plugin_config_dir(&plugin.plugin_id).display()
        );
        Ok(0)
    })();
    let _ = std::fs::remove_dir_all(&temp_root);
    install_result
}

fn plugin_uninstall(args: &[String]) -> std::io::Result<i32> {
    let Some(target) = args.first() else {
        eprintln!("usage: herdr plugin uninstall <plugin_id|owner/repo[/subdir...]>");
        return Ok(2);
    };
    if args.len() != 1 {
        eprintln!("usage: herdr plugin uninstall <plugin_id|owner/repo[/subdir...]>");
        return Ok(2);
    }

    let (plugin_id, existing) = match GithubPluginSource::parse(target) {
        Ok(source) => {
            let existing = installed_plugin_by_github_source(&source)?;
            let Some(existing) = existing else {
                eprintln!("plugin not installed: {}", source.display());
                return Ok(1);
            };
            (existing.plugin_id.clone(), Some(existing))
        }
        Err(_) => {
            let existing = match live_installed_plugin_info(target) {
                Ok(plugin) => plugin,
                Err(err) if is_connection_error(&err) => registry_plugin_info(target),
                Err(err) => return Err(err),
            };
            (target.clone(), existing)
        }
    };

    match super::send_request(&Request {
        id: "cli:plugin".into(),
        method: Method::PluginUnlink(PluginUnlinkParams {
            plugin_id: plugin_id.clone(),
        }),
    }) {
        Ok(response) => {
            if response.get("error").is_some() {
                return super::print_response(&response);
            }
            if response["result"]["removed"].as_bool() == Some(false) {
                eprintln!("plugin not installed: {target}");
                return Ok(1);
            }
        }
        Err(err) if is_connection_error(&err) => {
            let (removed, _) = crate::persist::plugin_registry::update(|plugins| {
                let before = plugins.len();
                plugins.retain(|plugin| plugin.plugin_id != plugin_id);
                before != plugins.len()
            })?;
            if !removed {
                eprintln!("plugin not installed: {target}");
                return Ok(1);
            }
        }
        Err(err) => return Err(err),
    }

    if let Some(plugin) = existing.as_ref() {
        remove_managed_plugin_files(plugin)?;
    }
    println!("Uninstalled {plugin_id}.");
    Ok(0)
}

fn plugin_set_enabled(args: &[String], enabled: bool) -> std::io::Result<i32> {
    let Some(plugin_id) = args.first() else {
        eprintln!(
            "usage: herdr plugin {} <plugin_id>",
            if enabled { "enable" } else { "disable" }
        );
        return Ok(2);
    };
    if args.len() != 1 {
        eprintln!(
            "usage: herdr plugin {} <plugin_id>",
            if enabled { "enable" } else { "disable" }
        );
        return Ok(2);
    }
    let params = PluginSetEnabledParams {
        plugin_id: plugin_id.clone(),
    };
    if enabled {
        print_plugin_response(Method::PluginEnable(params))
    } else {
        print_plugin_response(Method::PluginDisable(params))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PluginReloadArgs {
    plugin_id: String,
    pane: String,
    request: String,
    changelog: String,
    timeout_ms: u64,
    /// Prompt the running agent instead of restarting it with agent.resume.
    no_resume: bool,
}

/// Parse `plugin reload` arguments; `Err` carries the exit code and its one
/// stderr line.
fn parse_plugin_reload_args(args: &[String]) -> Result<PluginReloadArgs, (i32, String)> {
    let usage = || (1, format!("usage: {PLUGIN_RELOAD_USAGE}"));
    let mut plugin_id = None;
    let mut pane = None;
    let mut request = None;
    let mut changelog = None;
    let mut timeout_ms = PLUGIN_RELOAD_DEFAULT_TIMEOUT_MS;
    let mut no_resume = false;
    let mut index = 0;
    while index < args.len() {
        let value = || args.get(index + 1).cloned();
        match args[index].as_str() {
            "--pane" => pane = Some(value().ok_or_else(usage)?),
            "--request" => request = Some(value().ok_or_else(usage)?),
            "--changelog" => changelog = Some(value().ok_or_else(usage)?),
            "--timeout" => {
                let raw = value().ok_or_else(usage)?;
                timeout_ms = raw
                    .parse::<u64>()
                    .map_err(|_| (1, format!("invalid --timeout value: {raw}")))?;
            }
            // Output is always exactly one JSON object; `--json` is accepted
            // so callers can say so.
            "--json" => {
                index += 1;
                continue;
            }
            "--no-resume" => {
                no_resume = true;
                index += 1;
                continue;
            }
            other if other.starts_with('-') => {
                return Err((1, format!("unknown option: {other}")));
            }
            other if plugin_id.is_none() => {
                plugin_id = Some(other.to_string());
                index += 1;
                continue;
            }
            _ => return Err(usage()),
        }
        index += 2;
    }
    let (Some(plugin_id), Some(pane), Some(request), Some(changelog)) =
        (plugin_id, pane, request, changelog)
    else {
        return Err(usage());
    };
    if !valid_reload_request_id(&request) {
        return Err((
            1,
            format!("invalid --request id {request:?}: use 1-128 of A-Z a-z 0-9 . _ -"),
        ));
    }
    Ok(PluginReloadArgs {
        plugin_id,
        pane,
        request,
        changelog,
        timeout_ms,
        no_resume,
    })
}

fn valid_reload_request_id(request: &str) -> bool {
    (1..=128).contains(&request.len())
        && request
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn plugin_reload_record_path(plugin_id: &str, request: &str) -> PathBuf {
    crate::plugin_paths::plugin_state_dir(plugin_id)
        .join("reloads")
        .join(format!("{request}.json"))
}

/// A finished reload for this request, if one was recorded.
fn completed_plugin_reload_record(path: &Path) -> Option<serde_json::Value> {
    let record: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    plugin_reload_delivered(&record).then_some(record)
}

/// Whether a reload record finished: the agent was resumed, or (with
/// `--no-resume`) the running agent was prompted.
fn plugin_reload_delivered(record: &serde_json::Value) -> bool {
    ["resumed", "prompted"]
        .into_iter()
        .any(|field| record.get(field) == Some(&serde_json::Value::Bool(true)))
}

fn plugin_reload_prompt(changelog: &str, request: &str) -> String {
    format!(
        "Your plugin was updated: {changelog}, request {request} done. Run its check, then `agent-request confirm {request}` (or `--fail \"<why>\"`)."
    )
}

fn utc_now_iso() -> String {
    let now = time::OffsetDateTime::now_utc();
    let now = now.replace_nanosecond(0).unwrap_or(now);
    now.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| now.unix_timestamp().to_string())
}

const NO_RUNTIME_ID: &str = "the herdr server does not report the agent runtime_id, so it cannot guard the reload prompt; upgrade the herdr server";

struct PluginReloadFailure {
    exit_code: i32,
    message: String,
}

impl PluginReloadFailure {
    fn failed(message: impl Into<String>) -> Self {
        Self {
            exit_code: 1,
            message: message.into(),
        }
    }
}

#[cfg(test)]
type PluginReloadTestCall = Box<dyn FnMut(&Method) -> Result<serde_json::Value, (String, String)>>;

#[cfg(test)]
thread_local! {
    /// Scripted server for `plugin reload` flow tests.
    static PLUGIN_RELOAD_TEST_CALL: std::cell::RefCell<Option<PluginReloadTestCall>> =
        const { std::cell::RefCell::new(None) };
}

/// One API call; `Err` carries the error code and message.
fn plugin_reload_call(method: Method) -> Result<serde_json::Value, (String, String)> {
    #[cfg(test)]
    if let Some(result) =
        PLUGIN_RELOAD_TEST_CALL.with(|call| call.borrow_mut().as_mut().map(|call| call(&method)))
    {
        return result;
    }
    let response = super::send_request(&Request {
        id: "cli:plugin:reload".into(),
        method,
    })
    .map_err(|err| ("request_failed".to_string(), err.to_string()))?;
    if let Some(error) = response.get("error") {
        let field = |name: &str| {
            error
                .get(name)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        return Err((field("code"), field("message")));
    }
    Ok(response
        .get("result")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

fn plugin_reload_step(
    step: &str,
    method: Method,
) -> Result<serde_json::Value, PluginReloadFailure> {
    plugin_reload_call(method).map_err(|(code, message)| {
        PluginReloadFailure::failed(format!("{step}: {code}: {message}"))
    })
}

fn plugin_reload(args: &[String]) -> std::io::Result<i32> {
    let args = match parse_plugin_reload_args(args) {
        Ok(args) => args,
        Err((exit_code, message)) => {
            eprintln!("{message}");
            return Ok(exit_code);
        }
    };
    let record_path = plugin_reload_record_path(&args.plugin_id, &args.request);
    std::fs::create_dir_all(
        record_path
            .parent()
            .ok_or_else(|| std::io::Error::other("missing reload record directory"))?,
    )?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(record_path.with_extension("lock"))?;
    // The OS releases the exclusive lock even if the CLI crashes.
    lock.lock()?;
    if let Some(record) = completed_plugin_reload_record(&record_path) {
        if record.get("pane").and_then(serde_json::Value::as_str) != Some(args.pane.as_str()) {
            eprintln!(
                "reload request {} belongs to pane {}, expected {}",
                args.request,
                record.get("pane").unwrap_or(&serde_json::Value::Null),
                args.pane
            );
            return Ok(1);
        }
        println!("{record}");
        return Ok(0);
    }
    match run_plugin_reload(&args, &record_path) {
        Ok(record) => {
            println!("{record}");
            if plugin_reload_delivered(&record) {
                Ok(0)
            } else {
                Ok(PLUGIN_RELOAD_EXIT_NO_AGENT)
            }
        }
        Err(failure) => {
            eprintln!("{}", failure.message);
            Ok(failure.exit_code)
        }
    }
}

fn run_plugin_reload(
    args: &PluginReloadArgs,
    record_path: &Path,
) -> Result<serde_json::Value, PluginReloadFailure> {
    let prior: Option<serde_json::Value> = std::fs::read_to_string(record_path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    if !prior.is_some_and(|record| record.get("resumed") == Some(&serde_json::Value::Bool(false))) {
        plugin_reload_step(
            "plugin.reload",
            Method::PluginReload(crate::api::schema::PluginReloadParams {
                plugin_id: args.plugin_id.clone(),
            }),
        )?;
        run_plugin_reload_action(args)?;
    }

    let deadline = std::time::Instant::now()
        .checked_add(std::time::Duration::from_millis(args.timeout_ms))
        .ok_or_else(|| PluginReloadFailure::failed("--timeout is too large"))?;
    // The agent to prompt: (agent, session id, whether agent.resume restarted it).
    let (agent_label, session_id, resumed) = loop {
        let agent = match plugin_reload_call(Method::AgentGet(crate::api::schema::AgentTarget {
            target: args.pane.clone(),
        })) {
            Ok(agent) => agent,
            Err((code, _)) if code == "agent_not_found" => {
                let record = serde_json::json!({
                    "plugin_id": args.plugin_id,
                    "reloaded_at": utc_now_iso(),
                    "pane": args.pane,
                    "session_id": serde_json::Value::Null,
                    "resumed": false,
                    "request": args.request,
                    "changelog": args.changelog,
                });
                write_plugin_reload_record(record_path, &record).map_err(|err| {
                    PluginReloadFailure::failed(format!("failed to persist reload: {err}"))
                })?;
                return Ok(record);
            }
            Err((code, message)) => {
                return Err(PluginReloadFailure::failed(format!(
                    "agent.get: {code}: {message}"
                )));
            }
        };
        let status = agent
            .get("agent")
            .and_then(|agent| agent.get("agent_status"))
            .and_then(serde_json::Value::as_str);
        if !matches!(status, Some("idle" | "done")) {
            if std::time::Instant::now() + PLUGIN_RELOAD_POLL > deadline {
                return Err(PluginReloadFailure {
                    exit_code: PLUGIN_RELOAD_EXIT_BUSY,
                    message: "agent busy".into(),
                });
            }
            std::thread::sleep(PLUGIN_RELOAD_POLL);
            continue;
        }
        // Fail closed before restarting or prompting anything: a server that
        // does not report runtime identity cannot bind the prompt to it.
        if agent
            .get("agent")
            .and_then(|agent| agent.get("runtime_id"))
            .and_then(serde_json::Value::as_str)
            .is_none()
        {
            return Err(PluginReloadFailure::failed(NO_RUNTIME_ID));
        }
        if args.no_resume {
            // Prompt the running agent as is; agent.prompt's polite send
            // holds the line until the human is quiet.
            let info = agent.get("agent");
            let field = |name: &str| {
                info.and_then(|info| info.get(name))
                    .and_then(serde_json::Value::as_str)
            };
            let session = info
                .and_then(|info| info.get("agent_session"))
                .and_then(|session| session.get("value"))
                .and_then(serde_json::Value::as_str);
            let (Some("claude"), Some(session)) = (field("agent"), session) else {
                return Err(PluginReloadFailure::failed(
                    "--no-resume: the pane agent is not claude with a known session",
                ));
            };
            break ("claude".to_string(), session.to_string(), false);
        }
        // Recheck status and the existing per-terminal human-input timestamp
        // atomically at the mutation boundary, not just in this client poll.
        match plugin_reload_call(Method::AgentResume(crate::api::schema::AgentResumeParams {
            pane_id: args.pane.clone(),
            input_quiet_ms: Some(PLUGIN_RELOAD_INPUT_QUIET_MS),
        })) {
            Ok(resumed) => {
                let field = |name: &str| {
                    resumed
                        .get(name)
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string()
                };
                tracing::info!(
                    plugin_id = %args.plugin_id,
                    pane = %args.pane,
                    request = %args.request,
                    "resumed agent after plugin reload"
                );
                break (field("agent"), field("session_id"), true);
            }
            Err((code, _)) if code == "agent_busy" => {
                if std::time::Instant::now() + PLUGIN_RELOAD_POLL > deadline {
                    return Err(PluginReloadFailure {
                        exit_code: PLUGIN_RELOAD_EXIT_BUSY,
                        message: "agent busy".into(),
                    });
                }
                std::thread::sleep(PLUGIN_RELOAD_POLL);
            }
            Err((code, message)) => {
                return Err(PluginReloadFailure::failed(format!(
                    "agent.resume: {code}: {message}"
                )));
            }
        }
    };
    // Re-verify right before typing: the pane still runs this agent on this
    // session, idle or done, with no restore error. The prompt is bound to
    // the runtime seen here, so a later resume of the same session (a new
    // runtime) never receives it.
    let runtime_id = wait_for_resumed_agent_idle(
        &ResumedAgent {
            pane: &args.pane,
            agent: &agent_label,
            session_id: &session_id,
        },
        deadline,
    )?;
    let prompt = plugin_reload_step(
        "agent.prompt",
        Method::AgentPrompt(crate::api::schema::AgentPromptParams {
            if_idle: false,
            target: args.pane.clone(),
            text: plugin_reload_prompt(&args.changelog, &args.request),
            wait: None,
            // The server holds the line until this session is idle and the
            // human is quiet, drops it if the session or agent process
            // changes, and expires it at --timeout so a retry never finds a
            // second copy still queued.
            delivery: Some(crate::api::schema::AgentPromptDelivery {
                session_id: session_id.clone(),
                runtime_id: Some(runtime_id.clone()),
                input_quiet_ms: PLUGIN_RELOAD_INPUT_QUIET_MS,
                expires_ms: u64::try_from(
                    deadline
                        .saturating_duration_since(std::time::Instant::now())
                        .as_millis(),
                )
                .unwrap_or(u64::MAX),
            }),
        }),
    )?;

    // A server that ignores `delivery` would type the line unguarded; the
    // runtime_id check above refuses such servers before anything is sent,
    // and the ack proves this one applied the binding.
    let acked = prompt.get("delivery");
    let acked_field = |name: &str| {
        acked
            .and_then(|ack| ack.get(name))
            .and_then(serde_json::Value::as_str)
    };
    if acked_field("session_id") != Some(session_id.as_str())
        || acked_field("runtime_id") != Some(runtime_id.as_str())
    {
        return Err(PluginReloadFailure::failed(
            "agent.prompt: the herdr server did not acknowledge the session-bound delivery; upgrade the herdr server",
        ));
    }

    let delivery = wait_for_plugin_reload_prompt(&args.pane, prompt, deadline)?;

    let mut record = serde_json::json!({
        "plugin_id": args.plugin_id,
        "reloaded_at": utc_now_iso(),
        "pane": args.pane,
        "session_id": session_id,
        "resumed": resumed,
        "request": args.request,
        "changelog": args.changelog,
        "delivery": delivery,
    });
    if !resumed {
        record["prompted"] = serde_json::Value::Bool(true);
    }
    write_plugin_reload_record(record_path, &record).map_err(|err| {
        PluginReloadFailure::failed(format!("failed to write {}: {err}", record_path.display()))
    })?;
    Ok(record)
}

/// Run the plugin's optional `reload` action with the pane as context and
/// wait for it to finish.
fn run_plugin_reload_action(args: &PluginReloadArgs) -> Result<(), PluginReloadFailure> {
    let actions = plugin_reload_step(
        "plugin.action.list",
        Method::PluginActionList(PluginActionListParams {
            plugin_id: Some(args.plugin_id.clone()),
        }),
    )?;
    let has_reload_action = actions
        .get("actions")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|actions| {
            actions.iter().any(|action| {
                action.get("action_id").and_then(serde_json::Value::as_str) == Some("reload")
            })
        });
    if !has_reload_action {
        return Ok(());
    }
    let pane = plugin_reload_call(Method::PaneGet(crate::api::schema::PaneTarget {
        pane_id: args.pane.clone(),
    }))
    .ok();
    let pane_field = |name: &str| {
        pane.as_ref()
            .and_then(|pane| pane.get("pane"))
            .and_then(|pane| pane.get(name))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    let invoked = plugin_reload_step(
        "plugin.action.invoke",
        Method::PluginActionInvoke(PluginActionInvokeParams {
            action_id: "reload".into(),
            plugin_id: Some(args.plugin_id.clone()),
            context: Some(PluginInvocationContext {
                workspace_id: pane_field("workspace_id"),
                workspace_label: None,
                workspace_cwd: None,
                worktree: None,
                tab_id: pane_field("tab_id"),
                tab_label: None,
                focused_pane_id: Some(args.pane.clone()),
                focused_pane_cwd: pane_field("cwd"),
                focused_pane_agent: pane_field("agent"),
                focused_pane_status: None,
                selected_text: None,
                invocation_source: Some("plugin-reload".into()),
                correlation_id: Some(args.request.clone()),
                clicked_url: None,
                link_handler_id: None,
            }),
        }),
    )?;
    let Some(log_id) = invoked
        .get("log")
        .and_then(|log| log.get("log_id"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
    else {
        return Err(PluginReloadFailure::failed(
            "plugin.action.invoke: response has no log id",
        ));
    };
    let deadline = std::time::Instant::now() + PLUGIN_RELOAD_ACTION_TIMEOUT;
    loop {
        let logs = plugin_reload_step(
            "plugin.log.list",
            Method::PluginLogList(PluginLogListParams {
                plugin_id: Some(args.plugin_id.clone()),
                limit: Some(200),
            }),
        )?;
        let log = logs
            .get("logs")
            .and_then(serde_json::Value::as_array)
            .and_then(|logs| {
                logs.iter().find(|log| {
                    log.get("log_id").and_then(serde_json::Value::as_str) == Some(log_id.as_str())
                })
            })
            .cloned();
        match log
            .as_ref()
            .and_then(|log| log.get("status"))
            .and_then(serde_json::Value::as_str)
        {
            Some("succeeded") => return Ok(()),
            Some("failed") => {
                let detail = log
                    .as_ref()
                    .and_then(|log| {
                        log.get("error")
                            .or_else(|| log.get("stderr"))
                            .and_then(serde_json::Value::as_str)
                    })
                    .unwrap_or_default()
                    .lines()
                    .last()
                    .unwrap_or_default()
                    .to_string();
                return Err(PluginReloadFailure::failed(format!(
                    "reload action failed: {detail}"
                )));
            }
            _ if std::time::Instant::now() >= deadline => {
                return Err(PluginReloadFailure::failed(
                    "reload action did not finish within 120 s",
                ));
            }
            _ => std::thread::sleep(PLUGIN_RELOAD_POLL),
        }
    }
}

/// The restore error a `pane.get` result reports for the resumed pane, set
/// when its replacement runtime failed (`agent.resume` clears it on start).
fn resumed_pane_failure(pane: &serde_json::Value) -> Option<String> {
    pane.get("pane")
        .and_then(|pane| pane.get("restore_error"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

/// The agent `agent.resume` restarted, as the post-resume wait must see it in
/// the pane before anything is typed there.
struct ResumedAgent<'a> {
    pane: &'a str,
    agent: &'a str,
    session_id: &'a str,
}

/// Poll until the resumed agent itself is the pane's agent, on the resumed
/// session, and idle or done. Fails at once when the pane reports a restore
/// error, is gone, or loses the agent after it was seen, so the caller never
/// types the prompt into whatever replaced it. Returns the runtime_id the
/// agent was seen ready on.
fn wait_for_resumed_agent_idle(
    resumed: &ResumedAgent<'_>,
    deadline: std::time::Instant,
) -> Result<String, PluginReloadFailure> {
    let mut agent_seen = false;
    loop {
        match plugin_reload_call(Method::PaneGet(crate::api::schema::PaneTarget {
            pane_id: resumed.pane.to_string(),
        })) {
            Ok(pane) => {
                if let Some(error) = resumed_pane_failure(&pane) {
                    return Err(PluginReloadFailure::failed(format!(
                        "resumed agent failed: {error}"
                    )));
                }
            }
            Err((code, message)) if code == "pane_not_found" => {
                return Err(PluginReloadFailure::failed(format!(
                    "resumed agent failed: {code}: {message}"
                )));
            }
            Err(_) => {}
        }
        let agent = plugin_reload_call(Method::AgentGet(crate::api::schema::AgentTarget {
            target: resumed.pane.to_string(),
        }))
        .ok();
        let info = agent.as_ref().and_then(|result| result.get("agent"));
        let field = |name: &str| {
            info.and_then(|info| info.get(name))
                .and_then(serde_json::Value::as_str)
        };
        let is_resumed_agent = field("agent") == Some(resumed.agent)
            && info
                .and_then(|info| info.get("agent_session"))
                .and_then(|session| session.get("value"))
                .and_then(serde_json::Value::as_str)
                == Some(resumed.session_id);
        if is_resumed_agent && matches!(field("agent_status"), Some("idle" | "done")) {
            // Fail closed: a server without runtime identity cannot bind the
            // prompt to this runtime, and may ignore the binding entirely.
            return field("runtime_id")
                .map(str::to_string)
                .ok_or_else(|| PluginReloadFailure::failed(NO_RUNTIME_ID));
        }
        if is_resumed_agent {
            agent_seen = true;
        } else if agent_seen {
            return Err(PluginReloadFailure::failed(format!(
                "resumed agent failed: {} is no longer the agent in pane {}",
                resumed.agent, resumed.pane
            )));
        }
        if std::time::Instant::now() >= deadline {
            let message = "resumed agent did not report idle or done before --timeout";
            // Seen but still busy is retryable; never seen is a failure.
            return Err(if agent_seen {
                PluginReloadFailure {
                    exit_code: PLUGIN_RELOAD_EXIT_BUSY,
                    message: message.into(),
                }
            } else {
                PluginReloadFailure::failed(message)
            });
        }
        std::thread::sleep(PLUGIN_RELOAD_POLL);
    }
}

/// How a session-bound prompt left the server: written to the pane, or handed
/// to the PTY writer and possibly still landing (never treated as undelivered,
/// so a retry cannot type it twice).
const PROMPT_DELIVERED: &str = "delivered";
const PROMPT_HANDED_TO_WRITER: &str = "handed_to_writer";

/// Wait for the session-bound prompt to reach a final state. The server
/// expires it at `deadline`; if it is somehow still held after a short grace,
/// the CLI cancels it. Expiry or cancellation (agent busy or human not quiet)
/// is retryable; anything else fails.
fn wait_for_plugin_reload_prompt(
    pane: &str,
    mut prompt: serde_json::Value,
    deadline: std::time::Instant,
) -> Result<&'static str, PluginReloadFailure> {
    let id = prompt
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let settle_by = deadline + PLUGIN_RELOAD_EXPIRY_GRACE;
    let mut cancelled = false;
    // Once the line was handed to the writer it may still land, so losing
    // its receipt later (history eviction under backpressure) means handed,
    // never undelivered: a retry must not type it twice.
    let mut handed_seen = false;
    loop {
        let reason = prompt
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        handed_seen |= reason == PROMPT_HANDED_TO_WRITER;
        match prompt.get("state").and_then(serde_json::Value::as_str) {
            Some("delivered" | "acked") => return Ok(PROMPT_DELIVERED),
            Some("queued") if reason == PROMPT_HANDED_TO_WRITER && cancelled => {
                return Ok(PROMPT_HANDED_TO_WRITER);
            }
            Some("queued") if !cancelled => {}
            Some("queued") => {
                return Err(PluginReloadFailure::failed(
                    "agent.prompt is still queued past its expiry and could not be cancelled; not retrying blind",
                ));
            }
            Some("dropped") if matches!(reason, "expired" | "cancelled") => {
                return Err(PluginReloadFailure {
                    exit_code: PLUGIN_RELOAD_EXIT_BUSY,
                    message: "agent busy or pane input active until --timeout; the line was not delivered".into(),
                });
            }
            state => {
                return Err(PluginReloadFailure::failed(format!(
                    "agent.prompt was not delivered: {} {reason}",
                    state.unwrap_or("unknown")
                )));
            }
        }
        let past_grace = std::time::Instant::now() >= settle_by;
        if past_grace && reason == PROMPT_HANDED_TO_WRITER {
            return Ok(PROMPT_HANDED_TO_WRITER);
        }
        if !past_grace {
            std::thread::sleep(PLUGIN_RELOAD_POLL);
        }
        // Past the grace the server should have expired it; cancel it so a
        // retry never finds a second copy, then report what the cancel saw.
        cancelled = past_grace;
        let id = id
            .as_ref()
            .ok_or_else(|| PluginReloadFailure::failed("queued prompt has no id"))?;
        let queue =
            match plugin_reload_call(Method::PaneQueue(crate::api::schema::PaneQueueParams {
                pane_id: pane.into(),
                id: Some(id.clone()),
                flush: false,
                cancel: cancelled,
            })) {
                Ok(queue) => Some(queue),
                Err((code, _)) if code == "queue_item_not_found" => None,
                Err((code, message)) => {
                    return Err(PluginReloadFailure::failed(format!(
                        "pane.queue: {code}: {message}"
                    )));
                }
            };
        let found = queue.as_ref().and_then(|queue| {
            ["sends", "recent"]
                .into_iter()
                .filter_map(|key| queue.get(key).and_then(serde_json::Value::as_array))
                .flatten()
                .find(|send| {
                    send.get("id").and_then(serde_json::Value::as_str) == Some(id.as_str())
                })
                .cloned()
        });
        prompt = match found {
            Some(send) => send,
            None if handed_seen => return Ok(PROMPT_HANDED_TO_WRITER),
            None => {
                return Err(PluginReloadFailure::failed(
                    "queued prompt disappeared before delivery",
                ));
            }
        };
    }
}

fn write_plugin_reload_record(path: &Path, record: &serde_json::Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, format!("{record}\n"))?;
    std::fs::rename(&temp, path)
}

fn plugin_log_list(args: &[String]) -> std::io::Result<i32> {
    let mut plugin_id = None;
    let mut limit = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "list" if index == 0 => index += 1,
            "--plugin" => {
                let Some(value) = required_value(args, &mut index, "--plugin") else {
                    return Ok(2);
                };
                plugin_id = Some(value);
            }
            "--limit" => {
                let Some(raw) = required_value(args, &mut index, "--limit") else {
                    return Ok(2);
                };
                let Ok(parsed) = raw.parse::<usize>() else {
                    eprintln!("invalid --limit value: {raw}");
                    return Ok(2);
                };
                limit = Some(parsed);
            }
            other => {
                eprintln!("unknown option: {other}");
                return Ok(2);
            }
        }
    }
    print_plugin_response(Method::PluginLogList(PluginLogListParams {
        plugin_id,
        limit,
    }))
}

fn run_plugin_action_command(args: &[String]) -> std::io::Result<i32> {
    let Some(subcommand) = args.first().map(|arg| arg.as_str()) else {
        print_plugin_action_help();
        return Ok(2);
    };

    match subcommand {
        "list" => plugin_action_list(&args[1..]),
        "invoke" => plugin_action_invoke(&args[1..]),
        "help" | "--help" | "-h" => {
            print_plugin_action_help();
            Ok(0)
        }
        _ => {
            print_plugin_action_help();
            Ok(2)
        }
    }
}

fn plugin_action_list(args: &[String]) -> std::io::Result<i32> {
    let mut plugin_id = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--plugin" => {
                let Some(value) = required_value(args, &mut index, "--plugin") else {
                    return Ok(2);
                };
                plugin_id = Some(value);
            }
            other => {
                eprintln!("unknown option: {other}");
                return Ok(2);
            }
        }
    }

    print_plugin_response(Method::PluginActionList(PluginActionListParams {
        plugin_id,
    }))
}

fn plugin_action_invoke(args: &[String]) -> std::io::Result<i32> {
    let Some(action_id) = args.first() else {
        eprintln!("usage: herdr plugin action invoke <action_id> [--plugin ID]");
        return Ok(2);
    };
    let mut plugin_id = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--plugin" => {
                let Some(value) = required_value(args, &mut index, "--plugin") else {
                    return Ok(2);
                };
                plugin_id = Some(value);
            }
            other => {
                eprintln!("unknown option: {other}");
                return Ok(2);
            }
        }
    }

    print_plugin_response(Method::PluginActionInvoke(PluginActionInvokeParams {
        action_id: action_id.clone(),
        plugin_id,
        context: Some(PluginInvocationContext {
            workspace_id: None,
            workspace_label: None,
            workspace_cwd: None,
            worktree: None,
            tab_id: None,
            tab_label: None,
            focused_pane_id: None,
            focused_pane_cwd: None,
            focused_pane_agent: None,
            focused_pane_status: None,
            selected_text: None,
            invocation_source: Some("cli".into()),
            correlation_id: None,
            clicked_url: None,
            link_handler_id: None,
        }),
    }))
}

fn run_plugin_pane_command(args: &[String]) -> std::io::Result<i32> {
    let Some(subcommand) = args.first().map(|arg| arg.as_str()) else {
        print_plugin_pane_help();
        return Ok(2);
    };

    match subcommand {
        "open" => plugin_pane_open(&args[1..]),
        "focus" => plugin_pane_focus(&args[1..]),
        "close" => plugin_pane_close(&args[1..]),
        "help" | "--help" | "-h" => {
            print_plugin_pane_help();
            Ok(0)
        }
        _ => {
            print_plugin_pane_help();
            Ok(2)
        }
    }
}

fn plugin_pane_open(args: &[String]) -> std::io::Result<i32> {
    let mut plugin_id = None;
    let mut entrypoint = None;
    let mut placement = None;
    let mut width = None;
    let mut height = None;
    let mut workspace_id = None;
    let mut target_pane_id = None;
    let mut direction = None;
    let mut cwd = None;
    let mut focus = true;
    let mut env = HashMap::new();

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--plugin" => {
                let Some(value) = required_value(args, &mut index, "--plugin") else {
                    return Ok(2);
                };
                plugin_id = Some(value);
            }
            "--entrypoint" => {
                let Some(value) = required_value(args, &mut index, "--entrypoint") else {
                    return Ok(2);
                };
                entrypoint = Some(value);
            }
            "--placement" => {
                let Some(value) = required_value(args, &mut index, "--placement") else {
                    return Ok(2);
                };
                let Some(parsed) = parse_pane_placement(&value) else {
                    return Ok(2);
                };
                placement = Some(parsed);
            }
            "--width" => {
                let Some(value) = required_value(args, &mut index, "--width") else {
                    return Ok(2);
                };
                let Some(parsed) = parse_popup_dimension(&value, "--width") else {
                    return Ok(2);
                };
                width = Some(parsed);
            }
            "--height" => {
                let Some(value) = required_value(args, &mut index, "--height") else {
                    return Ok(2);
                };
                let Some(parsed) = parse_popup_dimension(&value, "--height") else {
                    return Ok(2);
                };
                height = Some(parsed);
            }
            "--workspace" => {
                let Some(value) = required_value(args, &mut index, "--workspace") else {
                    return Ok(2);
                };
                workspace_id = Some(value);
            }
            "--target-pane" => {
                let Some(value) = required_value(args, &mut index, "--target-pane") else {
                    return Ok(2);
                };
                target_pane_id = Some(value);
            }
            "--direction" => {
                let Some(value) = required_value(args, &mut index, "--direction") else {
                    return Ok(2);
                };
                let Some(parsed) = parse_split_direction(&value) else {
                    return Ok(2);
                };
                direction = Some(parsed);
            }
            "--cwd" => {
                let Some(value) = required_value(args, &mut index, "--cwd") else {
                    return Ok(2);
                };
                cwd = Some(value);
            }
            "--env" => {
                let Some(value) = required_value(args, &mut index, "--env") else {
                    return Ok(2);
                };
                let (key, value) = match super::parse_env_assignment(&value) {
                    Ok(pair) => pair,
                    Err(err) => {
                        eprintln!("{err}");
                        return Ok(2);
                    }
                };
                env.insert(key, value);
            }
            "--focus" => {
                focus = true;
                index += 1;
            }
            "--no-focus" => {
                focus = false;
                index += 1;
            }
            other => {
                eprintln!("unknown option: {other}");
                return Ok(2);
            }
        }
    }

    let Some(plugin_id) = plugin_id else {
        eprintln!("missing required --plugin");
        return Ok(2);
    };
    let Some(entrypoint) = entrypoint else {
        eprintln!("missing required --entrypoint");
        return Ok(2);
    };

    print_plugin_response(Method::PluginPaneOpen(PluginPaneOpenParams {
        plugin_id,
        entrypoint,
        placement,
        width,
        height,
        workspace_id,
        target_pane_id,
        direction,
        cwd,
        focus,
        env,
    }))
}

fn parse_popup_dimension(value: &str, flag: &str) -> Option<PopupSize> {
    match PopupSize::parse_cli(value) {
        Ok(value) => Some(value),
        Err(message) => {
            eprintln!("{flag} {message}");
            None
        }
    }
}

fn plugin_pane_focus(args: &[String]) -> std::io::Result<i32> {
    let Some(pane_id) = args.first() else {
        eprintln!("usage: herdr plugin pane focus <pane_id>");
        return Ok(2);
    };
    if args.len() != 1 {
        eprintln!("usage: herdr plugin pane focus <pane_id>");
        return Ok(2);
    }
    print_plugin_response(Method::PluginPaneFocus(PluginPaneFocusParams {
        pane_id: super::normalize_pane_id(pane_id),
    }))
}

fn plugin_pane_close(args: &[String]) -> std::io::Result<i32> {
    let Some(pane_id) = args.first() else {
        eprintln!("usage: herdr plugin pane close <pane_id>");
        return Ok(2);
    };
    if args.len() != 1 {
        eprintln!("usage: herdr plugin pane close <pane_id>");
        return Ok(2);
    }
    print_plugin_response(Method::PluginPaneClose(PluginPaneCloseParams {
        pane_id: super::normalize_pane_id(pane_id),
    }))
}

fn required_value(args: &[String], index: &mut usize, flag: &str) -> Option<String> {
    let Some(value) = args.get(*index + 1) else {
        eprintln!("missing value for {flag}");
        return None;
    };
    *index += 2;
    Some(value.clone())
}

fn parse_pane_placement(value: &str) -> Option<PluginPanePlacement> {
    match value {
        "overlay" => Some(PluginPanePlacement::Overlay),
        "popup" => Some(PluginPanePlacement::Popup),
        "split" => Some(PluginPanePlacement::Split),
        "tab" => Some(PluginPanePlacement::Tab),
        "zoomed" | "fullscreen" => Some(PluginPanePlacement::Zoomed),
        _ => {
            eprintln!("invalid pane placement: {value}");
            None
        }
    }
}

fn parse_split_direction(value: &str) -> Option<SplitDirection> {
    match value {
        "right" => Some(SplitDirection::Right),
        "down" => Some(SplitDirection::Down),
        _ => {
            eprintln!("invalid split direction: {value}");
            None
        }
    }
}

fn normalize_plugin_path_arg(value: &str) -> std::io::Result<String> {
    if super::target::is_remote() {
        if super::target::remote_path_is_absolute(value) {
            return Ok(value.to_owned());
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "remote plugin paths must be absolute",
        ));
    }
    let path = crate::worktree::expand_tilde_path(value);
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(absolute.display().to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GithubPluginSource {
    owner: String,
    repo: String,
    subdir: Option<String>,
}

impl GithubPluginSource {
    fn parse(value: &str) -> Result<Self, String> {
        if value.starts_with("http://")
            || value.starts_with("https://")
            || value.starts_with("git@")
            || value.contains(':')
        {
            return Err("plugin install v1 accepts only owner/repo[/subdir] shorthand".into());
        }
        let parts = value.split('/').collect::<Vec<_>>();
        if parts.len() < 2 {
            return Err(PLUGIN_INSTALL_USAGE.into());
        }
        let owner = parts[0];
        let repo = parts[1];
        validate_github_segment("owner", owner)?;
        validate_github_segment("repo", repo)?;
        let subdir_parts = &parts[2..];
        for part in subdir_parts {
            validate_subdir_segment(part)?;
        }
        let subdir = if subdir_parts.is_empty() {
            None
        } else {
            Some(subdir_parts.join("/"))
        };
        Ok(Self {
            owner: owner.to_string(),
            repo: repo.to_string(),
            subdir,
        })
    }

    fn remote_url(&self) -> String {
        format!("https://github.com/{}/{}.git", self.owner, self.repo)
    }

    fn display(&self) -> String {
        match &self.subdir {
            Some(subdir) => format!("{}/{}/{}", self.owner, self.repo, subdir),
            None => format!("{}/{}", self.owner, self.repo),
        }
    }

    fn manifest_root(&self, checkout: &Path) -> PathBuf {
        match &self.subdir {
            Some(subdir) => checkout.join(subdir),
            None => checkout.to_path_buf(),
        }
    }

    fn to_source_info(
        &self,
        requested_ref: Option<String>,
        resolved_commit: String,
        managed_path: Option<String>,
        installed_unix_ms: u64,
    ) -> PluginSourceInfo {
        PluginSourceInfo {
            kind: PluginSourceKind::Github,
            owner: Some(self.owner.clone()),
            repo: Some(self.repo.clone()),
            subdir: self.subdir.clone(),
            requested_ref,
            resolved_commit: Some(resolved_commit),
            managed_path,
            installed_unix_ms: Some(installed_unix_ms),
        }
    }
}

fn ensure_replacement_allowed(
    plugin: &InstalledPluginInfo,
    existing: Option<&InstalledPluginInfo>,
) -> std::io::Result<()> {
    let Some(existing) = existing else {
        return Ok(());
    };
    if existing.source.kind != PluginSourceKind::Github {
        return Err(std::io::Error::other(format!(
            "plugin {} is already linked from a local path; uninstall/unlink it before installing from GitHub",
            plugin.plugin_id
        )));
    }
    Ok(())
}

fn validate_github_segment(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("GitHub {label} must not be empty"));
    }
    if value == "." || value == ".." {
        return Err(format!("GitHub {label} is invalid: {value}"));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(format!(
            "GitHub {label} contains invalid characters: {value}"
        ));
    }
    Ok(())
}

fn validate_subdir_segment(value: &str) -> Result<(), String> {
    if value.is_empty() || value == "." || value == ".." {
        return Err(format!("invalid plugin subdir segment: {value}"));
    }
    if value.contains('\\') || value.contains('\0') {
        return Err(format!("invalid plugin subdir segment: {value}"));
    }
    Ok(())
}

fn git_checkout(
    source: &GithubPluginSource,
    requested_ref: Option<&str>,
    checkout: &Path,
) -> std::io::Result<()> {
    std::fs::create_dir_all(checkout)?;
    run_git(Some(checkout), ["init"])?;
    run_git(
        Some(checkout),
        ["remote", "add", "origin", &source.remote_url()],
    )?;
    match requested_ref {
        Some(reference) => {
            run_git(
                Some(checkout),
                ["fetch", "--depth", "1", "origin", reference],
            )?;
        }
        None => {
            run_git(Some(checkout), ["fetch", "--depth", "1", "origin", "HEAD"])?;
        }
    }
    run_git(Some(checkout), ["checkout", "--detach", "FETCH_HEAD"])?;
    Ok(())
}

fn run_git<const N: usize>(cwd: Option<&Path>, args: [&str; N]) -> std::io::Result<()> {
    let mut command = crate::noninteractive_process::command("git");
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    command.stdin(Stdio::null());
    let output = command.output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(std::io::Error::other(command_failure_message(
        "git", &output,
    )))
}

fn git_output<const N: usize>(cwd: &Path, args: [&str; N]) -> std::io::Result<String> {
    let output = crate::noninteractive_process::command("git")
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    Err(std::io::Error::other(command_failure_message(
        "git", &output,
    )))
}

fn command_failure_message(program: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        format!("{program} failed with status {}", output.status)
    } else {
        format!("{program} failed with status {}: {stderr}", output.status)
    }
}

fn load_cli_plugin_manifest(path: &Path, enabled: bool) -> std::io::Result<InstalledPluginInfo> {
    crate::app::load_plugin_manifest(&path.display().to_string(), enabled)
        .map_err(|(_, message)| std::io::Error::other(message))
}

fn persist_plugin_offline(plugin: &InstalledPluginInfo) -> std::io::Result<()> {
    crate::plugin_paths::ensure_plugin_user_dirs(&plugin.plugin_id)?;
    crate::persist::plugin_registry::update(|plugins| {
        plugins.retain(|entry| entry.plugin_id != plugin.plugin_id);
        plugins.push(plugin.clone());
    })?;
    Ok(())
}

fn register_installed_plugin(
    plugin: InstalledPluginInfo,
    source: PluginSourceInfo,
) -> Result<(), InstallFailure> {
    let request = Request {
        id: "cli:plugin".into(),
        method: Method::PluginLink(PluginLinkParams {
            path: plugin.manifest_path.clone(),
            enabled: plugin.enabled,
            source: Some(source.clone()),
        }),
    };
    match super::send_request(&request) {
        Ok(response) => {
            if response.get("error").is_some() {
                return Err(InstallFailure::Rollback(std::io::Error::other(
                    serde_json::to_string(&response).unwrap(),
                )));
            }
            if let Err(err) =
                verify_plugin_link_source_response(response, &plugin.plugin_id, &source)
            {
                let unlink = super::send_request(&Request {
                    id: "cli:plugin".into(),
                    method: Method::PluginUnlink(PluginUnlinkParams {
                        plugin_id: plugin.plugin_id.clone(),
                    }),
                });
                match unlink {
                    Ok(response) if response.get("error").is_none() => {
                        return Err(InstallFailure::Rollback(err));
                    }
                    Ok(response) => {
                        return Err(InstallFailure::KeepCheckout(std::io::Error::other(
                            format!(
                                "{err}; failed to undo incompatible plugin registration: {}",
                                serde_json::to_string(&response).unwrap()
                            ),
                        )));
                    }
                    Err(unlink_err) if super::protocol_mismatch_was_reported(&unlink_err) => {
                        return Err(InstallFailure::KeepCheckout(unlink_err));
                    }
                    Err(unlink_err) => {
                        return Err(InstallFailure::KeepCheckout(std::io::Error::other(
                            format!(
                                "{err}; failed to undo incompatible plugin registration: {unlink_err}"
                            ),
                        )));
                    }
                }
            }
            Ok(())
        }
        Err(err) if is_connection_error(&err) => {
            persist_plugin_offline(&plugin).map_err(InstallFailure::Rollback)
        }
        Err(err) => Err(InstallFailure::Rollback(err)),
    }
}

#[derive(Debug)]
enum InstallFailure {
    Rollback(std::io::Error),
    KeepCheckout(std::io::Error),
}

fn verify_plugin_link_source_response(
    response: serde_json::Value,
    plugin_id: &str,
    expected: &PluginSourceInfo,
) -> std::io::Result<()> {
    let parsed: SuccessResponse =
        serde_json::from_value(response).map_err(std::io::Error::other)?;
    let ResponseResult::PluginLinked { plugin } = parsed.result else {
        return Err(std::io::Error::other("expected plugin_linked response"));
    };
    if plugin.plugin_id != plugin_id
        || plugin.source.kind != PluginSourceKind::Github
        || plugin.source.owner != expected.owner
        || plugin.source.repo != expected.repo
        || plugin.source.subdir != expected.subdir
        || plugin.source.requested_ref != expected.requested_ref
        || plugin.source.resolved_commit != expected.resolved_commit
        || plugin.source.managed_path != expected.managed_path
    {
        return Err(std::io::Error::other(
            "running Herdr server did not persist GitHub plugin source metadata",
        ));
    }
    Ok(())
}

fn installed_plugin_info(plugin_id: &str) -> std::io::Result<Option<InstalledPluginInfo>> {
    match live_installed_plugin_info(plugin_id) {
        Ok(plugin) => Ok(plugin),
        Err(err) if is_connection_error(&err) => Ok(registry_plugin_info(plugin_id)),
        Err(err) => Err(err),
    }
}

fn live_installed_plugin_info(plugin_id: &str) -> std::io::Result<Option<InstalledPluginInfo>> {
    let response = super::send_request(&Request {
        id: "cli:plugin".into(),
        method: Method::PluginList(PluginListParams {
            plugin_id: Some(plugin_id.to_string()),
        }),
    })?;
    if response.get("error").is_some() {
        return Err(std::io::Error::other(
            serde_json::to_string(&response).unwrap(),
        ));
    }
    plugin_info_from_list_response(response)
}

fn installed_plugin_by_github_source(
    source: &GithubPluginSource,
) -> std::io::Result<Option<InstalledPluginInfo>> {
    let plugins = match live_installed_plugins() {
        Ok(plugins) => plugins,
        Err(err) if is_connection_error(&err) => registry_plugins(),
        Err(err) => return Err(err),
    };
    Ok(plugin_by_github_source(plugins, source))
}

fn live_installed_plugins() -> std::io::Result<Vec<InstalledPluginInfo>> {
    let response = super::send_request(&Request {
        id: "cli:plugin".into(),
        method: Method::PluginList(PluginListParams { plugin_id: None }),
    })?;
    if response.get("error").is_some() {
        return Err(std::io::Error::other(
            serde_json::to_string(&response).unwrap(),
        ));
    }
    plugin_list_from_response(response)
}

fn registry_plugin_info(plugin_id: &str) -> Option<InstalledPluginInfo> {
    registry_plugins()
        .into_iter()
        .find(|plugin| plugin.plugin_id == plugin_id)
}

fn registry_plugins() -> Vec<InstalledPluginInfo> {
    crate::persist::plugin_registry::load()
}

fn plugin_info_from_list_response(
    response: serde_json::Value,
) -> std::io::Result<Option<InstalledPluginInfo>> {
    let mut plugins = plugin_list_from_response(response)?;
    Ok(plugins.pop())
}

fn plugin_list_from_response(
    response: serde_json::Value,
) -> std::io::Result<Vec<InstalledPluginInfo>> {
    let parsed: SuccessResponse =
        serde_json::from_value(response).map_err(std::io::Error::other)?;
    let ResponseResult::PluginList { mut plugins } = parsed.result else {
        return Err(std::io::Error::other("expected plugin_list response"));
    };
    plugins.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
    Ok(plugins)
}

fn plugin_by_github_source(
    plugins: impl IntoIterator<Item = InstalledPluginInfo>,
    source: &GithubPluginSource,
) -> Option<InstalledPluginInfo> {
    plugins
        .into_iter()
        .find(|plugin| plugin_matches_github_source(plugin, source))
}

fn plugin_matches_github_source(plugin: &InstalledPluginInfo, source: &GithubPluginSource) -> bool {
    plugin.source.kind == PluginSourceKind::Github
        && plugin.source.owner.as_deref() == Some(source.owner.as_str())
        && plugin.source.repo.as_deref() == Some(source.repo.as_str())
        && plugin.source.subdir.as_deref() == source.subdir.as_deref()
}

fn offline_plugin_link_response(params: &PluginLinkParams) -> std::io::Result<serde_json::Value> {
    let plugin = load_cli_plugin_manifest(Path::new(&params.path), params.enabled)?;
    persist_plugin_offline(&plugin)?;
    serde_json::to_value(SuccessResponse {
        id: "cli:plugin".into(),
        result: ResponseResult::PluginLinked { plugin },
    })
    .map_err(std::io::Error::other)
}

fn offline_plugin_list_response(params: &PluginListParams) -> std::io::Result<serde_json::Value> {
    let entries = crate::persist::plugin_registry::load();
    let mut plugins =
        crate::persist::plugin_registry::reload_manifests(entries, |path, enabled| {
            crate::app::load_plugin_manifest(path, enabled).map_err(|(_, msg)| msg)
        })
        .into_iter()
        .filter(|plugin| {
            params
                .plugin_id
                .as_deref()
                .is_none_or(|plugin_id| plugin.plugin_id == plugin_id)
        })
        .collect::<Vec<_>>();
    plugins.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
    serde_json::to_value(SuccessResponse {
        id: "cli:plugin".into(),
        result: ResponseResult::PluginList { plugins },
    })
    .map_err(std::io::Error::other)
}

fn print_plugin_list_human(response: &serde_json::Value) -> std::io::Result<i32> {
    if response.get("error").is_some() {
        return super::print_response(response);
    }
    let parsed: SuccessResponse =
        serde_json::from_value(response.clone()).map_err(std::io::Error::other)?;
    let ResponseResult::PluginList { plugins } = parsed.result else {
        return super::print_response(response);
    };
    if plugins.is_empty() {
        println!("No plugins installed.");
        return Ok(0);
    }
    println!(
        "{} plugin{} installed:",
        plugins.len(),
        if plugins.len() == 1 { "" } else { "s" }
    );
    for plugin in plugins {
        let enabled = if plugin.enabled {
            "enabled"
        } else {
            "disabled"
        };
        let warning = if plugin.warnings.is_empty() {
            String::new()
        } else {
            format!("; {} warning(s)", plugin.warnings.len())
        };
        println!(
            "- {} ({}) {} [{}{}]",
            plugin.plugin_id,
            plugin.name,
            enabled,
            source_display(&plugin),
            warning
        );
        println!(
            "  config: {}",
            crate::plugin_paths::plugin_config_dir(&plugin.plugin_id).display()
        );
        for warning in plugin.warnings {
            println!("  warning: {warning}");
        }
    }
    Ok(0)
}

fn source_display(plugin: &InstalledPluginInfo) -> String {
    match plugin.source.kind {
        PluginSourceKind::Github => {
            let owner = plugin.source.owner.as_deref().unwrap_or("unknown");
            let repo = plugin.source.repo.as_deref().unwrap_or("unknown");
            let subdir = plugin
                .source
                .subdir
                .as_deref()
                .map(|subdir| format!("/{subdir}"))
                .unwrap_or_default();
            let reference = plugin
                .source
                .requested_ref
                .as_deref()
                .or(plugin.source.resolved_commit.as_deref())
                .unwrap_or("unknown");
            format!("github:{owner}/{repo}{subdir}@{reference}")
        }
        PluginSourceKind::Local => format!("local:{}", plugin.plugin_root),
    }
}

fn print_install_preview(
    plugin: &InstalledPluginInfo,
    source: &PluginSourceInfo,
    existing: Option<&InstalledPluginInfo>,
) {
    eprintln!("Plugin install preview:");
    eprintln!("  id: {}", plugin.plugin_id);
    eprintln!("  name: {}", plugin.name);
    eprintln!("  version: {}", plugin.version);
    if let (Some(owner), Some(repo)) = (&source.owner, &source.repo) {
        let subdir = source
            .subdir
            .as_deref()
            .map(|subdir| format!("/{subdir}"))
            .unwrap_or_default();
        eprintln!("  source: {owner}/{repo}{subdir}");
    }
    if let Some(reference) = &source.requested_ref {
        eprintln!("  ref: {reference}");
    }
    if let Some(commit) = &source.resolved_commit {
        eprintln!("  commit: {commit}");
    }
    eprintln!("  actions: {}", plugin.actions.len());
    eprintln!("  startup commands: {}", plugin.startup.len());
    eprintln!("  events: {}", plugin.events.len());
    eprintln!("  panes: {}", plugin.panes.len());
    eprintln!("  link handlers: {}", plugin.link_handlers.len());
    eprintln!("  build commands: {}", plugin.build.len());
    for build in &plugin.build {
        let support = if build_platform_supported(&build.platforms, &plugin.platforms) {
            String::new()
        } else {
            format!(
                " (skipped on {})",
                plugin_platform_name(current_plugin_platform())
            )
        };
        eprintln!("    build{}: {}", support, build.command.join(" "));
    }
    for startup in &plugin.startup {
        eprintln!("    startup: {}", startup.command.join(" "));
    }
    for action in &plugin.actions {
        eprintln!("    action {}: {}", action.id, action.command.join(" "));
    }
    for event in &plugin.events {
        eprintln!("    event {}: {}", event.on, event.command.join(" "));
    }
    for pane in &plugin.panes {
        eprintln!("    pane {}: {}", pane.id, pane.command.join(" "));
    }
    for warning in &plugin.warnings {
        eprintln!("  warning: {warning}");
    }
    if let Some(existing) = existing {
        eprintln!(
            "  replaces: {} from {}",
            existing.plugin_id,
            source_display(existing)
        );
    }
}

fn run_plugin_build_commands(
    plugin: &InstalledPluginInfo,
    manifest_root: &Path,
) -> Result<(), Box<PluginBuildFailure>> {
    let total = plugin.build.len();
    for (index, build) in plugin.build.iter().enumerate() {
        if !build_platform_supported(&build.platforms, &plugin.platforms) {
            continue;
        }
        run_plugin_build_command(
            &plugin.plugin_id,
            index + 1,
            total,
            manifest_root,
            &build.command,
        )?;
    }
    Ok(())
}

fn ensure_manifest_unchanged_after_build(
    before: &InstalledPluginInfo,
    after: &InstalledPluginInfo,
) -> io::Result<()> {
    if before == after {
        return Ok(());
    }
    Err(io::Error::other(
        "plugin build changed herdr-plugin.toml after install preview; aborting install",
    ))
}

fn run_plugin_build_command(
    plugin_id: &str,
    build_index: usize,
    build_total: usize,
    cwd: &Path,
    command: &[String],
) -> Result<(), Box<PluginBuildFailure>> {
    let context = plugin_build_context(plugin_id, build_index, build_total, cwd, command);
    let Some(program) = command.first() else {
        return Err(Box::new(PluginBuildFailure {
            context,
            kind: PluginBuildFailureKind::Start {
                error: io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "build command must not be empty",
                ),
            },
        }));
    };
    let args = command.iter().skip(1).cloned().collect::<Vec<_>>();
    let mut child = crate::plugin_command::command_for_argv_in_dir(program, &args, cwd);
    child
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    scrub_herdr_runtime_env(&mut child);

    let mut child = match child.spawn() {
        Ok(child) => child,
        Err(err) => {
            return Err(Box::new(PluginBuildFailure {
                context,
                kind: PluginBuildFailureKind::Start { error: err },
            }));
        }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_reader = stdout.map(|stdout| {
        std::thread::spawn(move || read_tail_capped_output(stdout, PLUGIN_BUILD_OUTPUT_MAX_BYTES))
    });
    let stderr_reader = stderr.map(|stderr| {
        std::thread::spawn(move || read_tail_capped_output(stderr, PLUGIN_BUILD_OUTPUT_MAX_BYTES))
    });
    let status = child.wait().map_err(|error| {
        Box::new(PluginBuildFailure {
            context: context.clone(),
            kind: PluginBuildFailureKind::Wait { error },
        })
    })?;
    let stdout = stdout_reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    let stderr = stderr_reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    if status.success() {
        return Ok(());
    }
    Err(Box::new(PluginBuildFailure {
        context,
        kind: PluginBuildFailureKind::Exit {
            status,
            stdout,
            stderr,
        },
    }))
}

fn plugin_build_context(
    plugin_id: &str,
    build_index: usize,
    build_total: usize,
    cwd: &Path,
    command: &[String],
) -> PluginBuildContext {
    PluginBuildContext {
        plugin_id: plugin_id.to_string(),
        build_index,
        build_total,
        cwd: cwd.display().to_string(),
        command: command.to_vec(),
    }
}

#[derive(Debug, Default)]
struct CappedOutput {
    text: String,
    truncated: bool,
}

#[derive(Debug, Clone)]
struct PluginBuildContext {
    plugin_id: String,
    build_index: usize,
    build_total: usize,
    cwd: String,
    command: Vec<String>,
}

#[derive(Debug)]
struct PluginBuildFailure {
    context: PluginBuildContext,
    kind: PluginBuildFailureKind,
}

#[derive(Debug)]
enum PluginBuildFailureKind {
    Start {
        error: io::Error,
    },
    Wait {
        error: io::Error,
    },
    Exit {
        status: ExitStatus,
        stdout: CappedOutput,
        stderr: CappedOutput,
    },
}

impl fmt::Display for PluginBuildFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "error: plugin build failed")?;
        write_build_context(f, &self.context)?;
        match &self.kind {
            PluginBuildFailureKind::Start { error } => {
                writeln!(f, "  error: failed to start: {error}")?;
            }
            PluginBuildFailureKind::Wait { error } => {
                writeln!(f, "  error: failed to wait for command: {error}")?;
            }
            PluginBuildFailureKind::Exit {
                status,
                stdout,
                stderr,
            } => {
                writeln!(f, "  status: {status}")?;
                write_output_section(f, "stderr", stderr)?;
                write_output_section(f, "stdout", stdout)?;
            }
        }
        writeln!(f)?;
        write!(f, "Plugin was not installed.")
    }
}

fn write_build_context(f: &mut fmt::Formatter<'_>, context: &PluginBuildContext) -> fmt::Result {
    writeln!(f, "  plugin: {}", context.plugin_id)?;
    writeln!(
        f,
        "  build: {}/{}",
        context.build_index, context.build_total
    )?;
    writeln!(f, "  cwd: {}", context.cwd)?;
    writeln!(f, "  command: {}", context.command.join(" "))
}

fn write_output_section(
    f: &mut fmt::Formatter<'_>,
    label: &str,
    output: &CappedOutput,
) -> fmt::Result {
    let text = output.text.trim_end();
    if text.is_empty() {
        return Ok(());
    }
    writeln!(f)?;
    if output.truncated {
        writeln!(
            f,
            "{label}: showing last {PLUGIN_BUILD_OUTPUT_MAX_BYTES} bytes; earlier output omitted"
        )?;
    } else {
        writeln!(f, "{label}:")?;
    }
    writeln!(f, "{text}")
}

fn read_tail_capped_output(mut reader: impl Read, cap: usize) -> CappedOutput {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    let mut truncated = false;
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                out.extend_from_slice(&buf[..n]);
                if out.len() > cap {
                    let excess = out.len() - cap;
                    out.drain(0..excess);
                    truncated = true;
                }
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    CappedOutput {
        text: String::from_utf8_lossy(&out).to_string(),
        truncated,
    }
}

fn scrub_herdr_runtime_env(command: &mut Command) {
    for key in [
        crate::api::SOCKET_PATH_ENV_VAR,
        crate::server::socket_paths::CLIENT_SOCKET_PATH_ENV_VAR,
        crate::session::SESSION_ENV_VAR,
        "HERDR_BIN_PATH",
        "HERDR_ENV",
        "HERDR_WORKSPACE_ID",
        "HERDR_TAB_ID",
        "HERDR_PANE_ID",
    ] {
        command.env_remove(key);
    }
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("HERDR_PLUGIN_") {
            command.env_remove(key);
        }
    }
}

fn build_platform_supported(
    platforms: &Option<Vec<PluginPlatform>>,
    plugin_platforms: &Option<Vec<PluginPlatform>>,
) -> bool {
    platforms
        .as_ref()
        .or(plugin_platforms.as_ref())
        .is_none_or(|platforms| platforms.contains(&current_plugin_platform()))
}

fn current_plugin_platform() -> PluginPlatform {
    if cfg!(target_os = "linux") {
        PluginPlatform::Linux
    } else if cfg!(target_os = "macos") {
        PluginPlatform::Macos
    } else {
        PluginPlatform::Windows
    }
}

fn plugin_platform_name(platform: PluginPlatform) -> &'static str {
    match platform {
        PluginPlatform::Linux => "linux",
        PluginPlatform::Macos => "macos",
        PluginPlatform::Windows => "windows",
    }
}

fn confirm(prompt: &str) -> std::io::Result<bool> {
    eprint!("{prompt} [y/N] ");
    io::stderr().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(matches!(line.trim(), "y" | "Y" | "yes" | "YES" | "Yes"))
}

fn create_plugin_temp_dir(label: &str) -> std::io::Result<PathBuf> {
    let path = crate::plugin_paths::managed_plugins_dir().join(format!(
        ".tmp-{label}-{}-{}",
        std::process::id(),
        current_unix_ms()
    ));
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

fn remove_managed_plugin_files(plugin: &InstalledPluginInfo) -> std::io::Result<()> {
    if plugin.source.kind != PluginSourceKind::Github {
        return Ok(());
    }
    let Some(path) = plugin.source.managed_path.as_deref() else {
        return Ok(());
    };
    let path = PathBuf::from(path);
    if !path.exists() {
        return Ok(());
    }
    if !is_expected_managed_path(plugin, &path) {
        return Err(std::io::Error::other(format!(
            "refusing to delete unmanaged plugin path: {}",
            path.display()
        )));
    }
    std::fs::remove_dir_all(&path)
        .map_err(|err| plugin_checkout_lifecycle_error("remove", &path, err))
}

fn plugin_checkout_lifecycle_error(operation: &str, path: &Path, err: io::Error) -> io::Error {
    if cfg!(windows) && err.kind() == io::ErrorKind::PermissionDenied {
        return io::Error::new(
            err.kind(),
            format!(
                "failed to {operation} managed plugin checkout at {}; close any Herdr plugin panes or plugin commands using that checkout, then retry: {err}",
                path.display()
            ),
        );
    }
    err
}

fn is_expected_managed_path(plugin: &InstalledPluginInfo, path: &Path) -> bool {
    let Ok(path) = path.canonicalize() else {
        return false;
    };
    let expected = crate::plugin_paths::managed_checkout_path(&plugin.plugin_id);
    let Ok(expected) = expected.canonicalize() else {
        return false;
    };
    path == expected
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn is_connection_error(err: &std::io::Error) -> bool {
    if super::target::is_remote() {
        return false;
    }
    // A `server_not_running` marker is a connect failure for recovery purposes:
    // treating it as a connection error lets plugin commands fall back to the
    // offline registry. The marker carries (but does not print) a friendly
    // response, so recovering here prints nothing.
    super::server_not_running_was_reported(err)
        || matches!(
            err.kind(),
            std::io::ErrorKind::NotFound
                | std::io::ErrorKind::ConnectionRefused
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::BrokenPipe
        )
}

fn print_plugin_response(method: Method) -> std::io::Result<i32> {
    super::print_response(&super::send_request(&Request {
        id: "cli:plugin".into(),
        method,
    })?)
}

fn print_plugin_help() {
    eprintln!("herdr plugin commands:");
    eprintln!("  herdr plugin install <owner>/<repo>[/subdir...] [--ref REF] [--yes]");
    eprintln!("  herdr plugin uninstall <plugin_id|owner/repo[/subdir...]>");
    eprintln!("  herdr plugin link <path> [--disabled]");
    eprintln!("  herdr plugin list [--plugin ID] [--json]");
    eprintln!("  herdr plugin config-dir <plugin_id>");
    eprintln!("  herdr plugin unlink <plugin_id>");
    eprintln!("  herdr plugin enable <plugin_id>");
    eprintln!("  herdr plugin disable <plugin_id>");
    eprintln!("  {PLUGIN_RELOAD_USAGE}");
    eprintln!("  herdr plugin action <list|invoke>");
    eprintln!("  herdr plugin log list [--plugin ID] [--limit N]");
    eprintln!("  herdr plugin pane <open|focus|close>");
}

fn print_plugin_action_help() {
    eprintln!("herdr plugin action commands:");
    eprintln!("  herdr plugin action list [--plugin ID]");
    eprintln!("  herdr plugin action invoke <action_id> [--plugin ID]");
}

fn print_plugin_pane_help() {
    eprintln!("herdr plugin pane commands:");
    eprintln!(
        "  herdr plugin pane open --plugin ID --entrypoint ID [--placement overlay|popup|split|tab|zoomed] [--width SIZE] [--height SIZE] [--workspace ID] [--target-pane PANE] [--direction right|down] [--cwd PATH] [--env KEY=VALUE] [--focus|--no-focus]"
    );
    eprintln!("  herdr plugin pane focus <pane_id>");
    eprintln!("  herdr plugin pane close <pane_id>");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_reload_prompt_rejects_undelivered_outcomes() {
        let now = std::time::Instant::now();
        for state in ["dropped", "stale_session"] {
            let failure = wait_for_plugin_reload_prompt(
                "test-pane",
                serde_json::json!({"state": state, "id": "send-1"}),
                now,
            )
            .expect_err("undelivered");
            assert_eq!(failure.exit_code, 1);
        }
        for reason in ["expired", "cancelled"] {
            let failure = wait_for_plugin_reload_prompt(
                "test-pane",
                serde_json::json!({"state": "dropped", "reason": reason, "id": "send-1"}),
                now,
            )
            .expect_err("undelivered");
            assert_eq!(failure.exit_code, PLUGIN_RELOAD_EXIT_BUSY);
        }
        assert!(wait_for_plugin_reload_prompt(
            "test-pane",
            serde_json::json!({"state": "delivered"}),
            now
        )
        .is_ok());
    }

    /// What the scripted server reports after `agent.resume` succeeded.
    #[derive(Clone, Copy)]
    enum AfterResume {
        /// The resumed claude is idle on its session.
        Ready,
        /// The pane reports a restore error while agent.get still says idle.
        RestoreErrorWhileIdle,
        /// Claude was seen starting, then the pane's agent is gone (the
        /// foreground is the pane shell again).
        AgentGoneAfterStart,
        /// Idle, but the server holds the session-bound line until it
        /// expires (agent busy or human input until --timeout).
        PromptExpires,
        /// The server never expires the held line; only a cancel removes it.
        StuckQueued,
        /// The line left the queue for a backpressured PTY writer and has
        /// not finished by the grace.
        HandedToWriter,
        /// Handed to the writer, then its receipt is evicted from the
        /// server's history before the writer confirms it.
        HandedThenEvicted,
        /// Handed to the writer, then pane.queue succeeds but no longer
        /// lists the receipt.
        HandedThenEmpty,
        /// Queued at first; the first poll sees it handed to the writer and
        /// the next finds its receipt evicted.
        HandedDuringPoll,
        /// An older server: agent.get has no runtime_id.
        NoRuntimeId,
        /// The server accepts agent.prompt but echoes no delivery ack.
        NoAck,
    }

    /// Run the real `plugin reload` flow against a scripted server and return
    /// the outcome, the API calls made, and whether a record was written.
    type ScriptedReload = (
        Result<serde_json::Value, PluginReloadFailure>,
        Vec<&'static str>,
        bool,
    );

    fn run_scripted_plugin_reload(scenario: AfterResume) -> ScriptedReload {
        run_scripted_plugin_reload_with(scenario, false)
    }

    fn run_scripted_plugin_reload_with(scenario: AfterResume, no_resume: bool) -> ScriptedReload {
        let dir = std::env::temp_dir().join(format!(
            "herdr-plugin-reload-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
        ));
        let record_path = dir.join("record.json");
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let log = calls.clone();
        let mut resumed = false;
        let mut post_resume_agent_gets = 0;
        let mut queue_polls = 0;
        PLUGIN_RELOAD_TEST_CALL.with(|call| {
            *call.borrow_mut() = Some(Box::new(move |method: &Method| {
                let name = match method {
                    Method::PluginReload(_) => "plugin.reload",
                    Method::PluginActionList(_) => "plugin.action.list",
                    Method::AgentGet(_) => "agent.get",
                    Method::AgentResume(_) => "agent.resume",
                    Method::PaneGet(_) => "pane.get",
                    Method::AgentPrompt(params) => {
                        // Delivery is always bound to the resumed or current
                        // session, on the runtime seen ready.
                        let delivery = params.delivery.as_ref().expect("session-bound prompt");
                        assert_eq!(delivery.session_id, "sess-1");
                        assert_eq!(delivery.runtime_id.as_deref(), Some("rt-a"));
                        assert_eq!(delivery.input_quiet_ms, PLUGIN_RELOAD_INPUT_QUIET_MS);
                        "agent.prompt"
                    }
                    Method::PaneQueue(params) if params.cancel => "pane.queue.cancel",
                    Method::PaneQueue(_) => "pane.queue",
                    _ => "other",
                };
                log.borrow_mut().push(name);
                let claude = |status: &str| {
                    let mut agent = serde_json::json!({"type": "agent_info", "agent": {
                        "agent": "claude",
                        "agent_status": status,
                        "agent_session": {"source": "claude", "agent": "claude",
                            "kind": "id", "value": "sess-1"},
                        "runtime_id": "rt-a",
                    }});
                    if matches!(scenario, AfterResume::NoRuntimeId) {
                        if let Some(info) = agent["agent"].as_object_mut() {
                            info.remove("runtime_id");
                        }
                    }
                    agent
                };
                let ack = serde_json::json!({"session_id": "sess-1", "runtime_id": "rt-a"});
                let send = |state: &str, reason: Option<&str>| {
                    serde_json::json!({"sends": [], "recent": [
                        {"id": "q1", "state": state, "reason": reason}
                    ]})
                };
                match name {
                    "plugin.reload" => Ok(serde_json::json!({"type": "ok"})),
                    "plugin.action.list" => Ok(serde_json::json!({"actions": []})),
                    "agent.get" if !resumed && !no_resume => Ok(claude("idle")),
                    "agent.resume" => {
                        resumed = true;
                        Ok(
                            serde_json::json!({"type": "agent_resumed", "pane_id": "w1:p1",
                            "agent": "claude", "session_id": "sess-1", "argv": []}),
                        )
                    }
                    "pane.get" => Ok(match scenario {
                        AfterResume::RestoreErrorWhileIdle => serde_json::json!({"pane": {
                            "pane_id": "w1:p1",
                            "restore_error": "Resumed agent exited before startup completed",
                        }}),
                        _ => serde_json::json!({"pane": {"pane_id": "w1:p1"}}),
                    }),
                    "agent.get" => {
                        post_resume_agent_gets += 1;
                        Ok(match scenario {
                            AfterResume::AgentGoneAfterStart if post_resume_agent_gets == 1 => {
                                claude("working")
                            }
                            AfterResume::AgentGoneAfterStart => {
                                serde_json::json!({"type": "agent_info", "agent": {
                                    "agent_status": "unknown",
                                    "agent_session": {"source": "claude", "agent": "claude",
                                        "kind": "id", "value": "sess-1"},
                                }})
                            }
                            _ => claude("idle"),
                        })
                    }
                    "agent.prompt" => Ok(match scenario {
                        AfterResume::NoAck => serde_json::json!({"id": "q1", "state": "delivered"}),
                        AfterResume::PromptExpires
                        | AfterResume::StuckQueued
                        | AfterResume::HandedDuringPoll => {
                            serde_json::json!({"id": "q1", "state": "queued", "queued": true,
                                "delivery": ack})
                        }
                        AfterResume::HandedToWriter
                        | AfterResume::HandedThenEvicted
                        | AfterResume::HandedThenEmpty => {
                            serde_json::json!({"id": "q1", "state": "queued",
                                "reason": "handed_to_writer", "delivery": ack})
                        }
                        _ => serde_json::json!({"id": "q1", "state": "delivered", "delivery": ack}),
                    }),
                    "pane.queue" if matches!(scenario, AfterResume::HandedThenEvicted) => {
                        Err(("queue_item_not_found".into(), "queue item not found".into()))
                    }
                    "pane.queue" if matches!(scenario, AfterResume::HandedDuringPoll) => {
                        queue_polls += 1;
                        if queue_polls == 1 {
                            Ok(send("queued", Some("handed_to_writer")))
                        } else {
                            Err(("queue_item_not_found".into(), "queue item not found".into()))
                        }
                    }
                    "pane.queue" => Ok(match scenario {
                        AfterResume::HandedThenEmpty => {
                            serde_json::json!({"sends": [], "recent": []})
                        }
                        AfterResume::StuckQueued => send("queued", None),
                        AfterResume::HandedToWriter => send("queued", Some("handed_to_writer")),
                        _ => send("dropped", Some("expired")),
                    }),
                    "pane.queue.cancel" => Ok(match scenario {
                        AfterResume::HandedToWriter => send("queued", Some("handed_to_writer")),
                        _ => send("dropped", Some("cancelled")),
                    }),
                    _ => Err(("unexpected".into(), name.into())),
                }
            }));
        });
        let args = PluginReloadArgs {
            plugin_id: "example.reload".into(),
            pane: "w1:p1".into(),
            request: "ar-1".into(),
            changelog: "run `agent-request confirm ar-1`".into(),
            // The held-line scenarios run to --timeout and its grace.
            timeout_ms: match scenario {
                AfterResume::StuckQueued | AfterResume::HandedToWriter => 0,
                _ => 30_000,
            },
            no_resume,
        };
        let result = run_plugin_reload(&args, &record_path);
        PLUGIN_RELOAD_TEST_CALL.with(|call| *call.borrow_mut() = None);
        let recorded = record_path.exists();
        let _ = std::fs::remove_dir_all(&dir);
        let calls = calls.borrow().clone();
        (result, calls, recorded)
    }

    #[test]
    fn plugin_reload_post_resume_prompts_only_the_resumed_idle_agent() {
        let (result, calls, recorded) = run_scripted_plugin_reload(AfterResume::Ready);
        let record = result.ok().expect("resumed idle agent is prompted");
        assert_eq!(record["delivery"], "delivered");
        assert!(calls.contains(&"agent.prompt"));
        assert!(recorded);

        for scenario in [
            AfterResume::RestoreErrorWhileIdle,
            AfterResume::AgentGoneAfterStart,
        ] {
            let (result, calls, recorded) = run_scripted_plugin_reload(scenario);
            let failure = result.expect_err("post-resume failure must fail the reload");
            assert_eq!(failure.exit_code, 1);
            assert!(
                failure.message.starts_with("resumed agent failed"),
                "{}",
                failure.message
            );
            assert!(
                !calls.contains(&"agent.prompt"),
                "prompt typed after failure: {calls:?}"
            );
            assert!(!recorded, "completed record written after failure");
        }

        // An expired line is retryable and leaves no record behind.
        for no_resume in [false, true] {
            let (result, calls, recorded) =
                run_scripted_plugin_reload_with(AfterResume::PromptExpires, no_resume);
            let failure = result.expect_err("an expired line must not complete");
            assert_eq!(
                failure.exit_code, PLUGIN_RELOAD_EXIT_BUSY,
                "{}",
                failure.message
            );
            assert!(calls.contains(&"pane.queue"), "{calls:?}");
            assert!(!recorded);
        }
    }

    /// A server that cannot report the runtime it would bind to never gets
    /// the prompt; one that takes it without acknowledging the binding fails
    /// the reload instead of recording it.
    #[test]
    fn plugin_reload_fails_closed_without_runtime_identity_or_delivery_ack() {
        let (result, calls, recorded) = run_scripted_plugin_reload(AfterResume::NoRuntimeId);
        let failure = result.expect_err("no runtime identity must fail");
        assert_eq!(failure.exit_code, 1);
        assert!(
            failure.message.contains("runtime_id"),
            "{}",
            failure.message
        );
        assert!(!calls.contains(&"agent.resume"), "{calls:?}");
        assert!(!calls.contains(&"agent.prompt"), "{calls:?}");
        assert!(!recorded);

        let (result, calls, recorded) = run_scripted_plugin_reload(AfterResume::NoAck);
        let failure = result.expect_err("no delivery ack must fail");
        assert_eq!(failure.exit_code, 1);
        assert!(
            failure.message.contains("did not acknowledge"),
            "{}",
            failure.message
        );
        assert!(calls.contains(&"agent.prompt"), "{calls:?}");
        assert!(!recorded);
    }

    /// Past --timeout and its grace, a line still held is cancelled and the
    /// reload is retryable (75); a line already handed to the writer may
    /// still land, so it is recorded and never cancelled or retried.
    #[test]
    fn plugin_reload_cancels_a_line_still_held_past_its_grace() {
        let (result, calls, recorded) = run_scripted_plugin_reload(AfterResume::StuckQueued);
        let failure = result.expect_err("a held line must not complete");
        assert_eq!(
            failure.exit_code, PLUGIN_RELOAD_EXIT_BUSY,
            "{}",
            failure.message
        );
        assert_eq!(calls.last(), Some(&"pane.queue.cancel"), "{calls:?}");
        assert!(!recorded);

        let (result, calls, recorded) = run_scripted_plugin_reload(AfterResume::HandedToWriter);
        let record = result.ok().expect("a handed line is not a failure");
        assert_eq!(record["delivery"], "handed_to_writer");
        assert!(!calls.contains(&"pane.queue.cancel"), "{calls:?}");
        assert!(recorded);
    }

    /// A line seen handed to the writer may still land, so when the server
    /// later loses its receipt (history eviction) the reload is recorded as
    /// handed instead of failing into a retry that could type it twice.
    #[test]
    fn plugin_reload_records_a_handed_line_whose_receipt_was_evicted() {
        for no_resume in [false, true] {
            let (result, calls, recorded) =
                run_scripted_plugin_reload_with(AfterResume::HandedThenEvicted, no_resume);
            let record = result
                .ok()
                .expect("an evicted handed line is not a failure");
            assert_eq!(record["delivery"], "handed_to_writer");
            if no_resume {
                assert_eq!(record["prompted"], true);
            } else {
                assert_eq!(record["resumed"], true);
            }
            assert!(calls.contains(&"pane.queue"), "{calls:?}");
            assert!(!calls.contains(&"pane.queue.cancel"), "{calls:?}");
            assert!(recorded);
        }
        // The receipt is simply absent from a successful response, or the
        // handoff is first seen while polling: still handed, never retried.
        for scenario in [AfterResume::HandedThenEmpty, AfterResume::HandedDuringPoll] {
            let (result, calls, recorded) = run_scripted_plugin_reload(scenario);
            let record = result
                .ok()
                .expect("a handed line whose receipt is gone is not a failure");
            assert_eq!(record["delivery"], "handed_to_writer");
            assert!(!calls.contains(&"pane.queue.cancel"), "{calls:?}");
            assert!(recorded);
        }
    }

    /// `--no-resume` prompts the running agent on its current session and
    /// never restarts it.
    #[test]
    fn plugin_reload_no_resume_prompts_the_running_agent_without_agent_resume() {
        let (result, calls, recorded) = run_scripted_plugin_reload_with(AfterResume::Ready, true);
        let record = result.ok().expect("idle agent is prompted");
        assert!(!calls.contains(&"agent.resume"), "{calls:?}");
        assert!(calls.contains(&"agent.prompt"), "{calls:?}");
        assert_eq!(record["resumed"], false);
        assert_eq!(record["prompted"], true);
        assert_eq!(record["session_id"], "sess-1");
        assert!(plugin_reload_delivered(&record));
        assert!(recorded);
        // A prompted record short-circuits a retry; a no-agent record does not.
        let dir = std::env::temp_dir().join(format!(
            "herdr-plugin-reload-prompted-{}",
            std::process::id()
        ));
        let record_path = dir.join("record.json");
        write_plugin_reload_record(&record_path, &record).unwrap();
        assert_eq!(completed_plugin_reload_record(&record_path), Some(record));
        let mut no_agent = serde_json::json!({"resumed": false, "session_id": null});
        write_plugin_reload_record(&record_path, &no_agent).unwrap();
        assert_eq!(completed_plugin_reload_record(&record_path), None);
        no_agent["prompted"] = serde_json::Value::Bool(false);
        assert!(!plugin_reload_delivered(&no_agent));
        let _ = std::fs::remove_dir_all(&dir);

        let (result, calls, recorded) =
            run_scripted_plugin_reload_with(AfterResume::RestoreErrorWhileIdle, true);
        assert_eq!(result.err().map(|failure| failure.exit_code), Some(1));
        assert!(!calls.contains(&"agent.resume") && !calls.contains(&"agent.prompt"));
        assert!(!recorded);
    }

    #[test]
    fn machine_plugin_connection_errors_never_use_local_offline_state() {
        crate::cli::target::with_test_client(crate::api::client::ApiClient::local(), || {
            assert!(!is_connection_error(&std::io::Error::from(
                std::io::ErrorKind::ConnectionRefused
            )));
        });
    }

    /// Parser edge cases protect the public reload command contract.
    #[test]
    fn plugin_reload_args_match_the_command_catalog_and_validate_request_ids() {
        let args: Vec<String> = [
            "example.reload",
            "--pane",
            "w1:p1",
            "--request",
            "ar-agent_1.2",
            "--changelog",
            "updated handler",
            "--json",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let parsed = parse_plugin_reload_args(&args).unwrap();
        assert_eq!(parsed.plugin_id, "example.reload");
        assert_eq!(parsed.pane, "w1:p1");
        assert_eq!(parsed.request, "ar-agent_1.2");
        assert_eq!(parsed.changelog, "updated handler");
        assert_eq!(parsed.timeout_ms, 600_000);
        assert!(!parsed.no_resume);
        let mut catalog_args = vec![
            "herdr".to_string(),
            "plugin".to_string(),
            "reload".to_string(),
        ];
        catalog_args.extend(args.clone());
        super::super::spec::command()
            .try_get_matches_from(catalog_args)
            .unwrap();
        let mut no_resume = args.clone();
        no_resume.push("--no-resume".into());
        assert!(parse_plugin_reload_args(&no_resume).unwrap().no_resume);
        let mut catalog_no_resume = vec!["herdr".to_string(), "plugin".into(), "reload".into()];
        catalog_no_resume.extend(no_resume);
        super::super::spec::command()
            .try_get_matches_from(catalog_no_resume)
            .unwrap();
        let mut timed = args.clone();
        timed.extend(["--timeout".into(), "1500".into()]);
        assert_eq!(parse_plugin_reload_args(&timed).unwrap().timeout_ms, 1500);
        for request in [
            String::new(),
            "bad/id".into(),
            "bad id".into(),
            "é".into(),
            "a".repeat(129),
        ] {
            let mut invalid = args.clone();
            invalid[4] = request;
            assert_eq!(parse_plugin_reload_args(&invalid).unwrap_err().0, 1);
        }
        for request in ["a".repeat(128), ".".into(), "A-Z_0.9".into()] {
            let mut valid = args.clone();
            valid[4] = request;
            assert!(parse_plugin_reload_args(&valid).is_ok());
        }
    }

    /// A real record must short-circuit before contacting even an available socket.
    #[cfg(unix)]
    #[test]
    fn plugin_reload_completed_record_never_calls_the_socket() {
        let plugin_id = unique_plugin_id("reload-record");
        let record_path = plugin_reload_record_path(&plugin_id, "request-1");
        let record = serde_json::json!({
            "plugin_id": plugin_id, "reloaded_at": "2026-10-06T12:00:00Z",
            "pane": "w1:p1", "session_id": "session-1", "resumed": true,
            "request": "request-1",
        });
        write_plugin_reload_record(&record_path, &record).unwrap();
        // A listening test socket detects an accidental API call without ever
        // connecting to the user's server.
        let socket_path = std::env::temp_dir().join(format!("hr-r-{}.sock", std::process::id()));
        let listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let args: Vec<String> = [
            plugin_id.as_str(),
            "--pane",
            "w1:p1",
            "--request",
            "request-1",
            "--changelog",
            "updated",
            "--json",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let client = crate::api::client::ApiClient::for_target(
            crate::api::client::ConnectionTarget::SocketPath(socket_path.clone()),
        );
        let code = crate::cli::target::with_test_client(client, || plugin_reload(&args)).unwrap();
        assert_eq!(code, 0);
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        // Reusing the same request for another pane must fail without replaying
        // the first pane's success or prompting either agent again.
        let mut wrong_pane_args = args.clone();
        wrong_pane_args[2] = "w1:p2".into();
        let client = crate::api::client::ApiClient::for_target(
            crate::api::client::ConnectionTarget::SocketPath(socket_path.clone()),
        );
        let code =
            crate::cli::target::with_test_client(client, || plugin_reload(&wrong_pane_args)).unwrap();
        assert_eq!(code, 1);
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert_eq!(completed_plugin_reload_record(&record_path), Some(record));
        std::fs::remove_dir_all(record_path.parent().unwrap().parent().unwrap()).unwrap();
        std::fs::remove_file(socket_path).unwrap();
    }

    fn unique_plugin_id(label: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        format!("test.{label}.{}.{nanos}", std::process::id())
    }

    fn github_plugin(
        id: &str,
        owner: &str,
        repo: &str,
        subdir: Option<&str>,
    ) -> InstalledPluginInfo {
        InstalledPluginInfo {
            plugin_id: id.to_string(),
            name: "Test Plugin".to_string(),
            version: "0.1.0".to_string(),
            min_herdr_version: crate::build_info::BASE_VERSION.to_string(),
            description: None,
            manifest_path: format!("/tmp/{id}/herdr-plugin.toml"),
            plugin_root: format!("/tmp/{id}"),
            enabled: true,
            platforms: None,
            build: vec![],
            startup: vec![],
            actions: vec![],
            events: vec![],
            panes: vec![],
            link_handlers: vec![],
            source: PluginSourceInfo {
                kind: PluginSourceKind::Github,
                owner: Some(owner.to_string()),
                repo: Some(repo.to_string()),
                subdir: subdir.map(str::to_string),
                requested_ref: None,
                resolved_commit: Some("abc123".to_string()),
                managed_path: Some(format!("/tmp/herdr/plugins/{id}")),
                installed_unix_ms: Some(42),
            },
            warnings: vec![],
        }
    }

    #[test]
    fn plugin_install_args_accept_options_around_source() {
        for (args, expected_ref, expected_yes) in [
            (vec!["owner/repo"], None, false),
            (vec!["--yes", "owner/repo"], None, true),
            (vec!["-y", "owner/repo"], None, true),
            (
                vec!["owner/repo", "--ref", "main", "-y"],
                Some("main"),
                true,
            ),
            (
                vec!["--ref", "main", "owner/repo", "--yes"],
                Some("main"),
                true,
            ),
            (
                vec![
                    "--yes",
                    "--ref",
                    "old",
                    "owner/repo",
                    "--ref",
                    "main",
                    "--yes",
                ],
                Some("main"),
                true,
            ),
            // Preserve ref-value consumption: a value named --yes is not consent.
            (vec!["owner/repo", "--ref", "--yes"], Some("--yes"), false),
        ] {
            let args = args.into_iter().map(String::from).collect::<Vec<_>>();
            let parsed = parse_plugin_install_args(&args).unwrap();
            assert_eq!(parsed.source.display(), "owner/repo", "{args:?}");
            assert_eq!(parsed.requested_ref.as_deref(), expected_ref, "{args:?}");
            assert_eq!(parsed.yes, expected_yes, "{args:?}");
        }
    }

    #[test]
    fn plugin_install_args_reject_invalid_syntax() {
        for (args, expected_error) in [
            (vec![], PLUGIN_INSTALL_USAGE),
            (vec!["--yes"], PLUGIN_INSTALL_USAGE),
            (vec!["owner/repo", "--ref"], "missing value for --ref"),
            (vec!["--unknown", "owner/repo"], "unknown option: --unknown"),
            (
                vec!["owner/repo", "extra/repo"],
                "unknown option: extra/repo",
            ),
            (vec!["owner"], PLUGIN_INSTALL_USAGE),
        ] {
            let args = args.into_iter().map(String::from).collect::<Vec<_>>();
            assert_eq!(
                parse_plugin_install_args(&args).unwrap_err(),
                expected_error,
                "{args:?}"
            );
        }
    }

    #[test]
    fn github_plugin_source_parses_root_repo() {
        let source = GithubPluginSource::parse("ogulcancelik/herdr-plugin-examples").unwrap();
        assert_eq!(source.owner, "ogulcancelik");
        assert_eq!(source.repo, "herdr-plugin-examples");
        assert_eq!(source.subdir, None);
        assert_eq!(
            source.remote_url(),
            "https://github.com/ogulcancelik/herdr-plugin-examples.git"
        );
    }

    #[test]
    fn github_plugin_source_parses_subdir() {
        let source =
            GithubPluginSource::parse("ogulcancelik/herdr-plugin-examples/worktree-bootstrap")
                .unwrap();
        assert_eq!(source.owner, "ogulcancelik");
        assert_eq!(source.repo, "herdr-plugin-examples");
        assert_eq!(source.subdir.as_deref(), Some("worktree-bootstrap"));
    }

    #[test]
    fn github_plugin_source_rejects_non_shorthand_sources() {
        for source in [
            "https://github.com/ogulcancelik/herdr-plugin-examples",
            "git@github.com:ogulcancelik/herdr-plugin-examples.git",
            "ogulcancelik",
            "ogulcancelik/herdr-plugin-examples/../bad",
            "ogulcancelik/herdr-plugin-examples//bad",
        ] {
            assert!(
                GithubPluginSource::parse(source).is_err(),
                "{source} should be rejected"
            );
        }
    }

    #[test]
    fn github_source_lookup_matches_installed_plugin_source() {
        let source =
            GithubPluginSource::parse("ogulcancelik/herdr-plugin-examples/agent-telegram-notify")
                .unwrap();
        let plugins = vec![
            github_plugin(
                "examples.github-link-preview",
                "ogulcancelik",
                "herdr-plugin-examples",
                Some("github-link-preview"),
            ),
            github_plugin(
                "examples.agent-telegram-notify",
                "ogulcancelik",
                "herdr-plugin-examples",
                Some("agent-telegram-notify"),
            ),
        ];

        let plugin = plugin_by_github_source(plugins, &source).unwrap();
        assert_eq!(plugin.plugin_id, "examples.agent-telegram-notify");
    }

    #[test]
    fn github_source_lookup_requires_exact_subdir() {
        let source = GithubPluginSource::parse("ogulcancelik/herdr-plugin-examples").unwrap();
        let plugins = vec![github_plugin(
            "examples.agent-telegram-notify",
            "ogulcancelik",
            "herdr-plugin-examples",
            Some("agent-telegram-notify"),
        )];

        assert!(plugin_by_github_source(plugins, &source).is_none());
    }

    #[test]
    fn github_source_lookup_ignores_local_plugins() {
        let source = GithubPluginSource::parse("ogulcancelik/herdr-plugin-examples").unwrap();
        let mut plugin = github_plugin(
            "examples.local",
            "ogulcancelik",
            "herdr-plugin-examples",
            None,
        );
        plugin.source = PluginSourceInfo::default();

        assert!(plugin_by_github_source([plugin], &source).is_none());
    }

    #[test]
    fn cli_user_dir_creation_seeds_legacy_config_before_printing_config_dir() {
        let plugin_id = unique_plugin_id("legacy-config");
        let config_dir = crate::plugin_paths::plugin_config_dir(&plugin_id);
        let state_dir = crate::plugin_paths::plugin_state_dir(&plugin_id);
        let legacy_dir = crate::config::config_dir().join("plugins").join(&plugin_id);
        let _ = std::fs::remove_dir_all(&config_dir);
        let _ = std::fs::remove_dir_all(&state_dir);
        let _ = std::fs::remove_dir_all(&legacy_dir);
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(legacy_dir.join(".env"), "TOKEN=legacy\n").unwrap();

        assert_eq!(
            plugin_config_dir_command(std::slice::from_ref(&plugin_id)).unwrap(),
            0
        );

        assert_eq!(
            std::fs::read_to_string(config_dir.join(".env")).unwrap(),
            "TOKEN=legacy\n"
        );

        let _ = std::fs::remove_dir_all(config_dir);
        let _ = std::fs::remove_dir_all(state_dir);
        let _ = std::fs::remove_dir_all(legacy_dir);
    }
}
