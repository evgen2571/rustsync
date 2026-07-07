use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{auth::ReplayCache, storage::Storage};

#[derive(Clone)]
pub struct AppState {
    storage: Arc<dyn Storage>,
    pub replay_cache: ReplayCache,
    access_event_lock: Arc<Mutex<()>>,
}

impl AppState {
    pub fn new<S>(storage: S) -> Self
    where
        S: Storage + 'static,
    {
        Self::from_storage(Arc::new(storage))
    }

    pub fn from_storage(storage: Arc<dyn Storage>) -> Self {
        Self {
            storage,
            replay_cache: ReplayCache::default(),
            access_event_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn storage(&self) -> &dyn Storage {
        self.storage.as_ref()
    }

    pub async fn lock_access_events(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.access_event_lock.lock().await
    }
}
