//! Provider SDK Versioning Policy and Compatibility Tests
//!
//! This test suite validates the provider SDK versioning policy and ensures
//! compatibility between providers and the core system.
//!
//! ## What We're Testing
//!
//! 1. **Semver Compliance**: Version parsing and comparison
//! 2. **API Version Compatibility**: Core/provider version negotiation
//! 3. **Version Requirements**: Dependency version constraints
//! 4. **Backward Compatibility**: Older providers with newer core
//! 5. **Forward Compatibility**: Newer providers with older core (limited)
//! 6. **Version Metadata**: Pre-release and build metadata handling
//! 7. **Breaking Changes**: Major version bump requirements
//! 8. **Minor Updates**: Feature additions without breakage
//! 9. **Patch Updates**: Bug fixes without API changes

use rustible::plugins::provider::{
    ProviderCapability, ProviderDependency, ProviderError, ProviderIndexEntry, ProviderMetadata,
};
use semver::{Version, VersionReq};
use std::collections::HashMap;

// ============================================================================
// Semver Compliance Tests
// ============================================================================

mod semver_compliance_tests {
    use super::*;

    #[test]
    fn test_version_parsing_basic() {
        let v = Version::new(1, 2, 3);
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
    }

    #[test]
    fn test_version_parsing_from_string() {
        let v: Version = "2.1.0".parse().unwrap();
        assert_eq!(v, Version::new(2, 1, 0));
    }

    #[test]
    fn test_version_parsing_with_prerelease() {
        let v: Version = "1.0.0-alpha.1".parse().unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 0);
        assert_eq!(v.patch, 0);
        assert!(!v.pre.is_empty());
    }

    #[test]
    fn test_version_parsing_with_build_metadata() {
        let v: Version = "1.0.0+build.123".parse().unwrap();
        assert_eq!(v.major, 1);
        assert!(!v.build.is_empty());
    }

    #[test]
    fn test_version_comparison() {
        let v1 = Version::new(1, 0, 0);
        let v2 = Version::new(2, 0, 0);
        let v3 = Version::new(1, 1, 0);
        let v4 = Version::new(1, 0, 1);

        assert!(v1 < v2);
        assert!(v1 < v3);
        assert!(v1 < v4);
        assert!(v3 < v2);
    }

    #[test]
    fn test_version_equality() {
        let v1 = Version::new(1, 2, 3);
        let v2 = Version::new(1, 2, 3);
        assert_eq!(v1, v2);
    }

    #[test]
    fn test_version_display() {
        let v = Version::new(1, 2, 3);
        assert_eq!(v.to_string(), "1.2.3");
    }

    #[test]
    fn test_prerelease_ordering() {
        let stable: Version = "1.0.0".parse().unwrap();
        let alpha: Version = "1.0.0-alpha".parse().unwrap();
        let beta: Version = "1.0.0-beta".parse().unwrap();

        // Pre-release versions are less than stable
        assert!(alpha < stable);
        assert!(beta < stable);
        assert!(alpha < beta);
    }
}

// ============================================================================
// Version Requirement Tests
// ============================================================================

mod version_requirement_tests {
    use super::*;

    #[test]
    fn test_exact_version_requirement() {
        let req: VersionReq = "=1.0.0".parse().unwrap();
        assert!(req.matches(&Version::new(1, 0, 0)));
        assert!(!req.matches(&Version::new(1, 0, 1)));
        assert!(!req.matches(&Version::new(1, 1, 0)));
    }

    #[test]
    fn test_caret_requirement() {
        // ^1.2.3 := >=1.2.3, <2.0.0
        let req: VersionReq = "^1.2.3".parse().unwrap();
        assert!(req.matches(&Version::new(1, 2, 3)));
        assert!(req.matches(&Version::new(1, 2, 4)));
        assert!(req.matches(&Version::new(1, 9, 0)));
        assert!(!req.matches(&Version::new(2, 0, 0)));
        assert!(!req.matches(&Version::new(1, 2, 2)));
    }

    #[test]
    fn test_tilde_requirement() {
        // ~1.2.3 := >=1.2.3, <1.3.0
        let req: VersionReq = "~1.2.3".parse().unwrap();
        assert!(req.matches(&Version::new(1, 2, 3)));
        assert!(req.matches(&Version::new(1, 2, 9)));
        assert!(!req.matches(&Version::new(1, 3, 0)));
    }

    #[test]
    fn test_greater_than_requirement() {
        let req: VersionReq = ">=1.0.0".parse().unwrap();
        assert!(req.matches(&Version::new(1, 0, 0)));
        assert!(req.matches(&Version::new(2, 0, 0)));
        assert!(!req.matches(&Version::new(0, 9, 9)));
    }

    #[test]
    fn test_less_than_requirement() {
        let req: VersionReq = "<2.0.0".parse().unwrap();
        assert!(req.matches(&Version::new(1, 9, 9)));
        assert!(!req.matches(&Version::new(2, 0, 0)));
    }

    #[test]
    fn test_combined_requirements() {
        let req: VersionReq = ">=1.0.0, <2.0.0".parse().unwrap();
        assert!(req.matches(&Version::new(1, 0, 0)));
        assert!(req.matches(&Version::new(1, 9, 9)));
        assert!(!req.matches(&Version::new(0, 9, 9)));
        assert!(!req.matches(&Version::new(2, 0, 0)));
    }

    #[test]
    fn test_wildcard_requirement() {
        let req: VersionReq = "1.*".parse().unwrap();
        assert!(req.matches(&Version::new(1, 0, 0)));
        assert!(req.matches(&Version::new(1, 9, 9)));
        assert!(!req.matches(&Version::new(2, 0, 0)));
    }
}

// ============================================================================
// API Version Compatibility Tests
// ============================================================================

mod api_compatibility_tests {
    use super::*;

    /// Simulates API version checking between provider and core
    fn is_api_compatible(provider_api: &Version, core_api: &Version) -> bool {
        // Major version must match for compatibility
        // Minor version of provider must be <= core (core provides at least what provider needs)
        provider_api.major == core_api.major && provider_api.minor <= core_api.minor
    }

    #[test]
    fn test_exact_api_version_match() {
        let provider = Version::new(1, 0, 0);
        let core = Version::new(1, 0, 0);
        assert!(is_api_compatible(&provider, &core));
    }

    #[test]
    fn test_provider_older_minor_compatible() {
        // Provider needs API 1.0, core provides API 1.2 - compatible
        let provider = Version::new(1, 0, 0);
        let core = Version::new(1, 2, 0);
        assert!(is_api_compatible(&provider, &core));
    }

    #[test]
    fn test_provider_newer_minor_incompatible() {
        // Provider needs API 1.3, core provides API 1.2 - incompatible
        let provider = Version::new(1, 3, 0);
        let core = Version::new(1, 2, 0);
        assert!(!is_api_compatible(&provider, &core));
    }

    #[test]
    fn test_major_version_mismatch_incompatible() {
        // Major version mismatch is always incompatible
        let provider = Version::new(2, 0, 0);
        let core = Version::new(1, 9, 9);
        assert!(!is_api_compatible(&provider, &core));
    }

    #[test]
    fn test_api_version_mismatch_error() {
        let err = ProviderError::ApiVersionMismatch {
            required: Version::new(2, 0, 0),
            available: Version::new(1, 0, 0),
        };

        let msg = err.to_string();
        assert!(msg.contains("API version mismatch"));
        assert!(msg.contains("2.0.0"));
        assert!(msg.contains("1.0.0"));
    }
}

// ============================================================================
// Provider Dependency Version Tests
// ============================================================================

mod dependency_version_tests {
    use super::*;

    fn dependency_satisfied(dep: &ProviderDependency, available: &Version) -> bool {
        let req: VersionReq = dep.req.parse().unwrap_or_else(|_| VersionReq::STAR);
        req.matches(available)
    }

    #[test]
    fn test_dependency_exact_version() {
        let dep = ProviderDependency {
            name: "core".to_string(),
            req: "=1.0.0".to_string(),
            optional: false,
        };

        assert!(dependency_satisfied(&dep, &Version::new(1, 0, 0)));
        assert!(!dependency_satisfied(&dep, &Version::new(1, 0, 1)));
    }

    #[test]
    fn test_dependency_caret_version() {
        let dep = ProviderDependency {
            name: "utils".to_string(),
            req: "^1.2.0".to_string(),
            optional: false,
        };

        assert!(dependency_satisfied(&dep, &Version::new(1, 2, 0)));
        assert!(dependency_satisfied(&dep, &Version::new(1, 9, 9)));
        assert!(!dependency_satisfied(&dep, &Version::new(2, 0, 0)));
    }

    #[test]
    fn test_dependency_range_version() {
        let dep = ProviderDependency {
            name: "network".to_string(),
            req: ">=1.0.0, <2.0.0".to_string(),
            optional: false,
        };

        assert!(dependency_satisfied(&dep, &Version::new(1, 5, 0)));
        assert!(!dependency_satisfied(&dep, &Version::new(2, 0, 0)));
        assert!(!dependency_satisfied(&dep, &Version::new(0, 9, 0)));
    }

    #[test]
    fn test_optional_dependency() {
        let dep = ProviderDependency {
            name: "optional-feature".to_string(),
            req: ">=1.0.0".to_string(),
            optional: true,
        };

        // Optional dependencies can be missing
        assert!(dep.optional);
    }
}

// ============================================================================
// Provider Index Version Tests
// ============================================================================

mod index_version_tests {
    use super::*;

    #[test]
    fn test_index_entry_version_parsing() {
        let entry = ProviderIndexEntry {
            name: "test-provider".to_string(),
            vers: "1.2.3".to_string(),
            deps: vec![],
            cksum: "abc123".to_string(),
            features: HashMap::new(),
            yanked: false,
            rustible_version: Some(">=0.1.0".to_string()),
            api_version: Some("1.0.0".to_string()),
            targets: vec![],
            capabilities: vec![],
        };

        let version = entry.version().unwrap();
        assert_eq!(version, Version::new(1, 2, 3));
    }

    #[test]
    fn test_index_entry_api_version() {
        let entry = ProviderIndexEntry {
            name: "versioned".to_string(),
            vers: "2.0.0".to_string(),
            deps: vec![],
            cksum: "xyz".to_string(),
            features: HashMap::new(),
            yanked: false,
            rustible_version: None,
            api_version: Some("1.1.0".to_string()),
            targets: vec![],
            capabilities: vec![],
        };

        assert_eq!(entry.api_version, Some("1.1.0".to_string()));
    }

    #[test]
    fn test_index_entry_rustible_version_requirement() {
        let entry = ProviderIndexEntry {
            name: "requires-new".to_string(),
            vers: "1.0.0".to_string(),
            deps: vec![],
            cksum: "checksum".to_string(),
            features: HashMap::new(),
            yanked: false,
            rustible_version: Some(">=0.2.0".to_string()),
            api_version: None,
            targets: vec![],
            capabilities: vec![],
        };

        let req: VersionReq = entry.rustible_version.unwrap().parse().unwrap();
        assert!(req.matches(&Version::new(0, 2, 0)));
        assert!(req.matches(&Version::new(0, 3, 0)));
        assert!(!req.matches(&Version::new(0, 1, 9)));
    }

    #[test]
    fn test_yanked_version() {
        let entry = ProviderIndexEntry {
            name: "yanked-provider".to_string(),
            vers: "1.0.0".to_string(),
            deps: vec![],
            cksum: "old-checksum".to_string(),
            features: HashMap::new(),
            yanked: true,
            rustible_version: None,
            api_version: None,
            targets: vec![],
            capabilities: vec![],
        };

        assert!(entry.yanked);
    }
}

// ============================================================================
// Breaking Change Tests
// ============================================================================

mod breaking_change_tests {
    use super::*;

    /// Determines if a version change is a breaking change
    fn is_breaking_change(old: &Version, new: &Version) -> bool {
        // Major version bump indicates breaking change
        new.major > old.major
    }

    /// Determines if a version change is a minor (feature) change
    fn is_minor_change(old: &Version, new: &Version) -> bool {
        old.major == new.major && new.minor > old.minor
    }

    /// Determines if a version change is a patch (fix) change
    fn is_patch_change(old: &Version, new: &Version) -> bool {
        old.major == new.major && old.minor == new.minor && new.patch > old.patch
    }

    #[test]
    fn test_major_version_is_breaking() {
        let old = Version::new(1, 5, 3);
        let new = Version::new(2, 0, 0);
        assert!(is_breaking_change(&old, &new));
    }

    #[test]
    fn test_minor_version_not_breaking() {
        let old = Version::new(1, 2, 0);
        let new = Version::new(1, 3, 0);
        assert!(!is_breaking_change(&old, &new));
        assert!(is_minor_change(&old, &new));
    }

    #[test]
    fn test_patch_version_not_breaking() {
        let old = Version::new(1, 2, 3);
        let new = Version::new(1, 2, 4);
        assert!(!is_breaking_change(&old, &new));
        assert!(!is_minor_change(&old, &new));
        assert!(is_patch_change(&old, &new));
    }

    #[test]
    fn test_version_downgrade() {
        let old = Version::new(2, 0, 0);
        let new = Version::new(1, 0, 0);
        // Downgrade is not a "breaking change" in the forward sense
        assert!(!is_breaking_change(&old, &new));
    }
}

// ============================================================================
// Metadata Version Tests
// ============================================================================

mod metadata_version_tests {
    use super::*;

    #[test]
    fn test_provider_metadata_version() {
        let metadata = ProviderMetadata {
            name: "test".to_string(),
            version: Version::new(1, 2, 3),
            api_version: Version::new(1, 0, 0),
            supported_targets: vec![],
            capabilities: vec![],
        };

        assert_eq!(metadata.version, Version::new(1, 2, 3));
    }

    #[test]
    fn test_provider_metadata_api_version() {
        let metadata = ProviderMetadata {
            name: "test".to_string(),
            version: Version::new(2, 0, 0),
            api_version: Version::new(1, 1, 0),
            supported_targets: vec![],
            capabilities: vec![],
        };

        // Provider version can be different from API version
        assert_eq!(metadata.version, Version::new(2, 0, 0));
        assert_eq!(metadata.api_version, Version::new(1, 1, 0));
    }

    #[test]
    fn test_index_entry_from_metadata_preserves_versions() {
        let metadata = ProviderMetadata {
            name: "versioned-provider".to_string(),
            version: Version::new(3, 1, 4),
            api_version: Version::new(1, 2, 0),
            supported_targets: vec!["test".to_string()],
            capabilities: vec![ProviderCapability::Read],
        };

        let entry = ProviderIndexEntry::from_metadata(&metadata, "checksum");

        assert_eq!(entry.vers, "3.1.4");
        assert_eq!(entry.api_version, Some("1.2.0".to_string()));
    }
}

// ============================================================================
// Version Sorting Tests
// ============================================================================

mod version_sorting_tests {
    use super::*;

    #[test]
    fn test_sort_versions_ascending() {
        let mut versions = vec![
            Version::new(1, 0, 0),
            Version::new(2, 0, 0),
            Version::new(0, 1, 0),
            Version::new(1, 1, 0),
        ];

        versions.sort();

        assert_eq!(versions[0], Version::new(0, 1, 0));
        assert_eq!(versions[1], Version::new(1, 0, 0));
        assert_eq!(versions[2], Version::new(1, 1, 0));
        assert_eq!(versions[3], Version::new(2, 0, 0));
    }

    #[test]
    fn test_sort_versions_with_prerelease() {
        let mut versions = vec![
            "1.0.0".parse::<Version>().unwrap(),
            "1.0.0-alpha".parse::<Version>().unwrap(),
            "1.0.0-beta".parse::<Version>().unwrap(),
            "1.0.0-rc.1".parse::<Version>().unwrap(),
        ];

        versions.sort();

        // Pre-release versions sort before stable
        assert_eq!(versions[3], "1.0.0".parse::<Version>().unwrap());
    }

    #[test]
    fn test_find_latest_compatible() {
        let available = vec![
            Version::new(1, 0, 0),
            Version::new(1, 1, 0),
            Version::new(1, 2, 0),
            Version::new(2, 0, 0),
        ];

        let req: VersionReq = "^1.0.0".parse().unwrap();

        let compatible: Vec<_> = available
            .iter()
            .filter(|v| req.matches(v))
            .collect();

        assert_eq!(compatible.len(), 3);

        let latest = compatible.iter().max().unwrap();
        assert_eq!(**latest, Version::new(1, 2, 0));
    }
}

// ============================================================================
// Version Policy Documentation Tests
// ============================================================================

mod version_policy_tests {
    use super::*;

    /// Documents the version policy for providers
    #[test]
    fn test_version_policy_major_bump_required_for_breaking() {
        // Major version bump is required when:
        // - Removing a module
        // - Changing module parameter types
        // - Removing required parameters
        // - Changing output types
        // - Changing error types

        let breaking_changes = vec![
            "Removing a module",
            "Changing parameter types",
            "Removing required parameters",
            "Changing output types",
            "Changing error behavior",
        ];

        assert!(breaking_changes.len() >= 5);
    }

    /// Documents when minor version bump is appropriate
    #[test]
    fn test_version_policy_minor_bump_for_features() {
        // Minor version bump is appropriate when:
        // - Adding new modules
        // - Adding optional parameters
        // - Adding new capabilities
        // - Adding new output fields

        let feature_additions = vec![
            "Adding new modules",
            "Adding optional parameters",
            "Adding new capabilities",
            "Adding new output fields",
        ];

        assert!(feature_additions.len() >= 4);
    }

    /// Documents when patch version bump is appropriate
    #[test]
    fn test_version_policy_patch_bump_for_fixes() {
        // Patch version bump is appropriate when:
        // - Bug fixes
        // - Documentation updates
        // - Performance improvements
        // - Internal refactoring

        let patch_changes = vec![
            "Bug fixes",
            "Documentation updates",
            "Performance improvements",
            "Internal refactoring",
        ];

        assert!(patch_changes.len() >= 4);
    }
}

// ============================================================================
// Compatibility Matrix Tests
// ============================================================================

mod compatibility_matrix_tests {
    use super::*;

    /// Simulates a compatibility check between provider and core versions
    fn check_compatibility(
        _provider_version: &Version,
        provider_api: &Version,
        _core_version: &Version,
        core_api: &Version,
    ) -> Result<(), String> {
        // Check API version compatibility
        if provider_api.major != core_api.major {
            return Err(format!(
                "API major version mismatch: provider {} vs core {}",
                provider_api, core_api
            ));
        }

        if provider_api.minor > core_api.minor {
            return Err(format!(
                "Provider requires API {}, but core only provides {}",
                provider_api, core_api
            ));
        }

        Ok(())
    }

    #[test]
    fn test_compatible_versions() {
        let result = check_compatibility(
            &Version::new(1, 0, 0), // provider version
            &Version::new(1, 0, 0), // provider API
            &Version::new(0, 5, 0), // core version
            &Version::new(1, 2, 0), // core API
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_incompatible_api_major() {
        let result = check_compatibility(
            &Version::new(2, 0, 0),
            &Version::new(2, 0, 0),
            &Version::new(1, 0, 0),
            &Version::new(1, 0, 0),
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("major version mismatch"));
    }

    #[test]
    fn test_provider_requires_newer_api() {
        let result = check_compatibility(
            &Version::new(1, 5, 0),
            &Version::new(1, 3, 0), // Provider needs API 1.3
            &Version::new(0, 9, 0),
            &Version::new(1, 2, 0), // Core only provides API 1.2
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("requires API"));
    }
}

// ============================================================================
// Serialization Version Tests
// ============================================================================

mod serialization_tests {
    use super::*;

    #[test]
    fn test_version_json_roundtrip() {
        let original = Version::new(1, 2, 3);
        let json = serde_json::to_string(&original).unwrap();
        let parsed: Version = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn test_metadata_json_roundtrip() {
        let original = ProviderMetadata {
            name: "test".to_string(),
            version: Version::new(1, 0, 0),
            api_version: Version::new(1, 0, 0),
            supported_targets: vec!["test".to_string()],
            capabilities: vec![ProviderCapability::Read],
        };

        let json = serde_json::to_string(&original).unwrap();
        let parsed: ProviderMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.name, original.name);
        assert_eq!(parsed.version, original.version);
    }

    #[test]
    fn test_dependency_json_roundtrip() {
        let original = ProviderDependency {
            name: "core".to_string(),
            req: "^1.0.0".to_string(),
            optional: false,
        };

        let json = serde_json::to_string(&original).unwrap();
        let parsed: ProviderDependency = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.name, original.name);
        assert_eq!(parsed.req, original.req);
    }
}
