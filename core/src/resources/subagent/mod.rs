//! Subagent resource management for Arca.

pub mod antigravity;
pub mod claude;
pub mod ops;

pub use ops::*;

#[cfg(test)]
mod tests;
