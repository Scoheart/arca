use super::{credential_store, ModelProfile};
use crate::domain::credentials::{CredentialReference, CredentialStoreConfig};
use crate::domain::types::{AuthRequirement, ModelProviderConfig};
use crate::settings::{load_settings_strict, mutate_settings_checked, Settings};
use std::io::Error;
use zeroize::Zeroizing;

pub(crate) fn store(settings: &Settings, id: &str) -> Result<CredentialStoreConfig, String> {
    if id == "keychain" {
        return Ok(credential_store::keychain_store());
    }
    settings.credential_stores.as_ref()
        .and_then(|stores| stores.get(id))
        .cloned()
        .ok_or_else(|| "credential_store_missing: select an existing credential store".into())
}

pub(crate) fn list_stores() -> Result<Vec<CredentialStoreConfig>, String> {
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let mut stores = vec![credential_store::keychain_store()];
    stores.extend(settings.credential_stores.into_iter().flat_map(|stores| stores.into_values()));
    Ok(stores)
}

pub(crate) fn save_store(config: CredentialStoreConfig) -> Result<(), String> {
    credential_store::validate(&config)?;
    if config.id == "keychain"
        || matches!(&config.backend, crate::domain::credentials::CredentialStoreBackend::Keychain)
    {
        return Err("credential_store_builtin: macOS Keychain is built in".into());
    }
    mutate_settings_checked(|settings| {
        let entries = settings.credential_stores.get_or_insert_default();
        if entries.get(&config.id).is_some_and(|existing| existing.backend != config.backend) {
            return Err(Error::other("credential_store_identity_change: use a new connection ID for a different credential store"));
        }
        entries.insert(config.id.clone(), config);
        Ok(())
    }).map_err(|error| error.to_string())
}

pub(crate) fn check_store(id: &str) -> Result<(), String> {
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    credential_store::check(&store(&settings, id)?)
}

pub(crate) fn delete_store(id: &str) -> Result<(), String> {
    if id == "keychain" {
        return Err("credential_store_builtin: macOS Keychain is built in".into());
    }
    mutate_settings_checked(|settings| {
        if settings.model_providers.iter().flat_map(|providers| providers.values())
            .any(|provider| provider.credential.as_ref().is_some_and(|reference| reference.store_id == id))
        {
            return Err(Error::other("credential_store_in_use: switch Providers to another store first"));
        }
        let removed = settings.credential_stores.as_mut().and_then(|stores| stores.remove(id));
        if removed.is_none() {
            return Err(Error::other("credential_store_missing: connection no longer exists"));
        }
        Ok(())
    }).map_err(|error| error.to_string())
}

pub(crate) fn provider_for_profile<'a>(settings: &'a Settings, profile: &ModelProfile) -> Option<&'a ModelProviderConfig> {
    let id = profile.provider_id.as_deref()?;
    settings.model_providers.as_ref()?.get(id)
}

pub(crate) fn read_reference(settings: &Settings, reference: &CredentialReference) -> Result<Zeroizing<Vec<u8>>, String> {
    credential_store::read(&store(settings, &reference.store_id)?, reference)
}

pub(crate) fn read_provider(
    settings: &Settings,
    provider: &ModelProviderConfig,
) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
    if provider.auth_requirement == AuthRequirement::None {
        return Ok(None);
    }
    match &provider.credential {
        Some(reference) => read_reference(settings, reference).map(Some),
        None if provider.auth_requirement == AuthRequirement::Required => {
            Err("credential_missing: save an API Key for this Provider".into())
        }
        None => Ok(None),
    }
}

pub(crate) fn read_profile(settings: &Settings, profile: &ModelProfile) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
    let provider = provider_for_profile(settings, profile)
        .ok_or_else(|| "model_provider_not_found: the Model has no registered Provider".to_string())?;
    read_provider(settings, provider)
}

pub(crate) fn create_revision(
    config: &CredentialStoreConfig,
    operation_id: &str,
    provider_id: &str,
    key: &[u8],
) -> Result<CredentialReference, String> {
    credential_store::create_revision(config, operation_id, provider_id, key)
}

#[derive(serde::Serialize)]
pub struct CredentialRevisionView {
    pub provider_id: String,
    pub store_id: String,
    pub revision: String,
    pub active: bool,
}

fn active_revision(settings: &Settings, revision: &str) -> bool {
    settings.model_providers.iter().flat_map(|providers| providers.values())
        .any(|provider| provider.credential.as_ref().is_some_and(|reference| reference.revision == revision))
}

pub(crate) fn list_revisions() -> Result<Vec<CredentialRevisionView>, String> {
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    Ok(credential_store::list_owned_revisions()?.into_iter().map(|(provider_id, reference)| {
        let active = active_revision(&settings, &reference.revision);
        CredentialRevisionView { provider_id, store_id: reference.store_id, revision: reference.revision, active }
    }).collect())
}

pub(crate) fn retire_revision(revision: &str) -> Result<(), String> {
    let _guard = crate::safe_write::acquire_settings_lock(&crate::paths::settings_file())?;
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let (_, reference) = credential_store::owned_revision(revision)?
        .ok_or("credential_revision_missing: there is no owned credential revision with that ID")?;
    if active_revision(&settings, revision) || super::delivery_state::uses_revision(revision)? {
        return Err("credential_revision_in_use: a Provider or Agent still uses this credential version".into());
    }
    if crate::assets::planner::operation_root(revision).join("plan.json").exists() {
        return Err("credential_revision_pending: a reviewed operation is still associated with this revision".into());
    }
    credential_store::retire(&store(&settings, &reference.store_id)?, &reference)
}
