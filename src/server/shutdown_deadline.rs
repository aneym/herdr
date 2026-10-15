use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const DEFAULT_DEADLINE: Duration = Duration::from_millis(4000);
const DEADLINE_ENV: &str = "HERDR_SERVER_SHUTDOWN_DEADLINE_MS";
pub(crate) const PANE_TEARDOWN_BUDGET: Duration = Duration::from_millis(1500);
pub(crate) const EVENT_DRAIN_BUDGET: Duration = Duration::from_millis(500);

static ARMED: AtomicBool = AtomicBool::new(false);

fn deadline_from(value: Option<&str>) -> Duration {
    value
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|ms| *ms > 0)
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_DEADLINE)
}

pub(crate) fn arm(reason: &'static str) {
    if ARMED.swap(true, Ordering::AcqRel) {
        return;
    }
    let limit = deadline_from(std::env::var(DEADLINE_ENV).ok().as_deref());
    tracing::info!(
        reason,
        deadline_ms = limit.as_millis() as u64,
        "server shutdown deadline armed"
    );
    let spawned = std::thread::Builder::new()
        .name("herdr-shutdown-deadline".into())
        .spawn(move || {
            std::thread::sleep(limit);
            tracing::warn!(
                reason,
                deadline_ms = limit.as_millis() as u64,
                "server shutdown passed its deadline; exiting now"
            );
            exit_now();
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "could not start the shutdown deadline thread");
    }
}

#[cfg(unix)]
fn exit_now() -> ! {
    unsafe { libc::_exit(0) }
}

#[cfg(not(unix))]
fn exit_now() -> ! {
    std::process::exit(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadline_defaults_and_overrides() {
        assert_eq!(deadline_from(None), DEFAULT_DEADLINE);
        assert_eq!(deadline_from(Some("")), DEFAULT_DEADLINE);
        assert_eq!(deadline_from(Some("0")), DEFAULT_DEADLINE);
        assert_eq!(deadline_from(Some("junk")), DEFAULT_DEADLINE);
        assert_eq!(deadline_from(Some(" 2500 ")), Duration::from_millis(2500));
        assert!(PANE_TEARDOWN_BUDGET + EVENT_DRAIN_BUDGET < DEFAULT_DEADLINE);
    }
}
