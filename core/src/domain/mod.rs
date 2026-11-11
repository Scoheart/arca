//! Stable business contracts for Arca.
//!
//! Domain types describe central assets and Agent relationships without owning
//! filesystem, Keychain, network, or frontend orchestration.

pub mod agents;
pub mod instructions;
pub mod assets;
pub mod error;
pub mod mcp;
pub mod hook;
pub mod skill;
pub mod subagent;
pub mod startup_agent;
pub mod types;

pub mod plugin;
