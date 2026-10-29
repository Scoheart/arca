//! Passive identity for one audited Crush artifact. No native-plugin observer.
//! Never executes Crush, shell configuration, helpers, MCPs or version probes.
use super::*;
use super::artifact_runtime::{Contract, version_for};
#[cfg(test)]
use super::artifact_runtime::{RuntimeSelection, verified_runtime};

fn contract() -> Option<&'static Contract> {
    static VALUE: OnceLock<Option<Contract>> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str(include_str!("../../../../data/native-runtime-crush.json")).ok()).as_ref()
}

pub(super) fn runtime_version(selected: &Path) -> Option<String> {
    version_for(contract()?, selected)
}

#[cfg(test)]
mod tests;
