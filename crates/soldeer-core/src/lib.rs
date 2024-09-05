//! Low-level library for interacting with Soldeer registries and files
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
