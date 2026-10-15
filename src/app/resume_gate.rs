use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ResumeRank {
    Pinned(usize),
    Visible,
    Background,
}

impl ResumeRank {
    pub(crate) fn pinned(self) -> bool {
        matches!(self, Self::Pinned(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ResumeGate {
    pub(crate) max_booting: usize,
    pub(crate) boot_window: Duration,
    pub(crate) load_per_core_limit: f64,
}

pub(crate) const RESUME_GATE_RECHECK: Duration = Duration::from_secs(2);

impl ResumeGate {
    pub(crate) fn from_config(
        config: &crate::config::SessionConfig,
        restores_session: bool,
    ) -> Self {
        Self {
            max_booting: config.startup_max_booting_agents as usize,
            boot_window: Duration::from_millis(config.startup_agent_boot_ms.into()),
            load_per_core_limit: if restores_session {
                config.startup_load_per_core_limit.max(0.0)
            } else {
                0.0
            },
        }
    }

    pub(crate) fn admits(
        &self,
        rank: ResumeRank,
        booting: usize,
        load_per_core: Option<f64>,
    ) -> bool {
        if self.max_booting != 0 && booting >= self.max_booting {
            return false;
        }
        if rank.pinned() || self.load_per_core_limit == 0.0 {
            return true;
        }
        !load_per_core.is_some_and(|load| load > self.load_per_core_limit)
    }
}

pub(crate) fn host_load_per_core() -> Option<f64> {
    #[cfg(unix)]
    {
        let mut loads = [0f64; 1];
        let read = unsafe { libc::getloadavg(loads.as_mut_ptr(), 1) };
        let cores = std::thread::available_parallelism().ok()?.get() as f64;
        (read == 1).then(|| loads[0] / cores)
    }
    #[cfg(not(unix))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_table() {
        let gate = |max_booting, limit| ResumeGate {
            max_booting,
            boot_window: Duration::from_secs(20),
            load_per_core_limit: limit,
        };
        let pinned = ResumeRank::Pinned(0);
        let rest = ResumeRank::Background;
        let cases = [
            (gate(4, 1.5), rest, 3, Some(0.5), true),
            (gate(4, 1.5), rest, 4, Some(0.5), false),
            (gate(4, 1.5), pinned, 4, Some(0.5), false),
            (gate(0, 1.5), rest, 500, Some(0.5), true),
            (gate(4, 1.5), rest, 0, Some(1.6), false),
            (gate(4, 1.5), ResumeRank::Visible, 0, Some(1.6), false),
            (gate(4, 1.5), pinned, 0, Some(16.0), true),
            (gate(4, 1.5), rest, 0, Some(1.5), true),
            (gate(4, 0.0), rest, 0, Some(16.0), true),
            (gate(4, 1.5), rest, 0, None, true),
        ];
        for (gate, rank, booting, load, admitted) in cases {
            assert_eq!(
                gate.admits(rank, booting, load),
                admitted,
                "{gate:?} {rank:?} booting={booting} load={load:?}"
            );
        }
        assert!(ResumeRank::Pinned(0) < ResumeRank::Pinned(3));
        assert!(ResumeRank::Pinned(9) < ResumeRank::Visible);
        assert!(ResumeRank::Visible < ResumeRank::Background);
    }
}
