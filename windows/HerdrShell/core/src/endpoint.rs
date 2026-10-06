//! Socket endpoints, named the way herdr `src/ipc.rs` names them.
//!
//! Unix: the socket path as a filesystem name (`GenericFilePath`).
//! Windows: the same path string as a namespaced pipe name (`GenericNamespaced`), so
//! `C:\...\herdr.sock` maps to the pipe herdr's server (or the studio relay) listens on.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use interprocess::local_socket::Stream as LocalStream;

/// Env var naming the JSON API socket; the client socket is derived from it.
pub const SOCKET_PATH_ENV_VAR: &str = "HERDR_SOCKET_PATH";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    #[cfg(unix)]
    UnixSocket(PathBuf),
    #[cfg(windows)]
    NamedPipe(PathBuf),
}

impl Endpoint {
    /// The platform endpoint for a herdr socket path.
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        #[cfg(unix)]
        {
            Endpoint::UnixSocket(path.into())
        }
        #[cfg(windows)]
        {
            Endpoint::NamedPipe(path.into())
        }
    }

    /// The API endpoint named by `HERDR_SOCKET_PATH`, if set.
    pub fn api_from_env() -> Option<Self> {
        std::env::var_os(SOCKET_PATH_ENV_VAR)
            .filter(|value| !value.is_empty())
            .map(Self::from_path)
    }

    pub fn path(&self) -> &Path {
        match self {
            #[cfg(unix)]
            Endpoint::UnixSocket(path) => path,
            #[cfg(windows)]
            Endpoint::NamedPipe(path) => path,
        }
    }

    /// The client-protocol endpoint beside an API endpoint: `herdr.sock` -> `herdr-client.sock`
    /// (herdr `derive_client_socket_from_api_socket`).
    pub fn client_for_api(&self) -> Self {
        let api = self.path();
        let stem = api
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("herdr");
        let parent = api.parent().unwrap_or_else(|| Path::new(""));
        Self::from_path(parent.join(format!("{stem}-client.sock")))
    }

    /// Opens a blocking stream to this endpoint.
    pub fn connect(&self) -> io::Result<LocalStream> {
        match self {
            #[cfg(unix)]
            Endpoint::UnixSocket(path) => {
                use interprocess::local_socket::{prelude::*, GenericFilePath};
                let name = path.as_path().to_fs_name::<GenericFilePath>()?;
                LocalStream::connect(name)
            }
            #[cfg(windows)]
            Endpoint::NamedPipe(path) => {
                use interprocess::local_socket::{prelude::*, GenericNamespaced};
                let name = path.to_string_lossy().to_string();
                let name = name.to_ns_name::<GenericNamespaced>()?;
                LocalStream::connect(name)
            }
        }
    }
}

/// Result of one non-blocking read attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadPoll {
    Data(usize),
    Pending,
    Closed,
}

/// Prepares a stream for [`poll_read`] and [`write_all_polled`] from a single thread.
///
/// Unix sockets switch to non-blocking mode. Windows synchronous pipes stay blocking:
/// a pending `ReadFile` would serialize behind it every `WriteFile` on the same handle,
/// so readers check `PeekNamedPipe` first (as herdr's own client does).
pub fn prepare_polled(stream: &mut LocalStream) -> io::Result<()> {
    #[cfg(unix)]
    {
        use interprocess::local_socket::traits::Stream as _;
        stream.set_nonblocking(true)
    }
    #[cfg(windows)]
    {
        let _ = stream;
        Ok(())
    }
}

/// Reads whatever is available without blocking.
pub fn poll_read(stream: &mut LocalStream, buf: &mut [u8]) -> io::Result<ReadPoll> {
    #[cfg(unix)]
    {
        match stream.read(buf) {
            Ok(0) => Ok(ReadPoll::Closed),
            Ok(read) => Ok(ReadPoll::Data(read)),
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(ReadPoll::Pending)
            }
            Err(err) if is_connection_closed_error(&err) => Ok(ReadPoll::Closed),
            Err(err) => Err(err),
        }
    }
    #[cfg(windows)]
    {
        match windows_pipe_available(stream)? {
            None => Ok(ReadPoll::Closed),
            Some(0) => Ok(ReadPoll::Pending),
            Some(available) => {
                let want = (available as usize).min(buf.len());
                match stream.read(&mut buf[..want]) {
                    Ok(0) => Ok(ReadPoll::Closed),
                    Ok(read) => Ok(ReadPoll::Data(read)),
                    Err(err) if is_connection_closed_error(&err) => Ok(ReadPoll::Closed),
                    Err(err) => Err(err),
                }
            }
        }
    }
}

/// Writes all bytes, retrying while a non-blocking socket is full.
pub fn write_all_polled(stream: &mut LocalStream, mut data: &[u8]) -> io::Result<()> {
    while !data.is_empty() {
        match stream.write(data) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(written) => data = &data[written..],
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(err) => return Err(err),
        }
    }
    loop {
        match stream.flush() {
            Ok(()) => return Ok(()),
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(err) => return Err(err),
        }
    }
}

pub fn is_connection_closed_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::NotConnected
            | io::ErrorKind::UnexpectedEof
            | io::ErrorKind::WriteZero
    ) || windows_pipe_closed_code(err)
}

#[cfg(windows)]
fn windows_pipe_closed_code(err: &io::Error) -> bool {
    // ERROR_INVALID_HANDLE, ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_NOT_CONNECTED (as herdr).
    matches!(err.raw_os_error(), Some(6 | 109 | 232 | 233))
}

#[cfg(not(windows))]
fn windows_pipe_closed_code(_err: &io::Error) -> bool {
    false
}

#[cfg(windows)]
fn windows_pipe_available(stream: &mut LocalStream) -> io::Result<Option<u32>> {
    use std::os::windows::io::{AsHandle, AsRawHandle};

    let LocalStream::NamedPipe(pipe) = stream;
    let mut available = 0u32;
    // SAFETY: the handle is owned by `pipe` for the duration of the call, and the only
    // out-pointer is a valid local; all buffer pointers are null with size 0.
    let ok = unsafe {
        windows_sys::Win32::System::Pipes::PeekNamedPipe(
            pipe.as_handle().as_raw_handle() as _,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut available,
            std::ptr::null_mut(),
        )
    };
    if ok != 0 {
        return Ok(Some(available));
    }
    let err = io::Error::last_os_error();
    if is_connection_closed_error(&err) {
        return Ok(None);
    }
    Err(err)
}
