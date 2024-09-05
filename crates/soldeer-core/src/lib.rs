//! Soldeer is a package manager for Solidity projects
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
pub use errors::SoldeerError;

pub type Result<T> = std::result::Result<T, SoldeerError>;

pub mod auth;
pub mod config;
pub mod download;
pub mod errors;
pub mod install;
pub mod lock;
pub mod push;
pub mod registry;
pub mod remappings;
pub mod update;
pub mod utils;
