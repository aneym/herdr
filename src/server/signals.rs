//! Server signal policy.
//!
//! The sigaction handler only stores atomics. The event loop reads them and
//! decides whether the daemon should quit.

#[cfg(unix)]
use std::ffi::OsStr;
#[cfg(unix)]
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
#[cfg(unix)]
use std::sync::Mutex;
#[cfg(test)]
use std::time::{Duration, Instant};

#[cfg(test)]
const REPEAT_WINDOW: Duration = Duration::from_secs(5);
const REPEAT_WINDOW_NS: u64 = 5_000_000_000;

#[cfg(unix)]
struct SignalSlot {
    count: AtomicU64,
    sender_pid: AtomicI32,
    arrival_ns: AtomicU64,
    previous_arrival_ns: AtomicU64,
}

#[cfg(unix)]
impl SignalSlot {
    const fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            sender_pid: AtomicI32::new(0),
            arrival_ns: AtomicU64::new(0),
            previous_arrival_ns: AtomicU64::new(0),
        }
    }
}

#[cfg(unix)]
static INT_SLOT: SignalSlot = SignalSlot::new();
#[cfg(unix)]
static TERM_SLOT: SignalSlot = SignalSlot::new();
#[cfg(unix)]
static HUP_SLOT: SignalSlot = SignalSlot::new();

#[cfg(unix)]
static POLICY: Mutex<Policy> = Mutex::new(Policy {
    escape_quit: false,
    last_ignored: None,
    seen_int: 0,
    seen_term: 0,
    seen_hup: 0,
    queued: Vec::new(),
});

#[cfg(unix)]
struct Policy {
    escape_quit: bool,
    last_ignored: Option<(i32, u64)>,
    seen_int: u64,
    seen_term: u64,
    seen_hup: u64,
    queued: Vec<Delivered>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    Quit,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuitReason {
    InitProcess,
    Repeated,
    EscapeHatch,
}

#[cfg(unix)]
pub(crate) struct ObservedSignal {
    pub(crate) signal: i32,
    pub(crate) sender_pid: i32,
    pub(crate) decision: Decision,
    pub(crate) quit_reason: Option<QuitReason>,
}

#[cfg(test)]
pub(crate) fn decide(
    signal: i32,
    sender_pid: i32,
    now: Instant,
    last_ignored: Option<(i32, Instant)>,
    escape_quit: bool,
) -> Decision {
    classify(signal, sender_pid, now, last_ignored, escape_quit).0
}

#[cfg(test)]
fn classify(
    signal: i32,
    sender_pid: i32,
    now: Instant,
    last_ignored: Option<(i32, Instant)>,
    escape_quit: bool,
) -> (Decision, Option<QuitReason>) {
    if sender_pid == 1 {
        return (Decision::Quit, Some(QuitReason::InitProcess));
    }
    if let Some((previous, ignored_at)) = last_ignored {
        if previous == signal && now.saturating_duration_since(ignored_at) < REPEAT_WINDOW {
            return (Decision::Quit, Some(QuitReason::Repeated));
        }
    }
    if escape_quit {
        return (Decision::Quit, Some(QuitReason::EscapeHatch));
    }
    (Decision::Ignore, None)
}

#[derive(Clone, Copy)]
struct Delivered {
    signal: i32,
    sender_pid: i32,
    arrival_ns: u64,
}

fn classify_at(
    signal: i32,
    sender_pid: i32,
    arrival_ns: u64,
    last_ignored: Option<(i32, u64)>,
    escape_quit: bool,
) -> (Decision, Option<QuitReason>) {
    if sender_pid == 1 {
        return (Decision::Quit, Some(QuitReason::InitProcess));
    }
    if let Some((previous, ignored_at)) = last_ignored {
        if previous == signal && arrival_ns.saturating_sub(ignored_at) < REPEAT_WINDOW_NS {
            return (Decision::Quit, Some(QuitReason::Repeated));
        }
    }
    if escape_quit {
        return (Decision::Quit, Some(QuitReason::EscapeHatch));
    }
    (Decision::Ignore, None)
}

fn consume_one(
    queued: &mut Vec<Delivered>,
    last_ignored: &mut Option<(i32, u64)>,
    escape_quit: bool,
) -> Option<(i32, i32, Decision, Option<QuitReason>)> {
    if queued.is_empty() {
        return None;
    }
    let event = queued.remove(0);
    let (decision, quit_reason) = classify_at(
        event.signal,
        event.sender_pid,
        event.arrival_ns,
        *last_ignored,
        escape_quit,
    );
    if decision == Decision::Ignore {
        *last_ignored = Some((event.signal, event.arrival_ns));
    }
    Some((event.signal, event.sender_pid, decision, quit_reason))
}

fn extend_coalesced(
    signal: i32,
    sender_pid: i32,
    seen: u64,
    count: u64,
    previous_arrival_ns: u64,
    arrival_ns: u64,
    out: &mut Vec<Delivered>,
) -> u64 {
    let delta = count.saturating_sub(seen);
    if delta == 0 {
        return seen;
    }
    if delta >= 2 {
        out.push(Delivered {
            signal,
            sender_pid,
            arrival_ns: previous_arrival_ns,
        });
    }
    out.push(Delivered {
        signal,
        sender_pid,
        arrival_ns,
    });
    count
}

#[cfg(test)]
fn decisions_for_coalesced(
    pending: &[(i32, i32, u64, u64, u64, u64)],
    escape_quit: bool,
) -> Vec<Decision> {
    let mut queued = Vec::new();
    for (signal, sender_pid, seen, count, previous_arrival_ns, arrival_ns) in pending {
        extend_coalesced(
            *signal,
            *sender_pid,
            *seen,
            *count,
            *previous_arrival_ns,
            *arrival_ns,
            &mut queued,
        );
    }
    queued.sort_by_key(|event| (event.arrival_ns, event.signal));
    let mut last_ignored = None;
    let mut decisions = Vec::new();
    while let Some((_, _, decision, _)) = consume_one(&mut queued, &mut last_ignored, escape_quit)
    {
        decisions.push(decision);
    }
    decisions
}

#[cfg(not(unix))]
pub(crate) fn install() {}

/// Installs SIGINT, SIGTERM, and SIGHUP handlers.
///
/// `HERDR_SERVER_SIGNAL_QUIT` is sampled once, at server start.
#[cfg(unix)]
pub(crate) fn install() {
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        if let Err(err) = install_one(signal) {
            tracing::warn!(
                signal = signal_name(signal),
                error = %err,
                "failed to install server signal handler"
            );
        }
    }
    let mut policy = policy_lock();
    policy.escape_quit =
        std::env::var_os("HERDR_SERVER_SIGNAL_QUIT").as_deref() == Some(OsStr::new("1"));
    policy.last_ignored = None;
    policy.queued.clear();
    policy.seen_int = INT_SLOT.count.load(Ordering::Acquire);
    policy.seen_term = TERM_SLOT.count.load(Ordering::Acquire);
    policy.seen_hup = HUP_SLOT.count.load(Ordering::Acquire);
    drop(policy);
    tracing::info!("server signal handlers installed");
}

/// Reads one newly stored signal and records an ignore when that is the decision.
#[cfg(unix)]
pub(crate) fn poll() -> Option<ObservedSignal> {
    let mut policy = policy_lock();
    ingest(&mut policy);
    let escape_quit = policy.escape_quit;
    let mut last_ignored = policy.last_ignored;
    let (signal, sender_pid, decision, quit_reason) =
        consume_one(&mut policy.queued, &mut last_ignored, escape_quit)?;
    policy.last_ignored = last_ignored;
    Some(ObservedSignal {
        signal,
        sender_pid,
        decision,
        quit_reason,
    })
}

#[cfg(unix)]
pub(crate) fn log_ignored(signal: i32, sender_pid: i32) {
    let name = signal_name(signal);
    let comm = sender_comm(sender_pid);
    tracing::warn!(
        "ignored {name} from pid {sender_pid} ({comm}): stop it with `herdr server stop`, or send the signal again within 5 s"
    );
}

#[cfg(unix)]
pub(crate) fn log_quit(signal: i32, sender_pid: i32, reason: QuitReason) {
    let name = signal_name(signal);
    let comm = sender_comm(sender_pid);
    let why = match reason {
        QuitReason::InitProcess => "sender is pid 1",
        QuitReason::Repeated => "same signal repeated within 5 s",
        QuitReason::EscapeHatch => "HERDR_SERVER_SIGNAL_QUIT=1",
    };
    tracing::info!("quitting on {name} from pid {sender_pid} ({comm}): {why}");
}

#[cfg(unix)]
fn policy_lock() -> std::sync::MutexGuard<'static, Policy> {
    POLICY.lock().unwrap_or_else(|err| err.into_inner())
}

#[cfg(unix)]
fn ingest(policy: &mut Policy) {
    let mut fresh = Vec::new();
    take_slot(&INT_SLOT, libc::SIGINT, &mut policy.seen_int, &mut fresh);
    take_slot(&TERM_SLOT, libc::SIGTERM, &mut policy.seen_term, &mut fresh);
    take_slot(&HUP_SLOT, libc::SIGHUP, &mut policy.seen_hup, &mut fresh);
    if fresh.is_empty() {
        return;
    }
    policy.queued.extend(fresh);
    policy.queued.sort_by_key(|event| (event.arrival_ns, event.signal));
}

#[cfg(unix)]
fn take_slot(slot: &SignalSlot, signal: i32, seen: &mut u64, out: &mut Vec<Delivered>) {
    let Some(loaded) = load_slot(slot, *seen) else {
        return;
    };
    *seen = extend_coalesced(
        signal,
        loaded.sender_pid,
        *seen,
        loaded.count,
        loaded.previous_arrival_ns,
        loaded.arrival_ns,
        out,
    );
}

#[cfg(unix)]
struct LoadedSlot {
    count: u64,
    sender_pid: i32,
    arrival_ns: u64,
    previous_arrival_ns: u64,
}

#[cfg(unix)]
fn load_slot(slot: &SignalSlot, seen: u64) -> Option<LoadedSlot> {
    loop {
        let count = slot.count.load(Ordering::Acquire);
        if count == seen {
            return None;
        }
        let sender_pid = slot.sender_pid.load(Ordering::Relaxed);
        let arrival_ns = slot.arrival_ns.load(Ordering::Relaxed);
        let previous_arrival_ns = slot.previous_arrival_ns.load(Ordering::Relaxed);
        let again = slot.count.load(Ordering::Acquire);
        if again == count {
            return Some(LoadedSlot {
                count,
                sender_pid,
                arrival_ns,
                previous_arrival_ns,
            });
        }
    }
}

#[cfg(unix)]
fn install_one(signal: libc::c_int) -> std::io::Result<()> {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_flags = libc::SA_SIGINFO | libc::SA_RESTART;
    action.sa_sigaction = handle_server_signal as *const () as libc::sighandler_t;
    let installed = unsafe {
        libc::sigemptyset(&mut action.sa_mask);
        for masked in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            libc::sigaddset(&mut action.sa_mask, masked);
        }
        libc::sigaction(signal, &action, std::ptr::null_mut())
    };
    if installed == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(unix)]
extern "C" fn handle_server_signal(
    signal: libc::c_int,
    info: *mut libc::siginfo_t,
    _context: *mut libc::c_void,
) {
    let Some(slot) = slot_for(signal) else {
        return;
    };
    let pid = sender_pid(info);
    let arrival_ns = monotonic_ns();
    let previous_arrival_ns = slot.arrival_ns.load(Ordering::Relaxed);
    slot.previous_arrival_ns
        .store(previous_arrival_ns, Ordering::Relaxed);
    slot.sender_pid.store(pid, Ordering::Relaxed);
    slot.arrival_ns.store(arrival_ns, Ordering::Relaxed);
    slot.count.fetch_add(1, Ordering::Release);
}

#[cfg(unix)]
fn slot_for(signal: libc::c_int) -> Option<&'static SignalSlot> {
    if signal == libc::SIGINT {
        Some(&INT_SLOT)
    } else if signal == libc::SIGTERM {
        Some(&TERM_SLOT)
    } else if signal == libc::SIGHUP {
        Some(&HUP_SLOT)
    } else {
        None
    }
}

#[cfg(unix)]
fn monotonic_ns() -> u64 {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) } != 0 {
        return 0;
    }
    (time.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(time.tv_nsec as u64)
}

#[cfg(unix)]
fn sender_pid(info: *mut libc::siginfo_t) -> i32 {
    if info.is_null() {
        0
    } else {
        unsafe { (*info).si_pid() }
    }
}

#[cfg(unix)]
fn signal_name(signal: i32) -> &'static str {
    if signal == libc::SIGINT {
        "SIGINT"
    } else if signal == libc::SIGTERM {
        "SIGTERM"
    } else if signal == libc::SIGHUP {
        "SIGHUP"
    } else {
        "unknown"
    }
}

#[cfg(target_os = "macos")]
fn sender_comm(pid: i32) -> String {
    if pid <= 0 {
        return "unknown".to_string();
    }
    let mut buffer = [0u8; 256];
    let written = unsafe {
        libc::proc_name(
            pid,
            buffer.as_mut_ptr().cast::<libc::c_void>(),
            buffer.len() as u32,
        )
    };
    if written <= 0 {
        return "unknown".to_string();
    }
    let end = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(buffer.len());
    let name = String::from_utf8_lossy(&buffer[..end]).trim().to_string();
    if name.is_empty() {
        "unknown".to_string()
    } else {
        name
    }
}

#[cfg(target_os = "linux")]
fn sender_comm(pid: i32) -> String {
    if pid <= 0 {
        return "unknown".to_string();
    }
    std::fs::read(format!("/proc/{pid}/comm"))
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn sender_comm(_pid: i32) -> String {
    "unknown".to_string()
}

#[cfg(test)]
mod tests {
    use super::{decide, decisions_for_coalesced, Decision};
    use std::time::{Duration, Instant};

    const TERM: i32 = 15;
    const HUP: i32 = 1;

    #[test]
    fn term_from_pid_4242_is_ignored() {
        let now = Instant::now();
        assert_eq!(decide(TERM, 4242, now, None, false), Decision::Ignore);
    }

    #[test]
    fn term_from_pid_1_quits() {
        let now = Instant::now();
        assert_eq!(decide(TERM, 1, now, None, false), Decision::Quit);
    }

    #[test]
    fn repeated_term_within_5s_quits() {
        let ignored_at = Instant::now();
        let now = ignored_at + Duration::from_secs(1);
        assert_eq!(
            decide(TERM, 4242, now, Some((TERM, ignored_at)), false),
            Decision::Quit
        );
    }

    #[test]
    fn repeated_term_after_6s_is_ignored() {
        let ignored_at = Instant::now();
        let now = ignored_at + Duration::from_secs(6);
        assert_eq!(
            decide(TERM, 4242, now, Some((TERM, ignored_at)), false),
            Decision::Ignore
        );
    }

    #[test]
    fn hup_after_ignored_term_is_ignored() {
        let ignored_at = Instant::now();
        let now = ignored_at + Duration::from_secs(1);
        assert_eq!(
            decide(HUP, 4242, now, Some((TERM, ignored_at)), false),
            Decision::Ignore
        );
    }

    #[test]
    fn escape_env_quits() {
        let now = Instant::now();
        assert_eq!(decide(TERM, 4242, now, None, true), Decision::Quit);
    }

    #[test]
    fn hup_then_term_coalesced_before_poll_are_ignored() {
        let decisions = decisions_for_coalesced(
            &[
                (HUP, 4242, 0, 1, 0, 1_000),
                (TERM, 4242, 0, 1, 0, 2_000),
            ],
            false,
        );
        assert_eq!(decisions, vec![Decision::Ignore, Decision::Ignore]);
    }

    #[test]
    fn repeated_term_coalesced_before_poll_quits() {
        let t0 = 1_000_000_000_u64;
        let decisions =
            decisions_for_coalesced(&[(TERM, 4242, 0, 2, t0, t0 + 1_000_000)], false);
        assert_eq!(decisions, vec![Decision::Ignore, Decision::Quit]);
    }

    #[test]
    fn repeated_term_six_seconds_apart_polled_together_is_ignored() {
        let t0 = 1_000_000_000_u64;
        let decisions =
            decisions_for_coalesced(&[(TERM, 4242, 0, 2, t0, t0 + 6_000_000_000)], false);
        assert_eq!(decisions, vec![Decision::Ignore, Decision::Ignore]);
    }
}
