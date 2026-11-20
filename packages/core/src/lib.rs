//! EasyLauncher core: pack manifests, the file sync engine, and game adapters.
//!
//! The sync engine (`sync`) is game-agnostic: it only knows files and policies.
//! Game adapters (`minecraft`) resolve versions, runtimes and launch the game.

pub mod accounts;
pub mod download;
pub mod hash;
pub mod keys;
pub mod library;
pub mod manifest;
pub mod manual;
pub mod minecraft;
pub mod paths;
pub mod progress;
pub mod sync;

pub const USER_AGENT: &str = concat!("EasyLauncher/", env!("CARGO_PKG_VERSION"));
