//! Shared filesystem-transfer implementation.
//!
//! Command adapters enter through `execute_copy` or `execute_move`. Source and destination
//! preflight, pinned-handle checks, staging, commit, and cross-volume fallback remain private to
//! this module so both commands preserve the same mutation invariants.

mod copy;
mod move_path;

pub(crate) use copy::execute_copy;
pub(crate) use move_path::execute_move;
