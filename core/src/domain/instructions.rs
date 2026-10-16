//! Native Agent documents, not central assets or consumption relationships.
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct InstructionPathSource {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub default_path: Option<String>,
    pub overridden: bool,
    pub patterns: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct InstructionPathConfiguration {
    pub agent_id: String,
    pub revision: String,
    pub sources: Vec<InstructionPathSource>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstructionPathUpdate {
    pub id: String,
    /// Null restores native path resolution. No filesystem content is changed.
    pub path: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct InstructionReader { pub id: String, pub name: String }

#[derive(Clone, Serialize, Deserialize)]
pub struct InstructionFile {
    pub id: String,
    pub path: String,
    pub name: String,
    pub purpose: String,
    pub loading: String,
    pub bytes: u64,
    pub editable: bool,
    pub readers: Vec<InstructionReader>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct InstructionCreateTarget { pub path: String, pub initial_text: String }

#[derive(Clone, Serialize, Deserialize)]
pub struct InstructionInventory {
    pub agent_id: String,
    pub supported: bool,
    pub revision: String,
    pub files: Vec<InstructionFile>,
    pub create: Option<InstructionCreateTarget>,
    pub complete: bool,
    pub issues: Vec<String>,
    pub reload: String,
    pub native_notes: String,
    pub docs: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct InstructionSnapshot {
    pub file: InstructionFile,
    pub text: String,
    pub revision: String,
    pub reload: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct InstructionEditPlan {
    pub operation_id: String,
    pub candidate_hash: String,
    pub path: String,
    pub creating: bool,
}
