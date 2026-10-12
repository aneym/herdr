//! Whether a client connection was forwarded in over ssh from another machine.
//!
//! The Book's Herdr Shell reaches this server through an ssh-forwarded client socket, so the
//! peer process of its connection is the user's sshd session. Agent tabs are homed on the
//! server's machine; the server uses this origin to keep a remote client from taking one over
//! from a client on this machine (Alex, 2026-10-12: "the herdr tab keeps getting taken over by
//! another computer even though this is the only one being used").
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

fn remote_ids() -> &'static Mutex<HashSet<u64>> {
    static IDS: OnceLock<Mutex<HashSet<u64>>> = OnceLock::new();
    IDS.get_or_init(Default::default)
}

/// Records the origin of a freshly accepted client before its handshake runs.
pub(crate) fn note(client_id: u64, stream: &crate::ipc::LocalStream) {
    if stream_is_ssh_forwarded(stream) {
        remote_ids()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(client_id);
    }
}

/// Whether the client arrived over ssh; consumes the record.
pub(crate) fn take(client_id: u64) -> bool {
    remote_ids()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(&client_id)
}

pub(crate) fn is_ssh_process_name(name: &str) -> bool {
    name == "sshd" || name.starts_with("sshd-") || name.starts_with("sshd:")
}

#[cfg(unix)]
fn stream_is_ssh_forwarded(stream: &crate::ipc::LocalStream) -> bool {
    use std::os::fd::AsRawFd;
    let crate::ipc::LocalStream::UdSocket(stream) = stream;
    peer_pid(stream.inner().as_raw_fd())
        .and_then(process_name)
        .is_some_and(|name| is_ssh_process_name(&name))
}

#[cfg(not(unix))]
fn stream_is_ssh_forwarded(_stream: &crate::ipc::LocalStream) -> bool {
    false
}

#[cfg(target_os = "macos")]
fn peer_pid(fd: std::os::fd::RawFd) -> Option<i32> {
    let mut pid: libc::pid_t = 0;
    let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: fd is a live socket owned by the caller; pid and len are valid out-parameters.
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut len,
        )
    };
    (rc == 0 && pid > 0).then_some(pid)
}

#[cfg(target_os = "linux")]
fn peer_pid(fd: std::os::fd::RawFd) -> Option<i32> {
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: fd is a live socket owned by the caller; cred and len are valid out-parameters.
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    (rc == 0 && cred.pid > 0).then_some(cred.pid)
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn peer_pid(_fd: std::os::fd::RawFd) -> Option<i32> {
    None
}

#[cfg(target_os = "macos")]
fn process_name(pid: i32) -> Option<String> {
    let mut buffer = [0u8; 256];
    // SAFETY: buffer is writable for its full length.
    let len = unsafe { libc::proc_name(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    (len > 0).then(|| String::from_utf8_lossy(&buffer[..len as usize]).into_owned())
}

#[cfg(target_os = "linux")]
fn process_name(pid: i32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|name| name.trim().to_owned())
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn process_name(_pid: i32) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_session_process_names_are_remote() {
        assert!(is_ssh_process_name("sshd"));
        assert!(is_ssh_process_name("sshd-session"));
        assert!(is_ssh_process_name("sshd: aneyman@notty"));
        assert!(!is_ssh_process_name("herdr"));
        assert!(!is_ssh_process_name("HerdrShell"));
        assert!(!is_ssh_process_name("ssh"));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn a_same_machine_peer_resolves_to_this_process_and_is_local() {
        use std::os::fd::AsRawFd;
        let (a, _b) = std::os::unix::net::UnixStream::pair().expect("socket pair");
        let pid = peer_pid(a.as_raw_fd()).expect("peer pid");
        assert_eq!(pid as u32, std::process::id());
        let name = process_name(pid).expect("process name");
        assert!(!is_ssh_process_name(&name), "{name}");
        let stream = crate::ipc::LocalStream::from(
            interprocess::os::unix::uds_local_socket::Stream::from(a),
        );
        assert!(!stream_is_ssh_forwarded(&stream));
    }
}
