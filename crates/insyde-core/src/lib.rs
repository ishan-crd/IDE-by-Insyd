//! InsyDE core. Everything here is UI-agnostic and safe to call from
//! background threads; the GPUI app owns scheduling.

pub mod agents;
pub mod brain;
pub mod fileops;
pub mod forge;
pub mod git;
pub mod project;
pub mod remote;
pub mod rpc;
pub mod settings;
pub mod store;
pub mod team;
pub mod terminal;
pub mod web;
