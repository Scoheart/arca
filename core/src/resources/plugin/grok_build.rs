//! Passive identity for one official Grok Build image. No plugin observer.
//! Do not execute the CLI, bootstrap, hooks, modules or version commands.
use super::artifact_runtime::{self, Contract};
use std::{path::Path, sync::OnceLock};

fn contract() -> Option<&'static Contract> {
    static VALUE: OnceLock<Option<Contract>> = OnceLock::new();
    VALUE.get_or_init(|| {
        serde_json::from_str(include_str!("../../../../data/native-runtime-grok-build.json")).ok()
    }).as_ref()
}

pub(super) fn runtime_version(selected: &Path) -> Option<String> {
    artifact_runtime::version_for(contract()?, selected)
}

#[cfg(test)]
mod tests;
