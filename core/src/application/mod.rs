//! Frontend-facing use cases.
//!
//! CLI, Tauri, and future frontends should depend on this layer instead of
//! composing storage, recovery, adapters, or domain engines themselves.

pub mod agents;
pub mod instructions;
pub mod agent_launch;
pub mod terminals;
pub mod assets;
pub mod bootstrap;
pub mod convergence;
mod error;
pub(crate) mod gate;
pub mod mcp;
pub mod models;
pub mod network;
pub mod operations;
pub mod skills;
pub mod ui;
pub mod update;
pub mod workspace;

pub use gate::{BackendStatus, CapabilityDomain};

/// Stable service facade for non-Rust consumers and future frontends.
pub struct ArcaCore;

impl ArcaCore {
    pub fn bootstrap() -> bootstrap::BootstrapReport {
        bootstrap::bootstrap()
    }

    pub fn snapshot() -> crate::domain::error::CoreResult<workspace::WorkspaceSnapshot> {
        workspace::snapshot()
    }

    pub fn backend_status() -> BackendStatus {
        gate::status()
    }

    /// Gate a frontend-specific host write that does not read or mutate MCP,
    /// Model, or Skill ownership (for example, a user-selected export path or
    /// the Desktop-bundled CLI symlink).
    pub fn independent_host_mutation<T>(
        stage: &'static str,
        operation: impl FnOnce() -> crate::domain::error::CoreResult<T>,
    ) -> crate::domain::error::CoreResult<T> {
        gate::mutate_independent_core(stage, operation)
    }

    pub fn plan(
        request: operations::PlanOperationRequest,
    ) -> crate::domain::error::CoreResult<operations::OperationPlan> {
        operations::plan(request)
    }

    pub fn commit(
        request: operations::CommitOperationRequest,
    ) -> crate::domain::error::CoreResult<operations::OperationCommitResult> {
        operations::commit(request)
    }

    pub fn cancel(
        request: operations::CancelOperationRequest,
    ) -> crate::domain::error::CoreResult<()> {
        operations::cancel(request)
    }
}

pub mod capture;
pub mod traces;
