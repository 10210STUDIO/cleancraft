//! CleanCraft engine: everything the app, the CLI and the MCP server do, without UI.
//!
//! Safety model, in order: [`env`] decides where things are (a temporary fake Mac in tests),
//! [`policy`] is the fence (allow-lists, protected locations), [`plan`] is the preview the user
//! confirms, [`executor`] moves confirmed items to the Trash or quarantine and logs each move
//! with an undo.

pub mod env;
pub mod executor;
pub mod features;
pub mod fsutil;
pub mod permissions;
pub mod plan;
pub mod plans;
pub mod policy;
pub mod settings;
pub mod sys;
pub mod testkit;
pub mod testkit_cleanup;

pub use env::{Env, Fault};
pub use executor::{ActionKind, ActionRecord, Method, Report, SkipReason};
pub use plan::{FeatureId, Item, Plan, Risk};
pub use settings::Settings;
