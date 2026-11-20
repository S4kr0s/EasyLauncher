//! Minecraft adapter: version/loader resolution, Java runtimes, launch.

pub mod install;
pub mod java;
pub mod launch;
pub mod loaders;
pub mod servers;
pub mod version;

pub use install::{prepare, Prepared};
pub use launch::{LaunchAccount, LaunchOptions};
