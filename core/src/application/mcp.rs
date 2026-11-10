//! MCP-specific asset capabilities behind the shared application boundary.

pub use crate::domain::mcp::{McpConfigurationView, McpInspectionRequest, McpInspectionSource};

/// Read a single MCP independently of ownership, model credentials and recovery.
pub fn inspect_configuration(request: McpInspectionRequest) -> crate::domain::error::CoreResult<McpConfigurationView> {
    crate::resources::mcp::inspection::inspect(request)
}

pub mod catalog {
    pub use crate::resources::mcp::registry::{CatalogItem, RegistrySnapshot};

    pub fn read_registry_snapshot() -> RegistrySnapshot {
        super::super::gate::read(crate::resources::mcp::registry::read_registry_snapshot)
    }

    pub fn read_registry() -> Vec<crate::domain::types::RegistryEntry> {
        super::super::gate::read(crate::resources::mcp::registry::read_registry)
    }

    pub fn read_registry_all() -> Vec<CatalogItem> {
        super::super::gate::read(crate::resources::mcp::registry::read_registry_all)
    }

    pub fn user_override_keys() -> Vec<String> {
        super::super::gate::read(crate::resources::mcp::registry::user_override_keys)
    }
}

pub mod operations {
    pub use crate::resources::mcp::effective::effective_config;
    pub use crate::resources::mcp::ops::{InstalledMcp, ResyncOutcome};

    pub fn export_effective() -> Result<String, String> {
        super::super::gate::read(crate::resources::mcp::ops::export_effective)
    }

    pub fn parse_pasted_entries(
        text: &str,
    ) -> Result<Vec<crate::domain::types::RegistryEntry>, String> {
        crate::resources::mcp::ops::parse_pasted_entries(text)
    }

    pub fn resolve_entry(
        server_name: &str,
        transport: &str,
    ) -> Result<crate::domain::types::RegistryEntry, String> {
        super::super::gate::read(|| {
            crate::resources::mcp::ops::resolve_entry(server_name, transport)
        })
    }

    pub fn scan_installed(project_dir: Option<&str>) -> Vec<InstalledMcp> {
        super::super::gate::read(|| crate::resources::mcp::ops::scan_installed(project_dir))
    }

    pub fn target_file(
        agent: &crate::domain::types::AgentDefinition,
        scope: &str,
        project_dir: Option<&str>,
    ) -> Option<std::path::PathBuf> {
        crate::resources::mcp::ops::target_file(agent, scope, project_dir)
    }
}

pub mod scanning {
    pub use crate::resources::mcp::scanner::ScannedMcp;

    pub fn scan_agents(
        agents: &std::collections::BTreeMap<String, crate::domain::types::AgentDefinition>,
        project_dir: Option<&std::path::Path>,
        include_disabled: bool,
    ) -> Vec<ScannedMcp> {
        super::super::gate::read(|| {
            crate::resources::mcp::scanner::scan_agents(agents, project_dir, include_disabled)
        })
    }
}

pub mod sources {
    use crate::application::gate::CapabilityDomain;
    pub use crate::resources::mcp::sources::SourceView;

    pub fn list_views() -> Vec<SourceView> {
        super::super::gate::read(crate::resources::mcp::sources::list_views)
    }

    pub fn subscribe(url: String, name: Option<String>) -> Result<SourceView, String> {
        // Check readiness briefly; never hold the application gate during HTTP.
        super::super::gate::write_for(CapabilityDomain::Mcp, || Ok::<(), String>(()))?;
        let prepared = crate::resources::mcp::sources::probe_subscription(url, name)?;
        super::super::gate::write_for(CapabilityDomain::Mcp, || {
            crate::resources::mcp::sources::commit_subscription(prepared)
        })
    }

    pub fn add_local(path: String, name: Option<String>) -> Result<SourceView, String> {
        super::super::gate::write_for(CapabilityDomain::Mcp, || {
            crate::resources::mcp::sources::add_local(path, name)
        })
    }

    pub fn add_official() -> Result<SourceView, String> {
        super::super::gate::write_for(
            CapabilityDomain::Mcp,
            crate::resources::mcp::sources::add_official,
        )
    }

    pub fn refresh(id: String) -> Result<SourceView, String> {
        let prepared = super::super::gate::write_for(CapabilityDomain::Mcp, || {
            crate::resources::mcp::sources::prepare_refresh(id)
        })?;
        let probed = crate::resources::mcp::sources::probe_refresh(prepared);
        super::super::gate::write_for(CapabilityDomain::Mcp, || {
            crate::resources::mcp::sources::reconcile_refresh(probed)
        })
    }

    pub fn set_enabled(id: String, enabled: bool) -> Result<(), String> {
        super::super::gate::write_for(CapabilityDomain::Mcp, || {
            crate::resources::mcp::sources::set_enabled(id, enabled)
        })
    }

    pub fn remove(id: String) -> Result<(), String> {
        super::super::gate::write_for(CapabilityDomain::Mcp, || {
            crate::resources::mcp::sources::remove(id)
        })
    }
}

pub use crate::domain::types::{
    transport_of, HttpConfig, McpConfig, RegistryConfig, RegistryEntry, RegistryOrigin, SourceDef,
    StdioConfig,
};
