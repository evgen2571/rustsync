mod atomic;
mod fs;
mod paths;

pub use fs::FsStorage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutResult {
    Created,
    AlreadyExists,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadUpdateResult {
    Updated,
    Conflict,
}
