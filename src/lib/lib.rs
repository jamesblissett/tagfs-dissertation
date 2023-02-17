//! Library crate that contains the main functionality for tagfs.
pub mod db;
pub mod fs;
mod error;

#[cfg(feature = "autotag")]
pub mod autotag;
