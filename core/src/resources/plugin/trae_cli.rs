//! Release-artifact identity for one audited TRAE CLI image.
//! No CLI, sidecar, plugin, hook or version probe is executed.
use super::artifact_runtime::{self, Contract};
use std::{path::Path, sync::OnceLock};

fn contract() -> Option<&'static Contract> {
    static VALUE: OnceLock<Option<Contract>> = OnceLock::new();
    VALUE.get_or_init(|| {
        serde_json::from_str(include_str!("../../../../data/native-runtime-trae-cli.json")).ok()
    }).as_ref()
}

pub(super) fn runtime_version(selected: &Path) -> Option<String> {
    // Shared Core verification uses a fresh, finite 512 MiB digest allowance.
    // This labels only the selected main image, independently of its sidecar.
    artifact_runtime::version_for(contract()?, selected)
}

#[cfg(test)]
mod tests;
