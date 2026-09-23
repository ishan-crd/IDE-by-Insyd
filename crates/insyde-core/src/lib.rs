//! InsyDE core. Everything here is UI-agnostic and safe to call from
//! background threads; the GPUI app owns scheduling.

pub mod forge;
pub mod git;
pub mod project;
pub mod store;
