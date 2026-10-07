use std::path::Path;

use serde::{Deserialize, Serialize};

const MAX_SESSION_ID_LEN: usize = 512;
const MAX_SESSION_PATH_LEN: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionRef {
    pub kind: AgentSessionRefKind,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionRefKind {
    Id,
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentResumePlan {
    pub agent: String,
    pub argv: Vec<String>,
    pub dedupe_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedAgentSession {
    pub source: String,
    pub agent: String,
    pub session_ref: AgentSessionRef,
}

impl AgentSessionRef {
    pub fn id(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        valid_session_id(&value).then_some(Self {
            kind: AgentSessionRefKind::Id,
            value,
        })
    }

    pub fn path(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        valid_session_path(&value).then_some(Self {
            kind: AgentSessionRefKind::Path,
            value,
        })
    }
}

pub fn session_ref_from_report(
    source: &str,
    agent: &str,
    agent_session_id: Option<String>,
    _agent_session_path: Option<String>,
) -> Option<AgentSessionRef> {
    if !is_official_agent_source(source, agent) {
        return None;
    }

    if agent == "pi" || agent == "omp" {
        return _agent_session_path
            .and_then(AgentSessionRef::path)
            .or_else(|| agent_session_id.and_then(AgentSessionRef::id));
    }

    agent_session_id.and_then(AgentSessionRef::id)
}

pub fn persisted_session_from_launch_args(
    agent: crate::detect::Agent,
    args: &[String],
) -> Option<PersistedAgentSession> {
    let [command, session_id] = args else {
        return None;
    };
    if agent != crate::detect::Agent::Codex || command != "resume" || session_id.starts_with('-') {
        return None;
    }

    Some(PersistedAgentSession {
        source: "herdr:codex".into(),
        agent: "codex".into(),
        session_ref: AgentSessionRef::id(session_id.clone())?,
    })
}

pub fn normalize_session_start_source(value: Option<String>) -> Option<String> {
    match value.as_deref().map(str::trim) {
        Some(
            source @ ("startup" | "resume" | "clear" | "compact" | "branch" | "new" | "fork"
            | "select"),
        ) => Some(source.to_string()),
        _ => None,
    }
}

pub fn is_reserved_native_state_source(source: &str, agent: &str) -> bool {
    matches!(
        (source, agent),
        ("herdr:claude", "claude")
            | ("herdr:codex", "codex")
            | ("herdr:copilot", "copilot")
            | ("herdr:devin", "devin")
            | ("herdr:droid", "droid")
            | ("herdr:qodercli", "qodercli")
            | ("herdr:qwen", "qwen")
            | ("herdr:cursor", "cursor")
            | ("herdr:grok", "grok")
    )
}

pub fn session_ref_from_snapshot(
    source: &str,
    agent: &str,
    kind: AgentSessionRefKind,
    value: &str,
) -> Option<PersistedAgentSession> {
    if !is_official_agent_source(source, agent) {
        return None;
    }
    let session_ref = match (agent, kind) {
        ("pi" | "omp", AgentSessionRefKind::Path) => AgentSessionRef::path(value)?,
        (_, AgentSessionRefKind::Id) => AgentSessionRef::id(value)?,
        _ => return None,
    };
    Some(PersistedAgentSession {
        source: source.to_string(),
        agent: agent.to_string(),
        session_ref,
    })
}

pub fn plan(source: &str, agent: &str, session_ref: &AgentSessionRef) -> Option<AgentResumePlan> {
    if !is_official_agent_source(source, agent) {
        return None;
    }

    let argv = match (source, agent, session_ref.kind) {
        ("herdr:claude", "claude", AgentSessionRefKind::Id) => {
            vec![
                "claude".into(),
                "--resume".into(),
                session_ref.value.clone(),
            ]
        }
        ("herdr:codex", "codex", AgentSessionRefKind::Id) => {
            vec!["codex".into(), "resume".into(), session_ref.value.clone()]
        }
        ("herdr:copilot", "copilot", AgentSessionRefKind::Id) => {
            vec!["copilot".into(), format!("--resume={}", session_ref.value)]
        }
        ("herdr:devin", "devin", AgentSessionRefKind::Id) => {
            vec!["devin".into(), "--resume".into(), session_ref.value.clone()]
        }
        ("herdr:droid", "droid", AgentSessionRefKind::Id) => {
            vec!["droid".into(), "--resume".into(), session_ref.value.clone()]
        }
        ("herdr:kimi", "kimi", AgentSessionRefKind::Id) => {
            vec!["kimi".into(), "--session".into(), session_ref.value.clone()]
        }
        ("herdr:mastracode", "mastracode", AgentSessionRefKind::Id) => {
            vec![
                "mastracode".into(),
                "--thread".into(),
                session_ref.value.clone(),
            ]
        }
        ("herdr:pi", "pi", AgentSessionRefKind::Path | AgentSessionRefKind::Id) => {
            vec!["pi".into(), "--session".into(), session_ref.value.clone()]
        }
        ("herdr:omp", "omp", AgentSessionRefKind::Path | AgentSessionRefKind::Id) => {
            // omp resume is `-r, --resume=<value>` (ID prefix or path); it has no
            // `--session` flag, unlike pi.
            vec!["omp".into(), format!("--resume={}", session_ref.value)]
        }
        ("herdr:hermes", "hermes", AgentSessionRefKind::Id) => {
            vec![
                "hermes".into(),
                "--resume".into(),
                session_ref.value.clone(),
            ]
        }
        ("herdr:opencode", "opencode", AgentSessionRefKind::Id) => {
            vec![
                "opencode".into(),
                "--session".into(),
                session_ref.value.clone(),
            ]
        }
        ("herdr:qodercli", "qodercli", AgentSessionRefKind::Id) => {
            vec![
                "qodercli".into(),
                "--resume".into(),
                session_ref.value.clone(),
            ]
        }
        ("herdr:qwen", "qwen", AgentSessionRefKind::Id) => {
            vec!["qwen".into(), "--resume".into(), session_ref.value.clone()]
        }
        ("herdr:kilo", "kilo", AgentSessionRefKind::Id) => {
            vec!["kilo".into(), "--session".into(), session_ref.value.clone()]
        }
        ("herdr:cursor", "cursor", AgentSessionRefKind::Id) => {
            vec![
                if cfg!(windows) {
                    "cursor-agent.cmd"
                } else {
                    "cursor-agent"
                }
                .into(),
                "--resume".into(),
                session_ref.value.clone(),
            ]
        }
        ("herdr:antigravity_cli", "agy", AgentSessionRefKind::Id) => {
            vec![
                "agy".into(),
                "--conversation".into(),
                session_ref.value.clone(),
            ]
        }
        ("herdr:grok", "grok", AgentSessionRefKind::Id) => {
            vec!["grok".into(), "--resume".into(), session_ref.value.clone()]
        }
        ("herdr:letta", "letta", AgentSessionRefKind::Id) => {
            if let Some(agent_id) = session_ref.value.strip_prefix("default:") {
                if agent_id.is_empty() {
                    return None;
                }
                vec![
                    "letta".into(),
                    "--conversation".into(),
                    "default".into(),
                    "--agent".into(),
                    agent_id.into(),
                ]
            } else {
                vec![
                    "letta".into(),
                    "--conversation".into(),
                    session_ref.value.clone(),
                ]
            }
        }
        _ => return None,
    };

    Some(AgentResumePlan {
        agent: agent.to_string(),
        argv,
        dedupe_key: dedupe_key(source, agent, session_ref),
    })
}

/// Resume argv for restarting a live agent in place.
///
/// When the pane's current foreground argv runs the same claude executable as
/// the plan, keep the user's own flags (model, settings, permissions) and only
/// swap the session-selection flags for `--resume <id>`. Resumption proceeds
/// only when its arguments can be mapped without losing flags.
pub fn resume_argv_preserving_flags(
    current_foreground_argv: &[String],
    plan: &AgentResumePlan,
) -> Result<Vec<String>, String> {
    let Some((current_program, current_args)) = current_foreground_argv.split_first() else {
        return Err("foreground argv cannot be mapped safely".into());
    };
    let Some(plan_program) = plan.argv.first() else {
        return Err("foreground argv cannot be mapped safely".into());
    };
    if plan.agent == "codex" && same_executable(current_program, "codex") {
        return codex_resume_argv(current_foreground_argv, plan);
    }
    if plan.agent != "claude" {
        return Ok(plan.argv.clone());
    }
    let wrapper = same_executable(current_program, "claude-lb-launch");
    let node_script = current_args.first().filter(|script| {
        if same_executable(script, "claude-lb-launch") {
            return true;
        }

        same_executable(current_program, "node")
            && (script.ends_with("/claude/cli.js")
                || script.contains("/claude-code/") && script.ends_with("/cli.js")
                || same_executable(script, "claude"))
    });
    if plan.agent != "claude"
        || (!same_executable(current_program, plan_program) && node_script.is_none() && !wrapper)
    {
        return Err("foreground argv cannot be mapped safely".into());
    }
    let Some(session_id) = plan
        .argv
        .iter()
        .position(|arg| arg == "--resume")
        .and_then(|index| plan.argv.get(index + 1))
    else {
        return Err("foreground argv cannot be mapped safely".into());
    };

    let mut argv = vec![current_program.clone()];
    let current_args = if let Some(script) = node_script {
        argv.push(script.clone());
        &current_args[1..]
    } else {
        current_args
    };
    let mut args = current_args.iter().peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--resume" | "-r" | "--session-id" => {
                if args.peek().is_some_and(|value| !value.starts_with('-')) {
                    args.next();
                }
            }
            "--continue" | "-c" | "--fork-session" => {}
            other if other.starts_with("--resume=") || other.starts_with("--session-id=") => {}
            _ if arg.starts_with('-') => {
                argv.push(arg.clone());
                if !arg.contains('=') && claude_option_takes_value(arg) {
                    let value = args
                        .peek()
                        .filter(|value| !value.starts_with('-'))
                        .ok_or_else(|| format!("missing claude option value: {arg}"))?;
                    argv.push((*value).clone());
                    args.next();
                }
            }
            _ => {
                // An unknown flag may take this value. Refuse rather than silently
                // dropping it as a prompt or replaying a prompt into the session.
                return Err("unclassified positional argument in claude argv".into());
            }
        }
    }
    argv.push("--resume".into());
    argv.push(session_id.clone());
    Ok(argv)
}

/// Whether a foreground argv[0] runs the plan's program (basename match,
/// tolerating a Windows `.exe` suffix).
pub(crate) fn same_executable(current: &str, planned: &str) -> bool {
    let basename = current.rsplit(['/', '\\']).next().unwrap_or(current);
    let planned = planned.rsplit(['/', '\\']).next().unwrap_or(planned);
    basename == planned
        || basename
            .strip_suffix(".exe")
            .or_else(|| basename.strip_suffix(".EXE"))
            .is_some_and(|stem| stem == planned)
}

pub fn dedupe_key(source: &str, agent: &str, session_ref: &AgentSessionRef) -> String {
    format!(
        "{source}\u{0}{agent}\u{0}{:?}\u{0}{}",
        session_ref.kind, session_ref.value
    )
}

pub(crate) fn is_official_agent_source(source: &str, agent: &str) -> bool {
    matches!(
        (source, agent),
        ("herdr:claude", "claude")
            | ("herdr:codex", "codex")
            | ("herdr:copilot", "copilot")
            | ("herdr:devin", "devin")
            | ("herdr:droid", "droid")
            | ("herdr:kimi", "kimi")
            | ("herdr:omp", "omp")
            | ("herdr:mastracode", "mastracode")
            | ("herdr:pi", "pi")
            | ("herdr:hermes", "hermes")
            | ("herdr:opencode", "opencode")
            | ("herdr:qodercli", "qodercli")
            | ("herdr:qwen", "qwen")
            | ("herdr:kilo", "kilo")
            | ("herdr:cursor", "cursor")
            | ("herdr:antigravity_cli", "agy")
            | ("herdr:grok", "grok")
            | ("herdr:letta", "letta")
    )
}

fn valid_session_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_SESSION_ID_LEN && !value.chars().any(char::is_control)
}

fn valid_session_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_SESSION_PATH_LEN
        && !value.chars().any(char::is_control)
        && Path::new(value).is_absolute()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn absolute_test_path(name: &str) -> String {
        std::env::current_dir()
            .unwrap()
            .join(name)
            .display()
            .to_string()
    }

    #[test]
    fn native_state_reservation_excludes_full_lifecycle_sources() {
        assert!(is_reserved_native_state_source("herdr:claude", "claude"));
        assert!(is_reserved_native_state_source("herdr:codex", "codex"));
        assert!(is_reserved_native_state_source("herdr:devin", "devin"));
        assert!(!is_reserved_native_state_source("herdr:kimi", "kimi"));
        assert!(!is_reserved_native_state_source(
            "herdr:opencode",
            "opencode"
        ));
    }

    #[test]
    fn codex_noncanonical_resume_launch_has_no_explicit_session() {
        assert_eq!(
            persisted_session_from_launch_args(
                crate::detect::Agent::Codex,
                &["resume".into(), "codex-session".into()]
            )
            .unwrap()
            .session_ref
            .value,
            "codex-session"
        );
        assert!(persisted_session_from_launch_args(
            crate::detect::Agent::Codex,
            &["resume".into(), "--last".into()]
        )
        .is_none());
        assert!(persisted_session_from_launch_args(
            crate::detect::Agent::Codex,
            &["resume".into(), "not-a-session".into(), "--last".into()]
        )
        .is_none());
        assert!(persisted_session_from_launch_args(
            crate::detect::Agent::Codex,
            &[
                "--remote".into(),
                "ws://example.test".into(),
                "resume".into(),
                "remote-session".into(),
            ]
        )
        .is_none());
    }

    #[test]
    fn planner_allows_supported_agents() {
        let pi_session = absolute_test_path("pi-session.jsonl");
        let omp_session = absolute_test_path("omp-session.jsonl");
        assert_eq!(
            plan(
                "herdr:claude",
                "claude",
                &AgentSessionRef::id("claude-session").unwrap()
            )
            .unwrap()
            .argv,
            vec!["claude", "--resume", "claude-session"]
        );
        assert_eq!(
            plan(
                "herdr:codex",
                "codex",
                &AgentSessionRef::id("codex-session").unwrap()
            )
            .unwrap()
            .argv,
            vec!["codex", "resume", "codex-session"]
        );
        assert_eq!(
            plan(
                "herdr:copilot",
                "copilot",
                &AgentSessionRef::id("copilot-session").unwrap()
            )
            .unwrap()
            .argv,
            vec!["copilot", "--resume=copilot-session"]
        );
        assert_eq!(
            plan(
                "herdr:devin",
                "devin",
                &AgentSessionRef::id("devin-session").unwrap()
            )
            .unwrap()
            .argv,
            vec!["devin", "--resume", "devin-session"]
        );
        assert_eq!(
            plan(
                "herdr:droid",
                "droid",
                &AgentSessionRef::id("droid-session").unwrap()
            )
            .unwrap()
            .argv,
            vec!["droid", "--resume", "droid-session"]
        );
        assert_eq!(
            plan(
                "herdr:kimi",
                "kimi",
                &AgentSessionRef::id("kimi-session").unwrap()
            )
            .unwrap()
            .argv,
            vec!["kimi", "--session", "kimi-session"]
        );
        assert_eq!(
            plan(
                "herdr:mastracode",
                "mastracode",
                &AgentSessionRef::id("mastracode-session").unwrap()
            )
            .unwrap()
            .argv,
            vec!["mastracode", "--thread", "mastracode-session"]
        );
        assert_eq!(
            plan(
                "herdr:pi",
                "pi",
                &AgentSessionRef::path(&pi_session).unwrap()
            )
            .unwrap()
            .argv,
            vec!["pi", "--session", pi_session.as_str()]
        );
        assert_eq!(
            plan(
                "herdr:omp",
                "omp",
                &AgentSessionRef::path(&omp_session).unwrap()
            )
            .unwrap()
            .argv,
            vec!["omp", format!("--resume={omp_session}").as_str()]
        );
        assert_eq!(
            plan(
                "herdr:hermes",
                "hermes",
                &AgentSessionRef::id("hermes-session").unwrap()
            )
            .unwrap()
            .argv,
            vec!["hermes", "--resume", "hermes-session"]
        );
        assert_eq!(
            plan(
                "herdr:opencode",
                "opencode",
                &AgentSessionRef::id("opencode-session").unwrap()
            )
            .unwrap()
            .argv,
            vec!["opencode", "--session", "opencode-session"]
        );
        assert_eq!(
            plan(
                "herdr:qodercli",
                "qodercli",
                &AgentSessionRef::id("qoder-session").unwrap()
            )
            .unwrap()
            .argv,
            vec!["qodercli", "--resume", "qoder-session"]
        );
        assert_eq!(
            plan(
                "herdr:qwen",
                "qwen",
                &AgentSessionRef::id("qwen-session").unwrap()
            )
            .unwrap()
            .argv,
            vec!["qwen", "--resume", "qwen-session"]
        );
        assert_eq!(
            plan(
                "herdr:kilo",
                "kilo",
                &AgentSessionRef::id("kilo-session").unwrap()
            )
            .unwrap()
            .argv,
            vec!["kilo", "--session", "kilo-session"]
        );
        assert_eq!(
            plan(
                "herdr:cursor",
                "cursor",
                &AgentSessionRef::id("cursor-session").unwrap()
            )
            .unwrap()
            .argv,
            vec![
                if cfg!(windows) {
                    "cursor-agent.cmd"
                } else {
                    "cursor-agent"
                },
                "--resume",
                "cursor-session",
            ]
        );
        assert_eq!(
            plan(
                "herdr:antigravity_cli",
                "agy",
                &AgentSessionRef::id("agy-session").unwrap()
            )
            .unwrap()
            .argv,
            vec!["agy", "--conversation", "agy-session"]
        );
        assert_eq!(
            plan(
                "herdr:grok",
                "grok",
                &AgentSessionRef::id("grok-session").unwrap()
            )
            .unwrap()
            .argv,
            vec!["grok", "--resume", "grok-session"]
        );
        assert_eq!(
            plan(
                "herdr:letta",
                "letta",
                &AgentSessionRef::id("conversation-123").unwrap()
            )
            .unwrap()
            .argv,
            vec!["letta", "--conversation", "conversation-123"]
        );
        assert_eq!(
            plan(
                "herdr:letta",
                "letta",
                &AgentSessionRef::id("default:agent-123").unwrap()
            )
            .unwrap()
            .argv,
            vec!["letta", "--conversation", "default", "--agent", "agent-123"]
        );
        assert!(plan(
            "herdr:letta",
            "letta",
            &AgentSessionRef::id("default:").unwrap()
        )
        .is_none());
    }

    #[test]
    fn planner_rejects_custom_and_unsupported_path_refs() {
        let claude_session = absolute_test_path("claude-session");
        assert!(plan(
            "custom:claude",
            "claude",
            &AgentSessionRef::id("session").unwrap()
        )
        .is_none());
        assert!(plan(
            "herdr:claude",
            "claude",
            &AgentSessionRef::path(&claude_session).unwrap()
        )
        .is_none());
    }

    #[test]
    fn report_ref_prefers_pi_and_omp_paths_and_validates_values() {
        let pi_session = absolute_test_path("pi-session.jsonl");
        let omp_session = absolute_test_path("omp-session.jsonl");
        let claude_session = absolute_test_path("claude-session");
        let copilot_session = absolute_test_path("copilot-session");
        let session_ref = session_ref_from_report(
            "herdr:pi",
            "pi",
            Some("pi-id".into()),
            Some(pi_session.clone()),
        )
        .unwrap();
        assert_eq!(session_ref.kind, AgentSessionRefKind::Path);
        assert_eq!(session_ref.value, pi_session);

        assert!(session_ref_from_report("herdr:pi", "pi", Some("bad\nid".into()), None).is_none());
        assert!(
            session_ref_from_report("herdr:pi", "pi", None, Some("relative.jsonl".into()))
                .is_none()
        );
        assert!(session_ref_from_report("custom:pi", "pi", Some("pi-id".into()), None).is_none());

        let session_ref = session_ref_from_report(
            "herdr:omp",
            "omp",
            Some("omp-id".into()),
            Some(omp_session.clone()),
        )
        .unwrap();
        assert_eq!(session_ref.kind, AgentSessionRefKind::Path);
        assert_eq!(session_ref.value, omp_session);

        let session_ref =
            session_ref_from_report("herdr:omp", "omp", Some("omp-id".into()), None).unwrap();
        assert_eq!(session_ref.kind, AgentSessionRefKind::Id);
        assert_eq!(session_ref.value, "omp-id");
        let session_ref = session_ref_from_report(
            "herdr:omp",
            "omp",
            Some("omp-id".into()),
            Some("relative.jsonl".into()),
        )
        .unwrap();
        assert_eq!(session_ref.kind, AgentSessionRefKind::Id);
        assert_eq!(session_ref.value, "omp-id");
        assert!(
            session_ref_from_report("herdr:omp", "omp", None, Some("relative.jsonl".into()))
                .is_none()
        );

        assert!(
            session_ref_from_report("herdr:claude", "claude", None, Some(claude_session)).is_none()
        );

        let session_ref =
            session_ref_from_report("herdr:copilot", "copilot", Some("copilot-id".into()), None)
                .unwrap();
        assert_eq!(session_ref.kind, AgentSessionRefKind::Id);
        assert_eq!(session_ref.value, "copilot-id");
        assert!(
            session_ref_from_report("herdr:copilot", "copilot", None, Some(copilot_session))
                .is_none()
        );

        let session_ref =
            session_ref_from_report("herdr:devin", "devin", Some("devin-id".into()), None).unwrap();
        assert_eq!(session_ref.kind, AgentSessionRefKind::Id);
        assert_eq!(session_ref.value, "devin-id");

        let session_ref =
            session_ref_from_report("herdr:droid", "droid", Some("droid-id".into()), None).unwrap();
        assert_eq!(session_ref.kind, AgentSessionRefKind::Id);
        assert_eq!(session_ref.value, "droid-id");
        assert!(session_ref_from_report(
            "herdr:droid",
            "droid",
            None,
            Some("/tmp/droid-session".into())
        )
        .is_none());

        let session_ref =
            session_ref_from_report("herdr:kimi", "kimi", Some("kimi-id".into()), None).unwrap();
        assert_eq!(session_ref.kind, AgentSessionRefKind::Id);
        assert_eq!(session_ref.value, "kimi-id");

        let session_ref = session_ref_from_report(
            "herdr:mastracode",
            "mastracode",
            Some("mastracode-id".into()),
            None,
        )
        .unwrap();
        assert_eq!(session_ref.kind, AgentSessionRefKind::Id);
        assert_eq!(session_ref.value, "mastracode-id");

        let session_ref =
            session_ref_from_report("herdr:kilo", "kilo", Some("kilo-id".into()), None).unwrap();
        assert_eq!(session_ref.kind, AgentSessionRefKind::Id);
        assert_eq!(session_ref.value, "kilo-id");

        let session_ref =
            session_ref_from_report("herdr:qodercli", "qodercli", Some("qoder-id".into()), None)
                .unwrap();
        assert_eq!(session_ref.kind, AgentSessionRefKind::Id);
        assert_eq!(session_ref.value, "qoder-id");

        let session_ref =
            session_ref_from_report("herdr:qwen", "qwen", Some("qwen-id".into()), None).unwrap();
        assert_eq!(session_ref.kind, AgentSessionRefKind::Id);
        assert_eq!(session_ref.value, "qwen-id");

        let session_ref =
            session_ref_from_report("herdr:antigravity_cli", "agy", Some("agy-id".into()), None)
                .unwrap();
        assert_eq!(session_ref.kind, AgentSessionRefKind::Id);
        assert_eq!(session_ref.value, "agy-id");
    }

    #[test]
    fn normalize_session_start_source_allows_known_values() {
        assert_eq!(
            normalize_session_start_source(Some("startup".into())),
            Some("startup".into())
        );
        assert_eq!(
            normalize_session_start_source(Some("resume".into())),
            Some("resume".into())
        );
        assert_eq!(
            normalize_session_start_source(Some("clear".into())),
            Some("clear".into())
        );
        assert_eq!(
            normalize_session_start_source(Some("compact".into())),
            Some("compact".into())
        );
        assert_eq!(
            normalize_session_start_source(Some("branch".into())),
            Some("branch".into())
        );
        assert_eq!(
            normalize_session_start_source(Some("new".into())),
            Some("new".into())
        );
        assert_eq!(
            normalize_session_start_source(Some("fork".into())),
            Some("fork".into())
        );
        assert_eq!(
            normalize_session_start_source(Some("select".into())),
            Some("select".into())
        );
        assert_eq!(
            normalize_session_start_source(Some(" resume ".into())),
            Some("resume".into())
        );
        assert_eq!(normalize_session_start_source(Some("other".into())), None);
        assert_eq!(normalize_session_start_source(None), None);
    }

    #[test]
    fn ids_are_data_not_shell_text() {
        let id = "abc; rm -rf /";
        let codex_plan = plan("herdr:codex", "codex", &AgentSessionRef::id(id).unwrap()).unwrap();
        assert_eq!(codex_plan.argv, vec!["codex", "resume", id]);

        let copilot_plan = plan(
            "herdr:copilot",
            "copilot",
            &AgentSessionRef::id(id).unwrap(),
        )
        .unwrap();
        assert_eq!(copilot_plan.argv, vec!["copilot", "--resume=abc; rm -rf /"]);

        let devin_plan = plan("herdr:devin", "devin", &AgentSessionRef::id(id).unwrap()).unwrap();
        assert_eq!(devin_plan.argv, vec!["devin", "--resume", id]);
    }

    #[test]
    fn planner_rejects_path_refs_for_id_only_agents() {
        let hermes_session = absolute_test_path("hermes-session");
        let opencode_session = absolute_test_path("opencode-session");
        let kilo_session = absolute_test_path("kilo-session");
        let copilot_session = absolute_test_path("copilot-session");
        let devin_session = absolute_test_path("devin-session");
        assert!(plan(
            "herdr:hermes",
            "hermes",
            &AgentSessionRef::path(&hermes_session).unwrap()
        )
        .is_none());
        assert!(plan(
            "herdr:opencode",
            "opencode",
            &AgentSessionRef::path(&opencode_session).unwrap()
        )
        .is_none());
        assert!(plan(
            "herdr:kilo",
            "kilo",
            &AgentSessionRef::path(&kilo_session).unwrap()
        )
        .is_none());
        assert!(plan(
            "herdr:copilot",
            "copilot",
            &AgentSessionRef::path(&copilot_session).unwrap()
        )
        .is_none());
        assert!(plan(
            "herdr:devin",
            "devin",
            &AgentSessionRef::path(&devin_session).unwrap()
        )
        .is_none());
        assert!(session_ref_from_snapshot(
            "herdr:mastracode",
            "mastracode",
            AgentSessionRefKind::Id,
            "mastracode-session"
        )
        .is_some());
        assert!(session_ref_from_snapshot(
            "herdr:hermes",
            "hermes",
            AgentSessionRefKind::Id,
            "hermes-session"
        )
        .is_some());
        assert!(session_ref_from_snapshot(
            "herdr:opencode",
            "opencode",
            AgentSessionRefKind::Id,
            "opencode-session"
        )
        .is_some());
        assert!(session_ref_from_snapshot(
            "herdr:kilo",
            "kilo",
            AgentSessionRefKind::Id,
            "kilo-session"
        )
        .is_some());
        assert!(session_ref_from_snapshot(
            "herdr:copilot",
            "copilot",
            AgentSessionRefKind::Id,
            "copilot-session"
        )
        .is_some());
        assert!(session_ref_from_snapshot(
            "herdr:devin",
            "devin",
            AgentSessionRefKind::Id,
            "devin-session"
        )
        .is_some());
        assert!(session_ref_from_snapshot(
            "herdr:antigravity_cli",
            "agy",
            AgentSessionRefKind::Id,
            "agy-session"
        )
        .is_some());
        let agy_session = absolute_test_path("agy-session");
        assert!(plan(
            "herdr:antigravity_cli",
            "agy",
            &AgentSessionRef::path(&agy_session).unwrap()
        )
        .is_none());
    }

    fn claude_plan(session: &str) -> AgentResumePlan {
        plan(
            "herdr:claude",
            "claude",
            &AgentSessionRef::id(session).unwrap(),
        )
        .unwrap()
    }

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| part.to_string()).collect()
    }

    #[test]
    fn resume_argv_keeps_the_real_pinned_agent_flags() {
        let current = argv(&[
            "claude",
            "--settings",
            "{\"viewMode\":\"focus\"}",
            "--autocompact",
            "300k",
            "--dangerously-skip-permissions",
            "--model",
            "opus[1m]",
            "--effort",
            "medium",
        ]);
        assert_eq!(
            resume_argv_preserving_flags(&current, &claude_plan("sess-1")).unwrap(),
            argv(&[
                "claude",
                "--settings",
                "{\"viewMode\":\"focus\"}",
                "--autocompact",
                "300k",
                "--dangerously-skip-permissions",
                "--model",
                "opus[1m]",
                "--effort",
                "medium",
                "--resume",
                "sess-1",
            ])
        );
    }

    #[test]
    fn resume_argv_replaces_existing_session_selection_flags() {
        let plan = claude_plan("new-session");
        assert_eq!(
            resume_argv_preserving_flags(
                &argv(&["claude", "--resume", "old", "--model", "opus"]),
                &plan
            )
            .unwrap(),
            argv(&["claude", "--model", "opus", "--resume", "new-session"])
        );
        assert_eq!(
            resume_argv_preserving_flags(&argv(&["claude", "-c", "--model", "opus"]), &plan)
                .unwrap(),
            argv(&["claude", "--model", "opus", "--resume", "new-session"])
        );
        assert_eq!(
            resume_argv_preserving_flags(
                &argv(&[
                    "/usr/local/bin/claude",
                    "--resume=old",
                    "-r",
                    "older",
                    "--continue",
                    "--session-id",
                    "fixed",
                    "--session-id=other",
                    "--fork-session",
                    "--resume",
                    "--verbose",
                ]),
                &plan
            )
            .unwrap(),
            argv(&[
                "/usr/local/bin/claude",
                "--verbose",
                "--resume",
                "new-session"
            ])
        );
    }

    #[test]
    fn resume_argv_rejects_unmapped_flags_and_preserves_node_prefix() {
        let plan = claude_plan("sess-1");
        for current in [
            argv(&["claude-wrapper", "--model", "opus"]),
            Vec::new(),
            argv(&["node", "/other/cli.js", "--model", "opus"]),
        ] {
            assert!(resume_argv_preserving_flags(&current, &plan).is_err());
        }
        assert_eq!(
            resume_argv_preserving_flags(
                &argv(&["node", "/opt/claude/cli.js", "--model", "opus"]),
                &plan
            )
            .unwrap(),
            argv(&[
                "node",
                "/opt/claude/cli.js",
                "--model",
                "opus",
                "--resume",
                "sess-1"
            ])
        );
        let codex = super::plan(
            "herdr:codex",
            "codex",
            &AgentSessionRef::id("codex-session").unwrap(),
        )
        .unwrap();
        assert_eq!(
            resume_argv_preserving_flags(&argv(&["codex", "--model", "o4"]), &codex).unwrap(),
            argv(&["codex", "--model", "o4", "resume", "codex-session"])
        );
    }
}

/// Retain supported global options, never an old prompt or session selector.
fn codex_resume_argv(current: &[String], plan: &AgentResumePlan) -> Result<Vec<String>, String> {
    let mut argv = vec![current[0].clone()];
    let mut args = current[1..].iter();
    while let Some(arg) = args.next() {
        let name = arg.split('=').next().unwrap_or(arg);
        match name {
            "-m" | "--model" | "-c" | "--config" | "-s" | "--sandbox" | "-a"
            | "--ask-for-approval" | "-p" | "--profile" | "-C" | "--cd" | "-i" | "--image" => {
                argv.push(arg.clone());
                if !arg.contains('=') {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing codex option value".to_string())?;
                    argv.push(value.clone());
                }
            }
            "--dangerously-bypass-approvals-and-sandbox" | "--full-auto" | "--search" => {
                argv.push(arg.clone())
            }
            _ => {}
        }
    }
    argv.extend(plan.argv.iter().skip(1).cloned());
    Ok(argv)
}

fn claude_option_takes_value(arg: &str) -> bool {
    matches!(
        arg,
        "--model"
            | "--name"
            | "-m"
            | "--effort"
            | "--settings"
            | "--settings-sources"
            | "--permission-mode"
            | "--system-prompt"
            | "--append-system-prompt"
            | "--system-prompt-file"
            | "--append-system-prompt-file"
            | "--mcp-config"
            | "--agents"
            | "--agent"
            | "--tools"
            | "--allowedTools"
            | "--allowed-tools"
            | "--disallowedTools"
            | "--disallowed-tools"
            | "--add-dir"
            | "--plugin-dir"
            | "--output-format"
            | "--input-format"
            | "--max-budget-usd"
            | "--max-turns"
            | "--fallback-model"
            | "--betas"
            | "--autocompact"
    )
}

pub(crate) fn restart_launch_argv(
    leaf: &[String],
    recorded: Option<&[String]>,
    configured: Option<&str>,
    plan: &AgentResumePlan,
) -> Result<(Vec<String>, &'static str), String> {
    if let Some(recorded) = recorded.filter(|argv| {
        let shell_command = argv.first().is_some_and(|program| {
            [
                "sh",
                "bash",
                "zsh",
                "fish",
                "dash",
                "ksh",
                "cmd",
                "powershell",
                "pwsh",
            ]
            .iter()
            .any(|shell| same_executable(program, shell))
        }) && argv
            .iter()
            .skip(1)
            .any(|arg| matches!(arg.as_str(), "-c" | "-lc" | "/c" | "-Command"));
        !argv.is_empty() && !shell_command
    }) {
        if recorded
            .first()
            .is_some_and(|program| same_executable(program, &plan.agent))
            || recorded
                .iter()
                .take(2)
                .any(|program| same_executable(program, "claude-lb-launch"))
        {
            return resume_argv_preserving_flags(recorded, plan).map(|argv| (argv, "recorded"));
        }
        let mut argv = recorded.to_vec();
        // An interpreter launcher already owns its script argument; do not
        // classify that same script as an agent prompt.
        let mut forwarded_leaf = leaf.to_vec();
        if leaf
            .get(1)
            .is_some_and(|script| recorded.get(1) == Some(script))
        {
            forwarded_leaf.remove(1);
        }
        // Recorded generic launchers forward the agent's arguments.
        argv.extend(
            resume_argv_preserving_flags(&forwarded_leaf, plan)?
                .into_iter()
                .skip(1),
        );
        return Ok((argv, "recorded"));
    }
    let mut argv = resume_argv_preserving_flags(leaf, plan)?;
    if let Some(launcher) = configured.filter(|launcher| !launcher.trim().is_empty()) {
        argv[0] = launcher.to_string();
        return Ok((argv, "configured"));
    }
    Ok((argv, "direct"))
}

/// Summaries expose option names only; option values may contain credentials.
pub(crate) fn restart_command_summary(argv: &[String]) -> String {
    let mut summary = argv.first().cloned().into_iter().collect::<Vec<_>>();
    let mut args = argv.iter().skip(1);
    while let Some(arg) = args.next() {
        if !arg.starts_with('-') {
            continue;
        }
        let name = arg.split('=').next().unwrap_or(arg);
        summary.push(name.to_string());
        if !arg.contains('=')
            && (claude_option_takes_value(name)
                || matches!(
                    name,
                    "--resume"
                        | "-r"
                        | "--session-id"
                        | "--config"
                        | "-s"
                        | "--sandbox"
                        | "-a"
                        | "--ask-for-approval"
                        | "-p"
                        | "--profile"
                        | "-C"
                        | "--cd"
                        | "-i"
                        | "--image"
                ))
        {
            args.next();
        }
    }
    summary.join(" ")
}

#[cfg(test)]
mod restart_tests {
    use super::*;
    fn strings(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    /// Pure argv/filter algorithms have multiple selector and option edge cases.
    #[test]
    fn agent_resume_restart_rewrite_table() {
        let session = AgentSessionRef::id("new-session").unwrap();
        for (agent, input, expected) in [
            (
                "claude",
                vec!["claude", "--continue", "--model", "opus"],
                vec!["claude", "--model", "opus", "--resume", "new-session"],
            ),
            (
                "claude",
                vec![
                    "python3",
                    "/bin/claude-lb-launch",
                    "--resume=old",
                    "--effort",
                    "high",
                ],
                vec![
                    "python3",
                    "/bin/claude-lb-launch",
                    "--effort",
                    "high",
                    "--resume",
                    "new-session",
                ],
            ),
            (
                "claude",
                vec!["claude", "--name", "worker", "--unknown"],
                vec![
                    "claude",
                    "--name",
                    "worker",
                    "--unknown",
                    "--resume",
                    "new-session",
                ],
            ),
            (
                "codex",
                vec!["codex", "--full-auto", "--search", "-i", "picture.png"],
                vec![
                    "codex",
                    "--full-auto",
                    "--search",
                    "-i",
                    "picture.png",
                    "resume",
                    "new-session",
                ],
            ),
            (
                "codex",
                vec![
                    "codex",
                    "-m",
                    "sol",
                    "-c",
                    "key=true",
                    "--sandbox=workspace-write",
                    "--dangerously-bypass-approvals-and-sandbox",
                    "old prompt",
                ],
                vec![
                    "codex",
                    "-m",
                    "sol",
                    "-c",
                    "key=true",
                    "--sandbox=workspace-write",
                    "--dangerously-bypass-approvals-and-sandbox",
                    "resume",
                    "new-session",
                ],
            ),
        ] {
            let plan = plan(&format!("herdr:{agent}"), agent, &session).unwrap();
            assert_eq!(
                resume_argv_preserving_flags(&strings(&input), &plan).unwrap(),
                strings(&expected)
            );
        }
        assert!(plan("custom:unknown", "unknown", &session).is_none());
    }

    /// Pure launcher precedence and prompt stripping algorithms have multiple edge cases.
    #[test]
    fn agent_resume_restart_launcher_precedence_and_prompt_removal() {
        let plan = plan(
            "herdr:claude",
            "claude",
            &AgentSessionRef::id("session").unwrap(),
        )
        .unwrap();
        assert!(resume_argv_preserving_flags(
            &strings(&["claude", "--future-flag", "value"]),
            &plan
        )
        .is_err());
        assert!(resume_argv_preserving_flags(&strings(&["claude", "old prompt"]), &plan).is_err());
        assert_eq!(
            restart_command_summary(&strings(&[
                "claude",
                "--name",
                "--private-name",
                "--settings=private",
                "--resume",
                "private-session"
            ])),
            "claude --name --settings --resume"
        );
        let leaf = strings(&["claude", "--model", "opus"]);
        let (configured, source) =
            restart_launch_argv(&leaf, None, Some("claude-lb-launch"), &plan).unwrap();
        assert_eq!(source, "configured");
        assert_eq!(
            configured,
            strings(&["claude-lb-launch", "--model", "opus", "--resume", "session"])
        );
        let recorded = strings(&["claude-lb-launch", "--effort", "high"]);
        let (argv, source) =
            restart_launch_argv(&leaf, Some(&recorded), Some("ignored"), &plan).unwrap();
        assert_eq!(source, "recorded");
        assert_eq!(
            argv,
            strings(&[
                "claude-lb-launch",
                "--effort",
                "high",
                "--resume",
                "session"
            ])
        );
        let shell = strings(&["sh", "-c", "claude --model opus"]);
        let (argv, source) =
            restart_launch_argv(&leaf, Some(&shell), Some("claude-lb-launch"), &plan).unwrap();
        assert_eq!(source, "configured");
        assert_eq!(argv, configured);
        let (argv, source) = restart_launch_argv(&leaf, None, None, &plan).unwrap();
        assert_eq!(source, "direct");
        assert_eq!(
            argv,
            strings(&["claude", "--model", "opus", "--resume", "session"])
        );
    }
}
