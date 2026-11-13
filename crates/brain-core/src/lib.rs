//! Shared logic for the Brain CLI and app: event protocol, store, accounts,
//! session discovery and the state reducer.

pub mod account;
pub mod account_setup;
pub mod agents;
pub mod brief;
pub mod changes;
pub mod commands;
pub mod conflicts;
pub mod digest;
pub mod event;
pub mod files;
pub mod folders;
pub mod garden;
pub mod github;
pub mod history;
pub mod later;
pub mod hook;
pub mod install;
pub mod log;
pub mod markdown;
pub mod process;
pub mod project;
pub mod qa;
pub mod requests;
pub mod review;
pub mod sessions;
pub mod state;
pub mod store;
pub mod styles;
pub mod sync;
pub mod tailscale;
pub mod templates;
pub mod transcript;
pub mod usage;
pub mod worktrees;
