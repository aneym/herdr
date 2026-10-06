//! Relays herdr's Windows named pipes to TCP ports.
//!
//! Herdr on Windows speaks over named pipes; Win32-OpenSSH can only forward
//! remote unix sockets to local TCP ports. This bridges the two so a local
//! `herdr` CLI or TUI reaches a remote server through `ssh -L`.
//!
//! Usage: herdr-pipe-relay <socket-path>=<host:port> [...]
//! The pipe name is derived from <socket-path> exactly as herdr does
//! (GenericNamespaced of the path string), and a marker file is written there.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::PathBuf;
use std::thread;

use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions, Stream};

fn main() {
    let maps: Vec<(PathBuf, String)> = std::env::args()
        .skip(1)
        .filter_map(|arg| {
            let (path, addr) = arg.split_once('=')?;
            Some((PathBuf::from(path), addr.to_string()))
        })
        .collect();
    if maps.is_empty() {
        eprintln!("usage: herdr-pipe-relay <socket-path>=<host:port> [...]");
        std::process::exit(2);
    }
    let mut handles = Vec::new();
    for (path, addr) in maps {
        handles.push(thread::spawn(move || {
            if let Err(err) = serve(&path, &addr) {
                eprintln!("relay {} -> {addr}: {err}", path.display());
                std::process::exit(1);
            }
        }));
    }
    for handle in handles {
        let _ = handle.join();
    }
}

#[cfg(windows)]
fn bind(path: &PathBuf) -> io::Result<interprocess::local_socket::Listener> {
    use interprocess::os::windows::local_socket::ListenerOptionsExt as _;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    use widestring::U16CString;

    // Same DACL as herdr's private listeners: SYSTEM and the owner only.
    let sddl = U16CString::from_str("D:P(A;;GA;;;SY)(A;;GA;;;OW)")
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
    let security_descriptor = SecurityDescriptor::deserialize(&sddl)?;
    let name = path.to_string_lossy().to_string();
    let name = name.to_ns_name::<GenericNamespaced>()?;
    let listener = ListenerOptions::new()
        .name(name)
        .reclaim_name(false)
        .security_descriptor(security_descriptor)
        .create_sync()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, format!("relay:{}\n", std::process::id()))?;
    Ok(listener)
}

#[cfg(not(windows))]
fn bind(path: &PathBuf) -> io::Result<interprocess::local_socket::Listener> {
    let name = path.to_string_lossy().to_string();
    let name = name.to_ns_name::<GenericNamespaced>()?;
    ListenerOptions::new().name(name).create_sync()
}

fn serve(path: &PathBuf, addr: &str) -> io::Result<()> {
    let listener = bind(path)?;
    eprintln!("relay {} -> {addr}", path.display());
    for conn in listener.incoming() {
        let pipe = match conn {
            Ok(pipe) => pipe,
            Err(err) => {
                eprintln!("accept {}: {err}", path.display());
                continue;
            }
        };
        let addr = addr.to_string();
        thread::spawn(move || {
            if let Err(err) = splice(pipe, &addr) {
                eprintln!("connection -> {addr}: {err}");
            }
        });
    }
    Ok(())
}

fn splice(pipe: Stream, addr: &str) -> io::Result<()> {
    let tcp = TcpStream::connect(addr)?;
    tcp.set_nodelay(true)?;
    let mut tcp_read = tcp.try_clone()?;
    let mut tcp_write = tcp;
    let (mut pipe_read, mut pipe_write) = pipe.split();

    let upstream = thread::spawn(move || {
        let _ = copy(&mut pipe_read, &mut tcp_write);
        let _ = tcp_write.shutdown(Shutdown::Write);
    });
    let _ = copy(&mut tcp_read, &mut pipe_write);
    let _ = tcp_read.shutdown(Shutdown::Both);
    let _ = upstream.join();
    Ok(())
}

fn copy(from: &mut impl Read, to: &mut impl Write) -> io::Result<u64> {
    let mut buf = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = match from.read(&mut buf) {
            Ok(0) => return Ok(total),
            Ok(n) => n,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        to.write_all(&buf[..n])?;
        to.flush()?;
        total += n as u64;
    }
}
