//! Model Profile use cases.

pub use crate::domain::assets::ApiKeyDelivery;
pub use crate::domain::types::{ModelProfile, ModelProtocol};
pub use crate::resources::model::credential::CredentialValidationView;
pub use crate::resources::model::{
    ModelAgentView, ModelProfileView, ModelProviderInstanceView, ModelProviderView, ModelProviderPortalView,
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
    let input = super::gate::read(|| crate::resources::model::prepare_provider_discovery(provider_id))?;
    crate::resources::model::execute_provider_discovery(input)
}

pub fn reveal_provider_credential(provider_id: &str) -> Result<String, String> {
    let provider = super::gate::read(|| crate::resources::model::prepare_provider_credential_reveal(provider_id))?;
    crate::resources::model::execute_provider_credential_reveal(&provider)
}

pub fn infer_provider(base_url: &str) -> String {
    crate::resources::model::infer_provider(base_url)
}

pub fn validate_credential_source(
    source: &crate::domain::types::ApiKeySource,
) -> Result<CredentialValidationView, String> {
    super::gate::read(|| crate::resources::model::credential::validate_for_ui(source))
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
