//! Shared logic for the DLSSG-for-SM86 Manager.
//!
//! The crate is split so that the GUI (`DLSSG-Manager`) and a small console
//! tool (`dlssgm-cli`) use exactly the same code paths.

pub mod config;
pub mod gamedetect;
pub mod github;
pub mod i18n;
pub mod icons;
pub mod installer;
pub mod model;
pub mod packages;
pub mod pe;
pub mod scan;
pub mod system;
pub mod util;

pub use model::*;
