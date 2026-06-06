mod acl;
mod envelope;

pub use acl::{WorkspaceAcl, WorkspaceMember, WorkspaceRole};

pub(crate) use crate::error::{AccessError, AccessResult};
