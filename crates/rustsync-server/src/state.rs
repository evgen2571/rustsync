use crate::{FsStorage, auth::ReplayCache};

#[derive(Clone)]
pub struct AppState {
    pub storage: FsStorage,
    pub replay_cache: ReplayCache,
}

impl AppState {
    pub fn new(storage: FsStorage) -> Self {
        Self {
            storage,
            replay_cache: ReplayCache::default(),
        }
    }
}
