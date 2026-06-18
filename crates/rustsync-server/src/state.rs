use crate::FsStorage;

#[derive(Clone)]
pub struct AppState {
    pub storage: FsStorage,
}

impl AppState {
    pub fn new(storage: FsStorage) -> Self {
        Self { storage }
    }
}
