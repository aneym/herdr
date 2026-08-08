//! Durable agent ownership — origin lineage and transferable current ownership.
//!
//! An agent occupancy of a terminal carries a durable `agent identity`
//! (`agent_<hex>`), independent of pane ids and terminal ids. A worker agent
//! spawned or adopted by another agent records that owner as an
//! [`AgentOwnerRef`]. The origin owner is immutable once recorded; the current
//! owner is transferable and clearable. When a current owner reference can no
//! longer be resolved to a live agent, the worker is orphaned rather than
//! silently flattened.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::agent_resume::PersistedAgentSession;

static NEXT_AGENT_IDENTITY: AtomicU64 = AtomicU64::new(1);

/// Allocate a durable agent identity for one agent occupancy of a terminal.
///
/// Pane ids are presentation, not identity; this value is the durable owner
/// key persisted across restarts and pane moves.
pub fn alloc_agent_identity() -> String {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or(0);
    let counter = NEXT_AGENT_IDENTITY.fetch_add(1, Ordering::Relaxed);
    format!("agent_{micros:x}{counter:x}")
}

/// A durable reference to an owning agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentOwnerRef {
    /// Durable agent identity of the owner.
    pub agent_id: String,
    /// Owner agent name snapshot at capture time (display only).
    pub name: Option<String>,
    /// Owner agent kind label snapshot at capture time (display only).
    pub agent: Option<String>,
    /// Owner agent session identity when known — allows reconciling the owner
    /// after its original terminal is gone but its session was resumed.
    pub session: Option<PersistedAgentSession>,
}

/// Ownership record carried by a worker agent's terminal occupancy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentOwnership {
    /// The first recorded owner. Immutable once the record exists.
    pub origin: AgentOwnerRef,
    /// The owner the worker currently belongs to. `None` after an explicit
    /// release: the worker is a root by choice, not an orphan.
    pub current: Option<AgentOwnerRef>,
}

impl AgentOwnership {
    pub fn new(origin: AgentOwnerRef) -> Self {
        Self {
            current: Some(origin.clone()),
            origin,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_identities_are_unique_and_prefixed() {
        let first = alloc_agent_identity();
        let second = alloc_agent_identity();
        assert!(first.starts_with("agent_"));
        assert!(second.starts_with("agent_"));
        assert_ne!(first, second);
    }

    #[test]
    fn new_ownership_starts_with_current_equal_to_origin() {
        let owner = AgentOwnerRef {
            agent_id: "agent_1".into(),
            name: Some("lead".into()),
            agent: Some("claude".into()),
            session: None,
        };
        let ownership = AgentOwnership::new(owner.clone());
        assert_eq!(ownership.origin, owner);
        assert_eq!(ownership.current, Some(owner));
    }
}
