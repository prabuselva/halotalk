//! HaloTalk domain core: data model, database bootstrap, and the [`store::Store`]
//! persistence layer. No HTTP concerns live here.

pub mod crypto;
pub mod db;
pub mod models;
pub mod rag;
pub mod store;

pub use anyhow::Result;
