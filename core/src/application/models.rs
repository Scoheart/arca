//! Model Profile use cases.

pub use crate::domain::assets::ApiKeyDelivery;
pub use crate::domain::types::{ModelProfile, ModelProtocol};
pub use crate::resources::model::spark_websocket::{
    request as request_spark_websocket, SparkWebSocketModel,
};

pub mod qiniu_batch {
    pub use crate::resources::model::qiniu_batch::{download, status, submit, QiniuBatchJob, QiniuBatchRequest};
}

pub use crate::resources::model::{
    ModelApplyResult, ModelAgentView, ModelProfileView, ModelProviderInstanceView, ModelProviderView, ModelProviderPortalView,
    ProviderModelSummary,
};

pub fn list_profiles() -> Vec<ModelProfileView> {
    super::gate::read(crate::resources::model::list_profiles)
}

pub fn export_curl(profile_id: &str, include_api_key: bool) -> Result<String, String> {
    super::gate::read(|| crate::resources::model::curl::for_profile(profile_id, include_api_key))
}

pub fn provider_docs_url(provider: &str) -> Option<&'static str> {
    crate::resources::model::provider_docs_url(provider)
}

pub fn provider_portal(provider: &str) -> Option<&'static ModelProviderPortalView> {
    crate::resources::model::provider_portal(provider)
}

pub fn provider_documentation(id: &str) -> Result<Option<&'static str>, String> {
    super::gate::read(|| {
        let settings = crate::settings::load_settings_strict().map_err(|error| error.to_string())?;
        let provider = settings.model_providers.as_ref().and_then(|providers| providers.get(id));
        Ok(provider_docs_url(provider.map(|provider| provider.provider.as_str()).unwrap_or(id)))
    })
}

pub fn list_providers() -> &'static [ModelProviderView] {
    crate::resources::model::list_providers()
}

pub fn list_provider_instances() -> Vec<ModelProviderInstanceView> {
    super::gate::read(crate::resources::model::list_provider_instances)
}

pub fn discover_provider_models(provider_id: &str) -> Result<Vec<ProviderModelSummary>, String> {
    let started = std::time::Instant::now();
    let result = (|| {
        let input = super::gate::read(|| crate::resources::model::prepare_provider_discovery(provider_id))?;
        crate::resources::model::execute_provider_discovery(input)
    })();
    if let Err(ref error) = result {
        let mapped = super::error::from_legacy(error.clone());
        crate::logging::LogEvent::error("models.discover_provider_models", &mapped.message)
            .code(&mapped.code)
            .duration_ms(started.elapsed().as_millis() as u64)
            .field("provider_id", provider_id)
            .emit();
    }
    result
}

pub fn reveal_provider_credential(provider_id: &str) -> Result<String, String> {
    let result = (|| {
        let provider = super::gate::read(|| crate::resources::model::prepare_provider_credential_reveal(provider_id))?;
        crate::resources::model::execute_provider_credential_reveal(&provider)
    })();
    if let Err(ref error) = result {
        let mapped = super::error::from_legacy(error.clone());
        crate::logging::LogEvent::warn("models.reveal_provider_credential", &mapped.message)
            .code(&mapped.code)
            .field("provider_id", provider_id)
            .emit();
    }
    result
}

pub use crate::domain::credentials::{CredentialStoreBackend, CredentialStoreConfig};
pub use crate::resources::model::credential_service::CredentialRevisionView;

pub fn list_credential_revisions() -> Result<Vec<CredentialRevisionView>, String> {
    super::gate::read(crate::resources::model::credential_service::list_revisions)
}

pub fn retire_credential_revision(revision: &str) -> Result<(), String> {
    super::gate::write_for(super::gate::CapabilityDomain::Model, || {
        crate::resources::model::credential_service::retire_revision(revision)
    })
}

pub fn list_credential_stores() -> Result<Vec<CredentialStoreConfig>, String> {
    super::gate::read(crate::resources::model::credential_service::list_stores)
}

pub fn save_credential_store(store: CredentialStoreConfig) -> Result<(), String> {
    super::gate::write_for(super::gate::CapabilityDomain::Model, || {
        crate::resources::model::credential_service::save_store(store)
    })
}

pub fn check_credential_store(store_id: &str) -> Result<(), String> {
    crate::resources::model::credential_service::check_store(store_id)
}

pub fn delete_credential_store(store_id: &str) -> Result<(), String> {
    super::gate::write_for(super::gate::CapabilityDomain::Model, || {
        crate::resources::model::credential_service::delete_store(store_id)
    })
}

pub fn resolve_agent_credential(agent_id: &str, profile_id: &str) -> Result<zeroize::Zeroizing<Vec<u8>>, String> {
    crate::resources::model::resolve_agent_credential(agent_id, profile_id)
}

pub fn infer_provider(base_url: &str) -> String {
    crate::resources::model::infer_provider(base_url)
}

/// Return Model target capabilities for frontend presentation.
pub fn list_agent_capabilities() -> Result<Vec<ModelAgentView>, String> {
    Ok(super::gate::read(crate::resources::model::list_agents))
}

/// Prepare the same credential policy operation used by Desktop, without
/// committing it so noninteractive clients can review its bound write set.
pub fn plan_credential_delivery(
    agent_id: &str,
    profile_id: Option<&str>,
    delivery: ApiKeyDelivery,
    confirm_plaintext: bool,
) -> Result<crate::domain::assets::AssetOperationPlan, String> {
    super::gate::prepare_for(super::gate::CapabilityDomain::Model, "asset_plan", || {
        crate::assets::planner::plan_model_credential_delivery(
            agent_id, profile_id, delivery, confirm_plaintext,
        )
    })
}

pub fn set_credential_delivery(
    agent_id: &str,
    profile_id: &str,
    delivery: ApiKeyDelivery,
    confirm_plaintext: bool,
) -> Result<crate::resources::model::ModelApplyResult, String> {
    super::gate::write_for(super::gate::CapabilityDomain::Model, || {
        crate::resources::model::set_model_credential_delivery(
            agent_id,
            profile_id,
            delivery,
            confirm_plaintext,
        )
    })
}

pub fn set_agent_credential_delivery(
    agent_id: &str,
    delivery: ApiKeyDelivery,
    confirm_plaintext: bool,
) -> Result<crate::resources::model::ModelApplyResult, String> {
    super::gate::write_for(super::gate::CapabilityDomain::Model, || {
        crate::resources::model::set_agent_credential_delivery(
            agent_id,
            delivery,
            confirm_plaintext,
        )
    })
}

/// Follow native model selections through the normal model mutation gate.
/// This only updates Arca metadata; it never writes Agent configuration.
pub fn reconcile_native_selection() -> Result<(), String> {
    super::gate::write_for(
        super::gate::CapabilityDomain::Model,
        crate::resources::model::reconcile_active_models,
    )
}
