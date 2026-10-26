//! Shared domain and application core for Arca.
//!
//! Arca manages central MCP, Model, and Skill assets plus their desired and
//! observed relationships with Agents. Frontends should enter through
//! [`application`]; domain contracts live under [`domain`].

pub mod agents;
pub mod brand;
pub mod application;
pub mod assets;
pub mod domain;
pub mod network;
pub mod paths;
pub mod pinned_agents;
pub mod resources;
mod safe_write;
pub mod settings;
#[doc(hidden)]
pub mod testenv;
pub mod update;

/// Compatibility surface for integrations compiled against the original
/// `models` namespace. New code should use [`resources::model`] or
/// [`application::models`].
pub mod models {
    pub use crate::resources::model::*;
}

/// Compatibility surface for integrations compiled against the original
/// `skills` namespace. New code should use [`resources::skill`] or
/// [`application::skills`].
pub mod skills {
    pub use crate::resources::skill::*;
}

/// Compatibility aliases for the original MCP-shaped root API. New code should
/// use [`resources::mcp`] or [`application::mcp`].
pub use resources::mcp::{
    adapter, applier, codec, differ, disabled, effective, json_adapter, ops, r#override, registry,
    scanner, sources, toml_adapter, toml_list_adapter, yaml_adapter,
};

/// Compatibility surface for the original root value-object module. New code
/// should import these contracts from [`domain::types`].
pub mod types {
    pub use crate::domain::types::*;
}

/// Compatibility surface for integrations compiled against the original
/// `consumption` namespace. New code should use [`assets`] or
/// [`application::assets`].
pub mod consumption {
    pub use crate::assets::*;

    pub mod compatibility {
        pub use crate::assets::compatibility::*;
    }
    pub mod inventory {
        pub use crate::assets::inventory::*;
    }
    pub mod lifecycle {
        pub use crate::assets::lifecycle::*;
    }
    pub mod migration {
        pub use crate::assets::migration::*;
    }
    pub mod model_migration {
        pub use crate::assets::model_migration::*;
    }
    pub mod planner {
        pub use crate::assets::planner::*;
    }
    pub mod transaction {
        pub use crate::assets::transaction::*;
    }
    pub mod types {
        pub use crate::domain::assets::*;
    }
}

pub mod capture;
