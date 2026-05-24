use crate::storage::Storage;

#[derive(Clone)]
pub struct AppState {
    pub storage: Storage,
}

impl AppState {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }
}
