use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CredentialStoreConfig {
    pub id: String,
    pub name: String,
    pub backend: CredentialStoreBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CredentialStoreBackend {
    Keychain,
    OnePassword {
        command: String,
        vault_id: String,
        token_env: String,
    },
    Bitwarden {
        command: String,
        server_url: String,
        appdata_dir: String,
        user_id: String,
        folder_id: String,
        session_env: String,
    },
    Keeper {
        command: String,
        folder_uid: String,
        client_id: String,
        config_env: String,
    },
    #[serde(rename = "nordpass")]
    NordPass {
        command: String,
        account_email: String,
        folder_id: String,
    },
    ProtonPass {
        command: String,
        session_dir: String,
        user_id: String,
        share_id: String,
    },
    #[serde(rename = "keepassxc")]
    KeePassXc {
        command: String,
        database_path: String,
        password_env: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key_file: Option<String>,
    },
    Passbolt {
        command: String,
        server_url: String,
        user_id: String,
        private_key_path: String,
        password_env: String,
        server_fingerprint: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        folder_id: Option<String>,
    },
    #[serde(rename = "openbao")]
    OpenBao {
        server_url: String,
        mount: String,
        prefix: String,
        token_env: String,
    },
}

impl CredentialStoreBackend {
    pub(crate) fn authentication_env(&self) -> Option<&str> {
        match self {
            Self::Keychain | Self::NordPass { .. } | Self::ProtonPass { .. } => None,
            Self::OnePassword { token_env, .. } | Self::OpenBao { token_env, .. } => Some(token_env),
            Self::Bitwarden { session_env, .. } => Some(session_env),
            Self::Keeper { config_env, .. } => Some(config_env),
            Self::KeePassXc { password_env, .. } | Self::Passbolt { password_env, .. } => Some(password_env),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CredentialReference {
    pub store_id: String,
    pub locator: String,
    pub revision: String,
}
