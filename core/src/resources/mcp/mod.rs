//! MCP asset, catalog, observation, codec, and writer implementation.

pub mod adapter;
pub mod applier;
pub mod codec;
pub mod differ;
pub mod disabled;
pub mod effective;
pub mod json_adapter;
pub mod ops;
pub mod r#override;
pub mod registry;
pub mod scanner;
pub mod sources;
pub mod toml_adapter;
pub mod toml_list_adapter;
pub mod yaml_adapter;

pub(crate) mod builtin;
