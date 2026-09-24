//! Small versioned checkpoints; disk writes run off the input/render thread.
use super::*;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
};

#[derive(Serialize, Deserialize)]
struct Saved {
    version: u32,
    worlds: HashMap<String, World>,
}
struct Pending {
    next: Option<Vec<u8>>,
    closed: bool,
}
pub(super) struct Writer {
    shared: Arc<(Mutex<Pending>, Condvar)>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Writer {
    pub fn save(&self, worlds: &HashMap<String, World>) {
        #[derive(Serialize)]
        struct Borrowed<'a> {
            version: u32,
            worlds: &'a HashMap<String, World>,
        }
        if let Ok(bytes) = serde_json::to_vec(&Borrowed { version: 1, worlds }) {
            if let Ok(mut pending) = self.shared.0.lock() {
                pending.next = Some(bytes);
                self.shared.1.notify_one();
            }
        }
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        if let Ok(mut pending) = self.shared.0.lock() {
            pending.closed = true;
            self.shared.1.notify_one();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}
pub(super) fn open(path: Option<&Path>) -> (HashMap<String, World>, Option<Writer>) {
    let Some(path) = path else {
        return (HashMap::new(), None);
    };
    let path: PathBuf = path.with_extension("colours-v1.json");
    let mut worlds = std::fs::read(&path)
        .ok()
        .filter(|b| b.len() <= 8 * 1024 * 1024)
        .and_then(|b| serde_json::from_slice::<Saved>(&b).ok())
        .filter(|s| s.version == 1)
        .map(|s| s.worlds)
        .unwrap_or_default();
    for world in worlds.values_mut() {
        world.workspaces.retain(|_, w| w.restore());
    }
    let shared = Arc::new((
        Mutex::new(Pending {
            next: None,
            closed: false,
        }),
        Condvar::new(),
    ));
    let worker = shared.clone();
    let thread = std::thread::Builder::new()
        .name("colour-checkpoint".into())
        .spawn(move || loop {
            let Ok(mut pending) = worker.0.lock() else {
                return;
            };
            while pending.next.is_none() && !pending.closed {
                let Ok(next) = worker.1.wait(pending) else {
                    return;
                };
                pending = next;
            }
            let bytes = pending.next.take();
            let closed = pending.closed;
            drop(pending);
            if let Some(bytes) = bytes {
                if let Err(error) = write(&path, &bytes) {
                    tracing::warn!(%error,"colour checkpoint write failed");
                }
            }
            if closed {
                return;
            }
        })
        .ok();
    let writer = thread.map(|thread| Writer {
        shared,
        thread: Some(thread),
    });
    (worlds, writer)
}
