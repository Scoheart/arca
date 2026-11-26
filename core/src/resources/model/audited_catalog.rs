//! Hosted text APIs verified from providers' own documentation.

use super::{ModelProviderView, MODEL_PROVIDERS};
use crate::domain::types::ModelProtocol;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;
use url::Url;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AuditedProvider {
    id: &'static str,
    name: &'static str,
    category: &'static str,
    base_url: &'static str,
    protocol: ModelProtocol,
    pub(super) docs_url: &'static str,
    pub(super) portal_url: Option<&'static str>,
    #[serde(default)]
    pub(super) models: Vec<&'static str>,
    #[serde(default)]
    pub(super) live_models: bool,
}

impl AuditedProvider {
    pub(super) fn template(&self) -> ModelProviderView {
        ModelProviderView {
            id: self.id,
            name: self.name,
            category: self.category,
            default_base_url: Some(self.base_url),
            default_protocol: self.protocol.clone(),
        }
    }
}

pub(super) fn providers() -> &'static BTreeMap<&'static str, AuditedProvider> {
    static CATALOG: LazyLock<BTreeMap<&'static str, AuditedProvider>> = LazyLock::new(|| {
        let entries: Vec<AuditedProvider> =
            serde_json::from_str(include_str!("../../../../data/audited-model-providers.json"))
                .expect("audited model provider catalog must be valid");
        let mut ids = MODEL_PROVIDERS.iter().map(|provider| provider.id).collect::<BTreeSet<_>>();
        let mut urls = MODEL_PROVIDERS
            .iter()
            .flat_map(|provider| {
                provider.default_base_url.into_iter().chain(
                    super::provider_additional_endpoints(provider.id)
                        .iter()
                        .map(|endpoint| endpoint.base_url),
                )
            })
            .map(str::to_ascii_lowercase)
            .collect::<BTreeSet<_>>();
        entries.into_iter().map(|entry| {
            let id = entry.id;
            let url = Url::parse(entry.base_url).expect("invalid audited provider base URL");
            assert!(
                url.scheme() == "https" && url.host_str().is_some() && url.username().is_empty()
                    && url.password().is_none() && url.query().is_none() && url.fragment().is_none()
                    && !entry.base_url.ends_with('/'),
                "audited provider requires a credential-free HTTPS base URL: {id}"
            );
            assert_eq!(super::normalize_slug(id), id, "invalid audited provider ID");
            assert!(ids.insert(id), "duplicate audited provider ID: {id}");
            assert!(urls.insert(entry.base_url.to_ascii_lowercase()), "duplicate audited provider URL: {id}");
            assert!(matches!(entry.category, "official" | "gateway"), "invalid audited provider category: {id}");
            assert!(
                !entry.live_models || matches!(entry.protocol, ModelProtocol::OpenaiCompletions | ModelProtocol::OpenaiResponses),
                "live model listing requires an OpenAI-compatible endpoint: {id}"
            );
            assert!(entry.models.iter().all(|model| !model.is_empty()), "empty model ID: {id}");
            (id, entry)
        }).collect()
    });
    &CATALOG
}

pub(super) fn get(provider_id: &str) -> Option<&'static AuditedProvider> {
    providers().get(provider_id)
}
