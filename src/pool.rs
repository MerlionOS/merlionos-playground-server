use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};
use tokio::time::{Duration, Instant};
use uuid::Uuid;
use tracing::{info, warn};

use crate::config::Config;
use crate::qemu::QemuInstance;

/// A session holding a QEMU instance.
pub struct Session {
    pub id: Uuid,
    pub instance: QemuInstance,
    pub created_at: Instant,
    pub last_activity: Instant,
}

/// Queue entry waiting for a slot.
struct QueueEntry {
    id: Uuid,
    notify: Arc<Notify>,
}

/// Manages a pool of QEMU instances with a queue.
pub struct Pool {
    config: Config,
    sessions: Mutex<Vec<Session>>,
    queue: Mutex<VecDeque<QueueEntry>>,
}

/// Status returned to the client.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PoolStatus {
    pub active: usize,
    pub max: usize,
    pub queue_length: usize,
    pub queue_position: Option<usize>,
}

impl Pool {
    pub fn new(config: Config) -> Arc<Self> {
        let pool = Arc::new(Self {
            config,
            sessions: Mutex::new(Vec::new()),
            queue: Mutex::new(VecDeque::new()),
        });

        // Spawn reaper task
        let pool_clone = pool.clone();
        tokio::spawn(async move {
            pool_clone.reaper_loop().await;
        });

        pool
    }

    /// Get pool status, optionally for a specific queue entry.
    pub async fn status(&self, queue_id: Option<Uuid>) -> PoolStatus {
        let sessions = self.sessions.lock().await;
        let queue = self.queue.lock().await;

        let position = queue_id.and_then(|id| {
            queue.iter().position(|e| e.id == id)
        });

        PoolStatus {
            active: sessions.len(),
            max: self.config.max_instances,
            queue_length: queue.len(),
            queue_position: position,
        }
    }

    /// Try to acquire a session. Returns immediately if a slot is available,
    /// otherwise queues and waits.
    pub async fn acquire(&self) -> Result<Uuid, String> {
        // Check if there's a free slot
        {
            let sessions = self.sessions.lock().await;
            if sessions.len() < self.config.max_instances {
                drop(sessions);
                return self.create_session().await;
            }
        }

        // Queue up and wait
        let entry_id = Uuid::new_v4();
        let notify = Arc::new(Notify::new());
        {
            let mut queue = self.queue.lock().await;
            let pos = queue.len() + 1;
            queue.push_back(QueueEntry {
                id: entry_id,
                notify: notify.clone(),
            });
            info!(id = %entry_id, position = pos, "Queued for session");
        }

        // Wait for notification (with timeout)
        let timeout = Duration::from_secs(self.config.session_timeout_secs);
        match tokio::time::timeout(timeout, notify.notified()).await {
            Ok(()) => self.create_session().await,
            Err(_) => {
                // Remove from queue on timeout
                let mut queue = self.queue.lock().await;
                queue.retain(|e| e.id != entry_id);
                Err("Queue timeout".to_string())
            }
        }
    }

    /// Create a new session with a QEMU instance.
    async fn create_session(&self) -> Result<Uuid, String> {
        let instance = QemuInstance::spawn(&self.config).await?;
        let session_id = Uuid::new_v4();
        let now = Instant::now();

        let session = Session {
            id: session_id,
            instance,
            created_at: now,
            last_activity: now,
        };

        let mut sessions = self.sessions.lock().await;
        sessions.push(session);
        info!(id = %session_id, active = sessions.len(), "Session created");
        Ok(session_id)
    }

    /// Get mutable access to a session by ID.
    pub async fn with_session<F, R>(&self, id: Uuid, f: F) -> Option<R>
    where
        F: FnOnce(&mut Session) -> R,
    {
        let mut sessions = self.sessions.lock().await;
        sessions.iter_mut().find(|s| s.id == id).map(f)
    }

    /// Mark a session as active (reset idle timer).
    pub async fn touch(&self, id: Uuid) {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.iter_mut().find(|s| s.id == id) {
            session.last_activity = Instant::now();
        }
    }

    /// Release a session and notify the next in queue.
    pub async fn release(&self, id: Uuid) {
        let mut sessions = self.sessions.lock().await;
        if let Some(pos) = sessions.iter().position(|s| s.id == id) {
            let mut session = sessions.remove(pos);
            session.instance.kill().await;
            info!(id = %id, active = sessions.len(), "Session released");
        }
        drop(sessions);

        // Notify next in queue
        let mut queue = self.queue.lock().await;
        if let Some(entry) = queue.pop_front() {
            info!(id = %entry.id, remaining = queue.len(), "Notifying queued client");
            entry.notify.notify_one();
        }
    }

    /// Periodically reap expired and idle sessions.
    async fn reaper_loop(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;

            let session_timeout = Duration::from_secs(self.config.session_timeout_secs);
            let idle_timeout = Duration::from_secs(self.config.idle_timeout_secs);
            let now = Instant::now();

            let mut to_remove = Vec::new();
            {
                let sessions = self.sessions.lock().await;
                for session in sessions.iter() {
                    let session_age = now - session.created_at;
                    let idle_age = now - session.last_activity;

                    if session_age > session_timeout {
                        warn!(id = %session.id, "Session expired (max time)");
                        to_remove.push(session.id);
                    } else if idle_age > idle_timeout {
                        warn!(id = %session.id, "Session expired (idle)");
                        to_remove.push(session.id);
                    }
                }
            }

            for id in to_remove {
                self.release(id).await;
            }
        }
    }

    /// Get remaining time for a session in seconds.
    pub async fn remaining_secs(&self, id: Uuid) -> Option<u64> {
        let sessions = self.sessions.lock().await;
        sessions.iter().find(|s| s.id == id).map(|s| {
            let elapsed = s.created_at.elapsed().as_secs();
            self.config.session_timeout_secs.saturating_sub(elapsed)
        })
    }
}
