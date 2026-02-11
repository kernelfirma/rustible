//! State operations for resource address manipulation
//!
//! Provides `state mv` to rename resource addresses and `state replace-provider`
//! to change the provider recorded against every resource from one prefix to another.

use tracing::info;

use super::error::{ProvisioningError, ProvisioningResult};
use super::state::{ProvisioningState, ResourceId, ResourceState};

/// Move (rename) a resource address in state.
///
/// The resource keeps the same cloud_id, provider, config and attributes;
/// only its logical address changes.
pub fn state_mv(
    state: &mut ProvisioningState,
    source: &str,
    destination: &str,
) -> ProvisioningResult<()> {
    let src_id = ResourceId::from_address(source)
        .ok_or_else(|| ProvisioningError::ValidationError(format!("Invalid source address: {}", source)))?;
    let dst_id = ResourceId::from_address(destination)
        .ok_or_else(|| ProvisioningError::ValidationError(format!("Invalid destination address: {}", destination)))?;

    // Check source exists
    let resource = state.get_resource(&src_id)
        .ok_or_else(|| ProvisioningError::ResourceNotInState(source.to_string()))?
        .clone();

    // Check destination does not exist
    if state.get_resource(&dst_id).is_some() {
        return Err(ProvisioningError::ResourceExists(destination.to_string()));
    }

    // Remove old, insert new
    state.remove_resource(&src_id);

    let moved = ResourceState::new(
        dst_id.clone(),
        &resource.cloud_id,
        &resource.provider,
        resource.config.clone(),
        resource.attributes.clone(),
    );
    state.add_resource(moved);

    info!("Moved {} -> {}", source, destination);
    Ok(())
}

/// Replace provider recorded in state for all matching resources.
///
/// Every resource whose `provider` field equals `from_provider` will have it
/// changed to `to_provider`. This is the moral equivalent of Terraform's
/// `terraform state replace-provider`.
pub fn state_replace_provider(
    state: &mut ProvisioningState,
    from_provider: &str,
    to_provider: &str,
) -> ProvisioningResult<usize> {
    if from_provider == to_provider {
        return Err(ProvisioningError::ValidationError(
            "Source and destination providers are the same".to_string(),
        ));
    }

    let mut count = 0usize;
    let addresses: Vec<String> = state.resources.keys().cloned().collect();

    for address in addresses {
        if let Some(resource) = state.resources.get_mut(&address) {
            if resource.provider == from_provider {
                resource.provider = to_provider.to_string();
                count += 1;
            }
        }
    }

    if count == 0 {
        return Err(ProvisioningError::ProviderNotFound(from_provider.to_string()));
    }

    info!(
        "Replaced provider {} -> {} on {} resource(s)",
        from_provider, to_provider, count
    );
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::state::{ProvisioningState, ResourceId, ResourceState};

    fn sample_state() -> ProvisioningState {
        let mut state = ProvisioningState::new();
        state.add_resource(ResourceState::new(
            ResourceId::new("aws_vpc", "main"),
            "vpc-123",
            "aws",
            serde_json::json!({"cidr_block": "10.0.0.0/16"}),
            serde_json::json!({"arn": "arn:aws:ec2:us-east-1:123:vpc/vpc-123"}),
        ));
        state.add_resource(ResourceState::new(
            ResourceId::new("aws_subnet", "public"),
            "subnet-456",
            "aws",
            serde_json::json!({"cidr_block": "10.0.1.0/24"}),
            serde_json::json!({}),
        ));
        state
    }

    #[test]
    fn test_state_mv_success() {
        let mut state = sample_state();
        state_mv(&mut state, "aws_vpc.main", "aws_vpc.production").unwrap();
        assert!(state.get_resource(&ResourceId::new("aws_vpc", "main")).is_none());
        assert!(state.get_resource(&ResourceId::new("aws_vpc", "production")).is_some());
    }

    #[test]
    fn test_state_mv_source_missing() {
        let mut state = sample_state();
        let err = state_mv(&mut state, "aws_vpc.nonexistent", "aws_vpc.prod").unwrap_err();
        assert!(matches!(err, ProvisioningError::ResourceNotInState(_)));
    }

    #[test]
    fn test_state_mv_destination_exists() {
        let mut state = sample_state();
        let err = state_mv(&mut state, "aws_vpc.main", "aws_subnet.public").unwrap_err();
        assert!(matches!(err, ProvisioningError::ResourceExists(_)));
    }

    #[test]
    fn test_replace_provider_success() {
        let mut state = sample_state();
        let count = state_replace_provider(&mut state, "aws", "awscc").unwrap();
        assert_eq!(count, 2);
        for resource in state.resources.values() {
            assert_eq!(resource.provider, "awscc");
        }
    }

    #[test]
    fn test_replace_provider_no_match() {
        let mut state = sample_state();
        let err = state_replace_provider(&mut state, "gcp", "awscc").unwrap_err();
        assert!(matches!(err, ProvisioningError::ProviderNotFound(_)));
    }

    #[test]
    fn test_replace_provider_same() {
        let mut state = sample_state();
        let err = state_replace_provider(&mut state, "aws", "aws").unwrap_err();
        assert!(matches!(err, ProvisioningError::ValidationError(_)));
    }
}
