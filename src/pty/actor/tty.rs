//! Per-PTY foreground observation and restoration of that PTY's own modes.
use std::{
    os::fd::RawFd,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy)]
struct Observation {
    pgid: u32,
    stopped: bool,
    modes: libc::termios,
}

#[derive(Default)]
pub(super) struct TtySafety {
    checked: Option<Instant>,
    observation: Option<Observation>,
    capture: Option<(u32, libc::termios, u64)>,
    pub dropped_mouse_reports: u64,
    pub dropped_input_bytes: u64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TtyStatus {
    pub pgid: Option<u32>,
    pub stopped: Option<bool>,
    pub canonical: Option<bool>,
    pub held_input_bytes: usize,
    pub dropped_mouse_reports: u64,
    pub dropped_input_bytes: u64,
    pub last_good_termios_at: Option<u64>,
}

impl TtySafety {
    pub fn refresh(&mut self, fd: RawFd) {
        if self
            .checked
            .is_some_and(|at| at.elapsed() < Duration::from_millis(250))
        {
            return;
        }
        self.checked = Some(Instant::now());
        self.observation = (|| {
            let pgid = unsafe { libc::tcgetpgrp(fd) };
            if pgid <= 0 {
                return None;
            }
            let mut modes = unsafe { std::mem::zeroed() };
            if unsafe { libc::tcgetattr(fd, &mut modes) } != 0 {
                return None;
            }
            Some(Observation {
                pgid: pgid as u32,
                stopped: crate::platform::process_is_stopped(pgid as u32)?,
                modes,
            })
        })();
        if let Some(obs) = self.observation {
            if self
                .capture
                .as_ref()
                .is_some_and(|(pgid, _, _)| *pgid != obs.pgid)
            {
                self.capture = None;
            }
            if !obs.stopped && obs.modes.c_lflag & libc::ICANON == 0 {
                // Retain the timestamp when unchanged, rather than making a capture look new on every poll.
                if !self.capture.as_ref().is_some_and(|(pgid, modes, _)| {
                    *pgid == obs.pgid && changed_fields(modes, &obs.modes).is_empty()
                }) {
                    let at = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    self.capture = Some((obs.pgid, obs.modes, at));
                }
            }
        }
    }
    pub fn stopped(&self) -> bool {
        self.observation.is_some_and(|obs| obs.stopped)
    }
    pub fn block_mouse(&self) -> bool {
        self.observation
            .is_some_and(|obs| obs.stopped || obs.modes.c_lflag & libc::ICANON != 0)
    }
    pub fn status(&self, held_input_bytes: usize) -> TtyStatus {
        TtyStatus {
            pgid: self.observation.map(|obs| obs.pgid),
            stopped: self.observation.map(|obs| obs.stopped),
            canonical: self
                .observation
                .map(|obs| obs.modes.c_lflag & libc::ICANON != 0),
            held_input_bytes,
            dropped_mouse_reports: self.dropped_mouse_reports,
            dropped_input_bytes: self.dropped_input_bytes,
            last_good_termios_at: self.capture.as_ref().map(|(_, _, at)| *at),
        }
    }
    pub fn repair(
        &mut self,
        fd: RawFd,
        dry_run: bool,
    ) -> std::io::Result<crate::api::schema::PaneTtyRepairResult> {
        let (pgid, modes, at) = self
            .capture
            .as_ref()
            .ok_or_else(|| std::io::Error::other("no captured termios for this pane"))?;
        let current_pgid = unsafe { libc::tcgetpgrp(fd) };
        if current_pgid < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if current_pgid as u32 != *pgid {
            return Err(std::io::Error::other(
                "foreground pgid changed since termios capture",
            ));
        }
        let mut current = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(fd, &mut current) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let changed_fields = changed_fields(&current, modes);
        let applied = !dry_run && !changed_fields.is_empty();
        if applied && unsafe { libc::tcsetattr(fd, libc::TCSANOW, modes) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let result = crate::api::schema::PaneTtyRepairResult {
            pane: String::new(),
            pgid: *pgid,
            captured_at: *at,
            was_canonical: current.c_lflag & libc::ICANON != 0,
            changed_fields,
            applied,
        };
        self.checked = None;
        Ok(result)
    }
}

pub(super) fn changed_fields(a: &libc::termios, b: &libc::termios) -> Vec<String> {
    let mut fields = Vec::new();
    for (name, differs) in [
        ("iflag", a.c_iflag != b.c_iflag),
        ("oflag", a.c_oflag != b.c_oflag),
        ("cflag", a.c_cflag != b.c_cflag),
        ("lflag", a.c_lflag != b.c_lflag),
        ("cc", a.c_cc != b.c_cc),
        ("ispeed", unsafe {
            libc::cfgetispeed(a) != libc::cfgetispeed(b)
        }),
        ("ospeed", unsafe {
            libc::cfgetospeed(a) != libc::cfgetospeed(b)
        }),
    ] {
        if differs {
            fields.push(name.to_owned());
        }
    }
    fields
}
