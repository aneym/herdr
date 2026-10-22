use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const REFRESH_AFTER: Duration = Duration::from_millis(500);
const PROBE_INLINE_AFTER: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProcessCwds {
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) foreground_cwd: Option<PathBuf>,
}

struct Sample {
    cwds: ProcessCwds,
    at: Instant,
}

#[derive(Default)]
pub(crate) struct CwdCache {
    sample: Arc<Mutex<Option<Sample>>>,
    refreshing: Arc<AtomicBool>,
}

type ProbeJob = Box<dyn FnOnce() + Send>;

fn probe_worker() -> Option<&'static mpsc::Sender<ProbeJob>> {
    static WORKER: OnceLock<Option<mpsc::Sender<ProbeJob>>> = OnceLock::new();
    WORKER
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel::<ProbeJob>();
            std::thread::Builder::new()
                .name("herdr-cwd-probe".into())
                .spawn(move || {
                    for job in rx {
                        job();
                    }
                })
                .ok()
                .map(|_| tx)
        })
        .as_ref()
}

impl CwdCache {
    pub(crate) fn get<P, B, F>(&self, probe_inline: P, background_probe: B) -> ProcessCwds
    where
        P: FnOnce() -> ProcessCwds,
        B: FnOnce() -> F,
        F: FnOnce() -> ProcessCwds + Send + 'static,
    {
        let cached = self.sample.lock().ok().and_then(|sample| {
            sample.as_ref().map(|sample| {
                (
                    sample.cwds.clone(),
                    Instant::now().saturating_duration_since(sample.at),
                )
            })
        });
        if let Some((cwds, age)) = cached {
            if age < PROBE_INLINE_AFTER {
                if age >= REFRESH_AFTER {
                    self.refresh_in_background(background_probe());
                }
                return cwds;
            }
        }
        let cwds = probe_inline();
        self.store(cwds.clone());
        cwds
    }

    fn store(&self, cwds: ProcessCwds) {
        if let Ok(mut sample) = self.sample.lock() {
            *sample = Some(Sample {
                cwds,
                at: Instant::now(),
            });
        }
    }

    fn refresh_in_background<F>(&self, probe: F)
    where
        F: FnOnce() -> ProcessCwds + Send + 'static,
    {
        if self.refreshing.swap(true, Ordering::AcqRel) {
            return;
        }
        let sample = Arc::clone(&self.sample);
        let refreshing = Arc::clone(&self.refreshing);
        let job: ProbeJob = Box::new(move || {
            let cwds = probe();
            if let Ok(mut sample) = sample.lock() {
                *sample = Some(Sample {
                    cwds,
                    at: Instant::now(),
                });
            }
            refreshing.store(false, Ordering::Release);
        });
        if probe_worker().is_some_and(|worker| worker.send(job).is_ok()) {
            return;
        }
        self.refreshing.store(false, Ordering::Release);
    }
}
