# Provider and Registry Ecosystem

## Status
Draft

## Problem Statement
Rustible needs a provider SDK and registry model so cloud and platform modules can be distributed, versioned, and upgraded independently of the core.

## Goals
- Provide a stable provider SDK with a minimal, async API.
- Support versioned distribution via a registry (compatible with the existing registry design).
- Define compatibility and deprecation policy for providers.

## Provider SDK (Minimal)
Providers expose a manifest plus a dynamic module catalog.

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn metadata(&self) -> ProviderMetadata;
    fn modules(&self) -> Vec<ModuleDescriptor>;
    async fn invoke(&self, module: &str, params: ModuleParams, ctx: ModuleContext)
        -> Result<ModuleOutput, ProviderError>;
}
```

### Required Metadata
- `name`, `version`, `api_version`
- `supported_targets` (aws, azure, gcp, onprem, etc.)
- `capabilities` (read, create, update, delete)

## Packaging Model
- Providers ship as signed artifacts with a manifest and compiled binary.
- Manifest includes checksum, minimum Rustible core version, and SDK API version.
- Local installation is supported via `rustible provider install ./path`.

## Registry Model
- Reuse registry architecture (content-addressable artifacts + metadata index).
- Add a `provider` namespace for discovery.
- Support mirrors and offline caches.

## Compatibility Policy
- Core exposes `provider_api_version` with semantic versioning.
- Providers declare compatible ranges (e.g., `^1.2`).
- Deprecations require a two-release grace period.

## Next Steps
- Implement provider discovery and manifest validation.
- Extend registry metadata with provider artifacts.
- Ship a sample provider (aws-core) to validate packaging and SDK stability.
