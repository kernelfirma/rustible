# Feature: Implement Remote State Backends (S3, GCS, Azure, Consul)

## Problem Statement
Rustible currently only supports local state storage (`~/.rustible/state/`). For team collaboration and production use cases, remote state backends are essential to share state across team members and integrate with CI/CD pipelines.

## Current State
- Local state storage only
- No remote backend support
- No team collaboration capabilities
- No CI/CD integration for state

## Proposed Solution

### Phase 1: S3 Backend (v0.2.x)
1. **S3 implementation**
   ```rust
   // src/state/backends/s3.rs
   use aws_sdk_s3::{Client as S3Client, types::ByteStream};
   
   pub struct S3StateBackendConfig {
       pub bucket: String,
       pub key: String,
       pub region: String,
       pub encrypt: bool,
       pub kms_key_id: Option<String>,
       pub acl: Option<String>,
   }
   
   pub struct S3StateBackend {
       config: S3StateBackendConfig,
       client: S3Client,
   }
   
   #[async_trait]
   impl StateBackend for S3StateBackend {
       async fn read(&self) -> Result<StateManifest> {
           let req = self.client
               .get_object()
               .bucket(&self.config.bucket)
               .key(&self.config.key);
           
           let resp = req.send().await?;
           let body = resp.body.collect().await?.into_bytes();
           let manifest: StateManifest = serde_json::from_slice(&body)?;
           
           Ok(manifest)
       }
       
       async fn write(&self, state: &StateManifest) -> Result<()> {
           let data = serde_json::to_vec(state)?;
           
           let mut req = self.client
               .put_object()
               .bucket(&self.config.bucket)
               .key(&self.config.key)
               .body(ByteStream::from(data));
           
           if let Some(kms_key) = &self.config.kms_key_id {
               req = req.sse_kms_key_id(kms_key);
           }
           
           if let Some(acl) = &self.config.acl {
               req = req.acl(acl.parse()?);
           }
           
           req.send().await?;
           
           Ok(())
       }
   }
   ```

2. **Configuration**
   ```toml
   [state]
   backend = "s3"
   
   [state.s3]
   bucket = "my-rustible-state"
   key = "production/rustible.tfstate"
   region = "us-east-1"
   encrypt = true
   kms_key_id = "arn:aws:kms:us-east-1:123456789012:key/abcd1234"
   acl = "bucket-owner-full-control"
   ```

3. **Environment variables**
   ```bash
   export AWS_ACCESS_KEY_ID=...
   export AWS_SECRET_ACCESS_KEY=...
   export AWS_REGION=us-east-1
   
   rustible run playbook.yml -i inventory.yml
   ```

### Phase 2: GCS Backend (v0.2.x)
1. **GCS implementation**
   ```rust
   // src/state/backends/gcs.rs
   use google_cloud_storage::client::Client as GCSClient;
   
   pub struct GCSStateBackend {
       bucket: String,
       key: String,
       client: GCSClient,
   }
   
   #[async_trait]
   impl StateBackend for GCSStateBackend {
       async fn read(&self) -> Result<StateManifest> {
           let bytes = self.client
               .download_object(&self.bucket, &self.key)
               .await?;
           
           let manifest: StateManifest = serde_json::from_slice(&bytes)?;
           Ok(manifest)
       }
       
       async fn write(&self, state: &StateManifest) -> Result<()> {
           let data = serde_json::to_vec(state)?;
           
           self.client
               .upload_object(&self.bucket, &self.key, &data)
               .await?;
           
           Ok(())
       }
   }
   ```

2. **Configuration**
   ```toml
   [state]
   backend = "gcs"
   
   [state.gcs]
   bucket = "my-rustible-state"
   key = "production/rustible.tfstate"
   credentials = "/path/to/service-account.json"
   ```

### Phase 3: Azure Backend (v0.2.x)
1. **Azure Blob implementation**
   ```rust
   // src/state/backends/azure.rs
   use azure_storage_blobs::prelude::*;
   
   pub struct AzureStateBackend {
       container_name: String,
       blob_name: String,
       client: BlobClient,
   }
   
   #[async_trait]
   impl StateBackend for AzureStateBackend {
       async fn read(&self) -> Result<StateManifest> {
           let response = self.client
               .get()
               .await?
               .into_raw()
               .data
               .collect()
               .await?;
           
           let manifest: StateManifest = serde_json::from_slice(&response)?;
           Ok(manifest)
       }
       
       async fn write(&self, state: &StateManifest) -> Result<()> {
           let data = serde_json::to_vec(state)?;
           
           self.client
               .put_block_blob(data)
               .await?;
           
           Ok(())
       }
   }
   ```

2. **Configuration**
   ```toml
   [state]
   backend = "azure"
   
   [state.azure]
   container = "rustible-state"
   blob = "production/rustible.tfstate"
   account = "mystorageaccount"
   access_key = "..."
   ```

### Phase 4: Consul Backend (v0.3.x)
1. **Consul KV implementation**
   ```rust
   // src/state/backends/consul.rs
   use consul::{Client as ConsulClient};
   
   pub struct ConsulStateBackend {
       client: ConsulClient,
       key: String,
   }
   
   #[async_trait]
   impl StateBackend for ConsulStateBackend {
       async fn read(&self) -> Result<StateManifest> {
           let value = self.client
               .get(&self.key)
               .await?
               .ok_or(Error::StateNotFound)?;
           
           let manifest: StateManifest = serde_json::from_str(&value)?;
           Ok(manifest)
       }
       
       async fn write(&self, state: &StateManifest) -> Result<()> {
           let data = serde_json::to_string(state)?;
           
           self.client
               .put(&self.key, data)
               .await?;
           
           Ok(())
       }
   }
   ```

2. **Configuration**
   ```toml
   [state]
   backend = "consul"
   
   [state.consul]
   address = "localhost:8500"
   scheme = "http"
   token = "..."
   key = "rustible/state/production"
   ```

### Phase 5: Backend Features (v0.3.x)
1. **State versioning**
   - Keep multiple versions of state
   - Rollback to previous versions
   - List state history

2. **State encryption**
   - Client-side encryption
   - Server-side encryption (S3, Azure)
   - KMS integration

3. **State compression**
   - Compress state before upload
   - Reduce storage costs
   - Faster transfers

4. **State caching**
   - Cache remote state locally
   - Refresh on changes
   - Offline mode support

## Expected Outcomes
- Remote state backend support for major cloud providers
- Team collaboration capabilities
- CI/CD integration
- State versioning and rollback
- Secure state encryption

## Success Criteria
- [ ] S3 backend implemented and tested
- [ ] GCS backend implemented and tested
- [ ] Azure Blob backend implemented and tested
- [ ] Consul KV backend implemented and tested
- [ ] Backend configuration documented
- [ ] Environment variable support working
- [ ] State versioning implemented
- [ ] State encryption working
- [ ] State compression implemented
- [ ] State caching working
- [ ] CI/CD integration examples provided

## Implementation Details

### Backend Trait
```rust
#[async_trait]
pub trait StateBackend: Send + Sync {
    async fn read(&self) -> Result<StateManifest>;
    async fn write(&self, state: &StateManifest) -> Result<()>;
    async fn delete(&self) -> Result<()>;
    async fn exists(&self) -> Result<bool>;
    async fn list_versions(&self) -> Result<Vec<StateVersion>>;
    async fn get_version(&self, version_id: &str) -> Result<StateManifest>;
}
```

### CLI Integration
```bash
# Configure backend
rustible state init -backend=s3 -bucket=my-state -key=prod/state

# Pull remote state
rustible state pull

# Push local state
rustible state push

# List backends
rustible state backends list

# Show backend status
rustible state backends status
```

## Related Issues
- #008: State Management
- #010: State Locking
- #011: State Versioning

## Additional Notes
This is a **P1 (High)** feature that enables team collaboration. Should be targeted for v0.2.x release with v0.3.x advanced features.
