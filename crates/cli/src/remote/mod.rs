mod client;
mod config;
pub mod dispatch;
pub mod manage;

pub use config::{guard_local_only, resolve_remote};
