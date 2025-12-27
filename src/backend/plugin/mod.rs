//! Plugin loading and registry infrastructure.
//!
//! This module provides functionality to load external plugins that implement
//! the QIDO-RS, WADO-RS, and/or STOW-RS services.

mod adapters;
mod registry;

pub use adapters::{PluginQidoAdapter, PluginStowAdapter, PluginWadoAdapter};
pub use registry::{LoadedPlugin, PluginLoadError, PluginRegistry};
