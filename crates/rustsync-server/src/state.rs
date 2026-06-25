use std::sync::Arc;

use crate::{auth::ReplayCache, storage::Storage};

#[derive(Clone)]
pub struct AppState {
    storage: Arc<dyn Storage>,
    pub replay_cache: ReplayCache,
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
        }
    }

    pub fn storage(&self) -> &dyn Storage {
        self.storage.as_ref()
    }
}
