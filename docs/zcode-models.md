# ZCode Desktop custom models

Arca manages custom Chat Completions models in `~/.zcode/v2/config.json`. This contract was audited against the installed desktop package on 2026-09-09: providers are under `provider`, connection fields under `options`, and models in a keyed `models` object. Built-in providers are excluded. Other protocols are not yet enabled by this adapter.

Create or import a central Model, then add it from the ZCode Agent page. Choose **Plaintext** credential delivery explicitly when a key is needed: ZCode stores its native `options.apiKey` in the private JSON file; the central credential remains in Keychain. Auto does not silently export credentials. Environment-variable substitution and native credential helpers are not verified for this desktop path and are not advertised.

Each central model receives a separate stable Arca provider. External imports only create central assets and preserve the original configuration. Add the imported asset from the ZCode page to create its new provider. Removing a model retains its provider and credentials. Reviewed clear-all removes only the custom Chat Completions models shown by discovery and retains providers, keys and builtins. Provider-level enabled state is observed; Arca does not control a global current model because selection belongs to ZCode conversations. Restart ZCode after applying configuration, then choose a model in its conversation UI.

All writes preserve unknown fields and use existing-file inode preservation, CAS and private encrypted rollback snapshots. Malformed or duplicate-key configurations fail closed. Synthetic adapter/discovery cases use temporary directories and invented values; no real ZCode configuration is modified during implementation.

Model registration does not grant API access or repair certificate trust. A server may list a model but reject inference with model_not_found; fix routing/access at the provider. TLS trust remains a separate ZCode setting.
