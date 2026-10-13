//! Every event that can change the model: an input key (already low-level from
//! crossterm), a tick, or the result of an async effect. `update` is the only
//! consumer.

use crossterm::event::KeyEvent;

use arca_core::application::agents::AgentInfo;
use arca_core::application::mcp::operations::InstalledMcp;
use arca_core::application::mcp::sources::SourceView;
use arca_core::domain::types::RegistryEntry;

pub enum Msg {
    /// First message: kick off the initial data load.
    Init,
    /// A key press from the input thread.
    Key(KeyEvent),
    /// Terminal resized — force a redraw (no state change).
    Redraw,
    /// Animation/idle tick (unused while nothing animates).
    Tick,
    /// Result of `Effect::LoadAll`: every cache, read from core.
    Loaded(Box<LoadedData>),
    /// Result of a mutation effect (install/enable/disable/delete): a human
    /// label plus success/error. Ok triggers a reload.
    Mutated {
        label: String,
        result: Result<(), String>,
    },
}

/// The four caches plus the set of user-overridden keys, loaded together.
pub struct LoadedData {
    pub registry: Vec<RegistryEntry>,
    pub custom_keys: Vec<String>,
    pub sources: Vec<SourceView>,
    pub agents: Vec<AgentInfo>,
    pub installed: Vec<InstalledMcp>,
}
