//! Library crate that contains the main functionality for tagfs.
pub mod db;
pub mod fs;

#[cfg(feature = "autotag")]
pub mod autotag;
