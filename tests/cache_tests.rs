//! Comprehensive Cache System Tests
//!
//! This test suite validates the caching functionality in Rustible:
//!
//! 1. Template Caching - Compiled template storage and retrieval
//! 2. Fact Caching - Host fact storage with TTL and subset support
//! 3. Module Result Caching - Idempotent operation result storage
//! 4. Cache Invalidation - TTL expiry, dependency invalidation, manual invalidation
//! 5. Tiered Fact Caching - Multi-tier L1/L2/L3 caching with promotion/demotion
//! 6. Variable Caching - Variable context caching with merge support
//! 7. Playbook/Role Caching - Parsed structure caching

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use indexmap::IndexMap;
use serde_json::{json, Value as JsonValue};

use rustible::cache::module_result::{
    classify_module_idempotency, CachedModuleResult, IdempotencyClass, ModuleCacheKey,
    ModuleResultCache,
};
use rustible::cache::{
    Cache, CacheConfig, CacheDependency, CacheManager, CacheMetrics, CacheType, FactCache,
    PlaybookCache, RoleCache, VariableCache,
};

// ============================================================================
// Template Cache Tests
// ============================================================================

mod template_cache_tests {
    use super::*;
    use rustible::cache::template::{
        LazyTemplate, TemplateCache, TemplateCacheConfig, TemplateCacheKey, TemplatePreloader,
    };

    #[test]
    fn test_template_cache_basic_render() {
        let cache = TemplateCache::new(TemplateCacheConfig::default());
        let vars: HashMap<String, JsonValue> = [
            ("name".to_string(), json!("World")),
            ("count".to_string(), json!(42)),
        ]
        .into_iter()
        .collect();

        let result = cache
            .render("Hello {{ name }}, count: {{ count }}", &vars)
            .unwrap();
        assert_eq!(result, "Hello World, count: 42");
    }

    #[test]
    fn test_template_cache_hit_on_second_render() {
        let cache = TemplateCache::new(TemplateCacheConfig::default());
        let vars: HashMap<String, JsonValue> =
            [("name".to_string(), json!("World"))].into_iter().collect();

        let template = "Hello {{ name }}!";

        // First render - cache miss
        cache.render(template, &vars).unwrap();

        // Second render - cache hit
        cache.render(template, &vars).unwrap();

        let metrics = cache.metrics();
        assert_eq!(metrics.hits.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.misses.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_template_cache_plain_string_not_cached() {
        let cache = TemplateCache::new(TemplateCacheConfig::default());
        let vars: HashMap<String, JsonValue> = HashMap::new();

        // Plain string without template syntax
        let result = cache.render("Hello World!", &vars).unwrap();
        assert_eq!(result, "Hello World!");

        // Should not be cached (fast path)
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_template_cache_is_template_detection() {
        assert!(TemplateCache::is_template("Hello {{ name }}"));
        assert!(TemplateCache::is_template("{% if x %}yes{% endif %}"));
        assert!(TemplateCache::is_template("{# comment #}"));
        assert!(!TemplateCache::is_template("Hello World"));
        assert!(!TemplateCache::is_template("Just a plain string"));
        assert!(!TemplateCache::is_template("{ not a template }"));
    }

    #[test]
    fn test_template_cache_precompile() {
        let cache = TemplateCache::new(TemplateCacheConfig::default());

        // Precompile a valid template
        assert!(cache.precompile("Hello {{ name }}!").is_ok());
        assert_eq!(cache.len(), 1);

        // Precompile an invalid template
        let result = cache.precompile("Hello {{ name");
        assert!(result.is_err());
    }

    #[test]
    fn test_template_cache_precompile_batch() {
        let cache = TemplateCache::new(TemplateCacheConfig::default());

        let templates = vec![
            "Hello {{ name }}!",
            "Count: {{ count }}",
            "{% for item in items %}{{ item }}{% endfor %}",
        ];

        let compiled = cache.precompile_batch(&templates);
        assert_eq!(compiled, 3);
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn test_template_cache_eviction_on_max_entries() {
        let config = TemplateCacheConfig {
            max_templates: 2,
            ..Default::default()
        };
        let cache = TemplateCache::new(config);
        let vars: HashMap<String, JsonValue> = HashMap::new();

        cache.render("Template 1: {{ x }}", &vars).unwrap();
        thread::sleep(Duration::from_millis(10));
        cache.render("Template 2: {{ y }}", &vars).unwrap();
        thread::sleep(Duration::from_millis(10));

        // Third template should evict the first (LRU)
        cache.render("Template 3: {{ z }}", &vars).unwrap();

        assert_eq!(cache.len(), 2);
        assert!(cache.metrics().evictions.load(Ordering::Relaxed) >= 1);
    }

    #[test]
    fn test_template_cache_key_hash_consistency() {
        let key1 = TemplateCacheKey::from_source("Hello {{ name }}!");
        let key2 = TemplateCacheKey::from_source("Hello {{ name }}!");
        let key3 = TemplateCacheKey::from_source("Different {{ template }}");

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
        assert_eq!(key1.content_hash, key2.content_hash);
    }

    #[test]
    fn test_template_cache_stats_tracking() {
        let cache = TemplateCache::new(TemplateCacheConfig::default());
        let vars: HashMap<String, JsonValue> =
            [("name".to_string(), json!("World"))].into_iter().collect();

        let template = "Hello {{ name }}!";

        // Render multiple times
        for _ in 0..5 {
            cache.render(template, &vars).unwrap();
        }

        let stats = cache.get_template_stats(template).unwrap();
        assert_eq!(stats.render_count, 5);
        // Time saved should be positive after multiple renders
        assert!(stats.time_saved_us > 0);
    }

    #[test]
    fn test_lazy_template_standalone() {
        let lazy = LazyTemplate::new("Hello {{ name }}!");
        assert!(!lazy.is_compiled());
        assert_eq!(lazy.source(), "Hello {{ name }}!");

        let vars: HashMap<String, JsonValue> =
            [("name".to_string(), json!("World"))].into_iter().collect();

        let result = lazy.render(&vars).unwrap();
        assert_eq!(result, "Hello World!");
    }

    #[test]
    fn test_lazy_template_with_cache() {
        let cache = Arc::new(TemplateCache::new(TemplateCacheConfig::default()));
        let lazy = LazyTemplate::with_cache("Hello {{ name }}!", cache.clone());

        let vars: HashMap<String, JsonValue> =
            [("name".to_string(), json!("World"))].into_iter().collect();

        let result = lazy.render(&vars).unwrap();
        assert_eq!(result, "Hello World!");

        // Template should be in the cache now
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_template_preloader() {
        let mut preloader = TemplatePreloader::new();
        preloader
            .add("Hello {{ name }}")
            .add("Count: {{ count }}")
            .add_all(vec!["Item: {{ item }}", "Value: {{ value }}"]);

        assert_eq!(preloader.len(), 4);
        assert!(!preloader.is_empty());

        let cache = TemplateCache::new(TemplateCacheConfig::default());
        let loaded = preloader.preload(&cache);
        assert_eq!(loaded, 4);
        assert_eq!(cache.len(), 4);
    }

    #[test]
    fn test_template_cache_complex_templates() {
        let cache = TemplateCache::new(TemplateCacheConfig::default());

        let vars: HashMap<String, JsonValue> = [
            ("items".to_string(), json!(["a", "b", "c"])),
            ("show".to_string(), json!(true)),
        ]
        .into_iter()
        .collect();

        // Loop template
        let result = cache
            .render("{% for item in items %}{{ item }}{% endfor %}", &vars)
            .unwrap();
        assert_eq!(result, "abc");

        // Conditional template
        let result = cache
            .render("{% if show %}visible{% else %}hidden{% endif %}", &vars)
            .unwrap();
        assert_eq!(result, "visible");
    }

    #[test]
    fn test_template_cache_memory_tracking() {
        let config = TemplateCacheConfig {
            max_memory_bytes: 1024 * 1024, // 1MB
            ..Default::default()
        };
        let cache = TemplateCache::new(config);
        let vars: HashMap<String, JsonValue> = HashMap::new();

        // Add some templates
        cache.render("Template {{ x }}", &vars).unwrap();
        cache.render("Another {{ y }}", &vars).unwrap();

        let metrics = cache.metrics();
        assert!(metrics.memory_bytes.load(Ordering::Relaxed) > 0);
        assert_eq!(metrics.template_count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_template_cache_clear() {
        let cache = TemplateCache::new(TemplateCacheConfig::default());
        let vars: HashMap<String, JsonValue> = HashMap::new();

        cache.render("Template {{ x }}", &vars).unwrap();
        cache.render("Template {{ y }}", &vars).unwrap();
        assert_eq!(cache.len(), 2);

        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }
}

// ============================================================================
// Fact Cache Tests
// ============================================================================

mod fact_cache_tests {
    use super::*;
    use rustible::cache::facts::CachedFacts;

    fn sample_facts() -> IndexMap<String, JsonValue> {
        let mut facts = IndexMap::new();
        facts.insert("ansible_os_family".to_string(), json!("Debian"));
        facts.insert("ansible_distribution".to_string(), json!("Ubuntu"));
        facts.insert("ansible_distribution_version".to_string(), json!("22.04"));
        facts.insert("ansible_hostname".to_string(), json!("test-host"));
        facts.insert("ansible_fqdn".to_string(), json!("test-host.example.com"));
        facts.insert(
            "ansible_default_ipv4".to_string(),
            json!({
                "address": "192.168.1.100"
            }),
        );
        facts
    }

    #[test]
    fn test_fact_cache_basic_insert_get() {
        let cache = FactCache::new(CacheConfig::default());

        cache.insert_raw("host1", sample_facts());

        let cached = cache.get("host1").unwrap();
        assert_eq!(cached.os_family, Some("Debian".to_string()));
        assert_eq!(cached.distribution, Some("Ubuntu".to_string()));
        assert_eq!(cached.distribution_version, Some("22.04".to_string()));
    }

    #[test]
    fn test_fact_cache_get_nonexistent() {
        let cache = FactCache::new(CacheConfig::default());

        assert!(cache.get("nonexistent-host").is_none());
    }

    #[test]
    fn test_fact_cache_get_by_ip() {
        let cache = FactCache::new(CacheConfig::default());

        cache.insert_raw("host1", sample_facts());

        // Should be able to get by IP after insertion
        let cached = cache.get_by_ip("192.168.1.100").unwrap();
        assert_eq!(cached.hostname, "host1");
    }

    #[test]
    fn test_fact_cache_subsets() {
        let cache = FactCache::new(CacheConfig::default());

        cache.insert_with_subsets(
            "host1",
            sample_facts(),
            vec!["network".to_string(), "hardware".to_string()],
        );

        let cached = cache.get("host1").unwrap();
        assert!(cached.covers_subsets(&["network".to_string()]));
        assert!(cached.covers_subsets(&["hardware".to_string()]));
        assert!(!cached.covers_subsets(&["all".to_string()]));
    }

    #[test]
    fn test_fact_cache_subsets_all() {
        let cache = FactCache::new(CacheConfig::default());

        // Default insert uses "all" subset
        cache.insert_raw("host1", sample_facts());

        let cached = cache.get("host1").unwrap();
        assert!(cached.covers_subsets(&["network".to_string()]));
        assert!(cached.covers_subsets(&["hardware".to_string()]));
        assert!(cached.covers_subsets(&["all".to_string()]));
    }

    #[test]
    fn test_fact_cache_invalidate_host() {
        let cache = FactCache::new(CacheConfig::default());

        cache.insert_raw("host1", sample_facts());
        cache.insert_raw("host2", sample_facts());

        cache.invalidate_host("host1");

        assert!(cache.get("host1").is_none());
        assert!(cache.get("host2").is_some());
    }

    #[test]
    fn test_fact_cache_clear() {
        let cache = FactCache::new(CacheConfig::default());

        cache.insert_raw("host1", sample_facts());
        cache.insert_raw("host2", sample_facts());
        assert_eq!(cache.len(), 2);

        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_fact_cache_merge_facts() {
        let cache = FactCache::new(CacheConfig::default());

        cache.insert_raw("host1", sample_facts());

        let mut additional = IndexMap::new();
        additional.insert("custom_fact".to_string(), json!("custom_value"));

        cache.merge_facts("host1", additional);

        let cached = cache.get("host1").unwrap();
        assert_eq!(cached.get_str("custom_fact"), Some("custom_value"));
        // Original facts should be preserved
        assert_eq!(cached.os_family, Some("Debian".to_string()));
    }

    #[test]
    fn test_fact_cache_needs_refresh() {
        let cache = FactCache::new(CacheConfig::default());

        cache.insert_raw("host1", sample_facts());

        // Should not need refresh immediately
        assert!(!cache.needs_refresh("host1", Duration::from_secs(60)));

        // Unknown host should need refresh
        assert!(cache.needs_refresh("unknown", Duration::from_secs(60)));
    }

    #[test]
    fn test_fact_cache_hostnames() {
        let cache = FactCache::new(CacheConfig::default());

        cache.insert_raw("host1", sample_facts());
        cache.insert_raw("host2", sample_facts());
        cache.insert_raw("host3", sample_facts());

        let hostnames = cache.hostnames();
        assert_eq!(hostnames.len(), 3);
        assert!(hostnames.contains(&"host1".to_string()));
        assert!(hostnames.contains(&"host2".to_string()));
        assert!(hostnames.contains(&"host3".to_string()));
    }

    #[test]
    fn test_fact_cache_metrics() {
        let cache = FactCache::new(CacheConfig::default());

        cache.insert_raw("host1", sample_facts());

        // Hit
        cache.get("host1");
        // Miss
        cache.get("nonexistent");

        let metrics = cache.metrics();
        assert_eq!(metrics.hits.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.misses.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_cached_facts_size_estimate() {
        let facts = CachedFacts::new("host1", sample_facts());
        let size = facts.size_bytes();

        // Should have a reasonable size
        assert!(size > 0);
        assert!(size < 10000); // Not too big for simple facts
    }

    #[test]
    fn test_fact_cache_get_with_subsets() {
        let cache = FactCache::new(CacheConfig::default());

        cache.insert_with_subsets("host1", sample_facts(), vec!["network".to_string()]);

        // Should find when requesting covered subset
        assert!(cache
            .get_with_subsets("host1", &["network".to_string()])
            .is_some());

        // Should not find when requesting uncovered subset
        assert!(cache
            .get_with_subsets("host1", &["all".to_string()])
            .is_none());
    }
}

// ============================================================================
// Module Result Cache Tests
// ============================================================================

mod module_result_cache_tests {
    use super::*;

    fn sample_params() -> HashMap<String, JsonValue> {
        let mut params = HashMap::new();
        params.insert("name".to_string(), json!("nginx"));
        params.insert("state".to_string(), json!("present"));
        params
    }

    fn sample_result() -> CachedModuleResult {
        CachedModuleResult {
            changed: false,
            msg: "Package already installed".to_string(),
            success: true,
            diff: None,
            data: HashMap::new(),
            cached_at: None,
            ttl: None,
        }
    }

    #[test]
    fn test_module_cache_key_creation() {
        let params = sample_params();
        let key = ModuleCacheKey::new("apt", &params, "host1", false, None);

        assert_eq!(key.module, "apt");
        assert_eq!(key.host, "host1");
        assert!(!key.check_mode);
        assert!(key.become_user.is_none());
    }

    #[test]
    fn test_module_cache_key_determinism() {
        let mut params1 = HashMap::new();
        params1.insert("name".to_string(), json!("nginx"));
        params1.insert("state".to_string(), json!("present"));

        let mut params2 = HashMap::new();
        // Insert in different order
        params2.insert("state".to_string(), json!("present"));
        params2.insert("name".to_string(), json!("nginx"));

        let key1 = ModuleCacheKey::new("apt", &params1, "host1", false, None);
        let key2 = ModuleCacheKey::new("apt", &params2, "host1", false, None);

        // Should produce same hash regardless of insertion order
        assert_eq!(key1.params_hash, key2.params_hash);
    }

    #[test]
    fn test_module_cache_key_different_for_different_params() {
        let params1 = sample_params();

        let mut params2 = sample_params();
        params2.insert("extra".to_string(), json!("value"));

        let key1 = ModuleCacheKey::new("apt", &params1, "host1", false, None);
        let key2 = ModuleCacheKey::new("apt", &params2, "host1", false, None);

        assert_ne!(key1.params_hash, key2.params_hash);
    }

    #[test]
    fn test_idempotency_classification_fully_idempotent() {
        let params = HashMap::new();

        assert_eq!(
            classify_module_idempotency("stat", &params),
            IdempotencyClass::FullyIdempotent
        );
        assert_eq!(
            classify_module_idempotency("debug", &params),
            IdempotencyClass::FullyIdempotent
        );
        assert_eq!(
            classify_module_idempotency("set_fact", &params),
            IdempotencyClass::FullyIdempotent
        );
    }

    #[test]
    fn test_idempotency_classification_state_based() {
        let params = HashMap::new();

        assert_eq!(
            classify_module_idempotency("copy", &params),
            IdempotencyClass::StateBasedIdempotent
        );
        assert_eq!(
            classify_module_idempotency("template", &params),
            IdempotencyClass::StateBasedIdempotent
        );
        assert_eq!(
            classify_module_idempotency("file", &params),
            IdempotencyClass::StateBasedIdempotent
        );
    }

    #[test]
    fn test_idempotency_classification_command_without_creates() {
        let params = HashMap::new();

        assert_eq!(
            classify_module_idempotency("command", &params),
            IdempotencyClass::NonIdempotent
        );
        assert_eq!(
            classify_module_idempotency("shell", &params),
            IdempotencyClass::NonIdempotent
        );
    }

    #[test]
    fn test_idempotency_classification_command_with_creates() {
        let mut params = HashMap::new();
        params.insert("creates".to_string(), json!("/tmp/marker"));

        assert_eq!(
            classify_module_idempotency("command", &params),
            IdempotencyClass::ConditionallyIdempotent
        );
    }

    #[test]
    fn test_idempotency_classification_package_with_state() {
        let mut params = HashMap::new();
        params.insert("state".to_string(), json!("present"));

        assert_eq!(
            classify_module_idempotency("apt", &params),
            IdempotencyClass::StateBasedIdempotent
        );
        assert_eq!(
            classify_module_idempotency("yum", &params),
            IdempotencyClass::StateBasedIdempotent
        );
    }

    #[test]
    fn test_module_result_cache_put_get() {
        let cache = ModuleResultCache::new(CacheConfig::default());
        let key = ModuleCacheKey::new("apt", &sample_params(), "host1", false, None);

        cache.put(
            key.clone(),
            sample_result(),
            IdempotencyClass::StateBasedIdempotent,
        );

        let cached = cache.get(&key).unwrap();
        assert_eq!(cached.msg, "Package already installed");
        assert!(!cached.changed);
        assert!(cached.success);
    }

    #[test]
    fn test_module_result_cache_non_idempotent_not_cached() {
        let cache = ModuleResultCache::new(CacheConfig::default());
        let params = HashMap::new();
        let key = ModuleCacheKey::new("shell", &params, "host1", false, None);

        let result = CachedModuleResult {
            changed: true,
            msg: "Command executed".to_string(),
            success: true,
            diff: None,
            data: HashMap::new(),
            cached_at: None,
            ttl: None,
        };

        cache.put(key.clone(), result, IdempotencyClass::NonIdempotent);

        // Should not be cached
        assert!(cache.get(&key).is_none());
    }

    #[test]
    fn test_module_result_cache_invalidation() {
        let cache = ModuleResultCache::new(CacheConfig::default());
        let key = ModuleCacheKey::new("apt", &sample_params(), "host1", false, None);

        cache.put(
            key.clone(),
            sample_result(),
            IdempotencyClass::StateBasedIdempotent,
        );
        assert!(cache.get(&key).is_some());

        cache.invalidate(&key);
        assert!(cache.get(&key).is_none());
    }

    #[test]
    fn test_module_result_cache_host_invalidation() {
        let cache = ModuleResultCache::new(CacheConfig::default());
        let params = HashMap::new();

        let key1 = ModuleCacheKey::new("stat", &params, "host1", false, None);
        let key2 = ModuleCacheKey::new("stat", &params, "host2", false, None);

        cache.put(
            key1.clone(),
            sample_result(),
            IdempotencyClass::FullyIdempotent,
        );
        cache.put(
            key2.clone(),
            sample_result(),
            IdempotencyClass::FullyIdempotent,
        );

        cache.invalidate_host("host1");

        assert!(cache.get(&key1).is_none());
        assert!(cache.get(&key2).is_some());
    }

    #[test]
    fn test_module_result_cache_module_invalidation() {
        let cache = ModuleResultCache::new(CacheConfig::default());
        let params = HashMap::new();

        let key1 = ModuleCacheKey::new("stat", &params, "host1", false, None);
        let key2 = ModuleCacheKey::new("copy", &params, "host1", false, None);

        cache.put(
            key1.clone(),
            sample_result(),
            IdempotencyClass::FullyIdempotent,
        );
        cache.put(
            key2.clone(),
            sample_result(),
            IdempotencyClass::StateBasedIdempotent,
        );

        cache.invalidate_module("stat");

        assert!(cache.get(&key1).is_none());
        assert!(cache.get(&key2).is_some());
    }

    #[test]
    fn test_module_result_cache_clear() {
        let cache = ModuleResultCache::new(CacheConfig::default());
        let params = HashMap::new();

        cache.put(
            ModuleCacheKey::new("stat", &params, "host1", false, None),
            sample_result(),
            IdempotencyClass::FullyIdempotent,
        );
        cache.put(
            ModuleCacheKey::new("stat", &params, "host2", false, None),
            sample_result(),
            IdempotencyClass::FullyIdempotent,
        );

        assert_eq!(cache.len(), 2);

        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_module_result_cache_metrics() {
        let cache = ModuleResultCache::new(CacheConfig::default());
        let key = ModuleCacheKey::new("stat", &HashMap::new(), "host1", false, None);

        cache.put(
            key.clone(),
            sample_result(),
            IdempotencyClass::FullyIdempotent,
        );

        // Hit
        cache.get(&key);
        // Miss
        cache.get(&ModuleCacheKey::new(
            "stat",
            &HashMap::new(),
            "nonexistent",
            false,
            None,
        ));

        let metrics = cache.metrics();
        assert_eq!(metrics.hits.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.misses.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_module_cache_key_display_string() {
        let key = ModuleCacheKey::new("apt", &sample_params(), "host1", true, None);
        let display = key.to_display_string();

        assert!(display.contains("apt"));
        assert!(display.contains("host1"));
        assert!(display.contains("check"));
    }

    #[test]
    fn test_idempotency_class_default_ttl() {
        assert!(IdempotencyClass::FullyIdempotent.default_ttl().is_some());
        assert!(IdempotencyClass::StateBasedIdempotent
            .default_ttl()
            .is_some());
        assert!(IdempotencyClass::ConditionallyIdempotent
            .default_ttl()
            .is_some());
        assert!(IdempotencyClass::NonIdempotent.default_ttl().is_none());
    }

    #[test]
    fn test_idempotency_class_is_cacheable() {
        assert!(IdempotencyClass::FullyIdempotent.is_cacheable());
        assert!(IdempotencyClass::StateBasedIdempotent.is_cacheable());
        assert!(IdempotencyClass::ConditionallyIdempotent.is_cacheable());
        assert!(!IdempotencyClass::NonIdempotent.is_cacheable());
    }
}

// ============================================================================
// Cache Invalidation Tests
// ============================================================================

mod cache_invalidation_tests {
    use super::*;

    #[test]
    fn test_cache_ttl_expiration() {
        let config = CacheConfig {
            default_ttl: Duration::from_millis(50),
            ..Default::default()
        };

        let cache: Cache<String, String> = Cache::new(CacheType::Facts, config);
        cache.insert("key1".to_string(), "value1".to_string(), 10);

        // Should be available immediately
        assert_eq!(cache.get(&"key1".to_string()), Some("value1".to_string()));

        // Wait for expiration
        thread::sleep(Duration::from_millis(100));

        // Should be expired now
        assert_eq!(cache.get(&"key1".to_string()), None);
    }

    #[test]
    fn test_cache_manual_remove() {
        let cache: Cache<String, String> = Cache::new(CacheType::Facts, CacheConfig::default());

        cache.insert("key1".to_string(), "value1".to_string(), 10);
        assert!(cache.get(&"key1".to_string()).is_some());

        let removed = cache.remove(&"key1".to_string());
        assert_eq!(removed, Some("value1".to_string()));
        assert!(cache.get(&"key1".to_string()).is_none());
    }

    #[test]
    fn test_cache_clear_all() {
        let cache: Cache<String, String> = Cache::new(CacheType::Facts, CacheConfig::default());

        cache.insert("key1".to_string(), "value1".to_string(), 10);
        cache.insert("key2".to_string(), "value2".to_string(), 10);
        cache.insert("key3".to_string(), "value3".to_string(), 10);

        assert_eq!(cache.len(), 3);

        cache.clear();

        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_cleanup_expired() {
        let config = CacheConfig {
            default_ttl: Duration::from_millis(50),
            ..Default::default()
        };

        let cache: Cache<String, String> = Cache::new(CacheType::Facts, config);

        cache.insert("key1".to_string(), "value1".to_string(), 10);
        cache.insert("key2".to_string(), "value2".to_string(), 10);

        // Wait for expiration
        thread::sleep(Duration::from_millis(100));

        let removed = cache.cleanup_expired();
        assert_eq!(removed, 2);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_cache_lru_eviction() {
        let config = CacheConfig {
            max_entries: 3,
            ..Default::default()
        };

        let cache: Cache<String, String> = Cache::new(CacheType::Facts, config);

        cache.insert("key1".to_string(), "value1".to_string(), 10);
        thread::sleep(Duration::from_millis(10));
        cache.insert("key2".to_string(), "value2".to_string(), 10);
        thread::sleep(Duration::from_millis(10));
        cache.insert("key3".to_string(), "value3".to_string(), 10);

        // Access key1 to make it recently used
        cache.get(&"key1".to_string());

        // Insert another entry, should evict key2 (least recently used)
        cache.insert("key4".to_string(), "value4".to_string(), 10);

        assert_eq!(cache.get(&"key1".to_string()), Some("value1".to_string()));
        assert_eq!(cache.get(&"key3".to_string()), Some("value3".to_string()));
        assert_eq!(cache.get(&"key4".to_string()), Some("value4".to_string()));
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn test_cache_manager_clear_all() {
        let manager = CacheManager::new();

        manager.facts.insert_raw("host1", IndexMap::new());
        manager
            .variables
            .insert_global(rustible::cache::variable::CachedVariables::new(
                IndexMap::new(),
            ));

        assert!(!manager.facts.is_empty() || !manager.variables.is_empty());

        manager.clear_all();

        assert_eq!(manager.facts.len(), 0);
        assert_eq!(manager.playbooks.len(), 0);
        assert_eq!(manager.roles.len(), 0);
        assert_eq!(manager.variables.len(), 0);
    }

    #[test]
    fn test_cache_manager_invalidate_host() {
        let manager = CacheManager::new();

        let mut facts = IndexMap::new();
        facts.insert("test".to_string(), json!("value"));
        manager.facts.insert_raw("host1", facts.clone());
        manager.facts.insert_raw("host2", facts);

        manager.invalidate_host("host1");

        assert!(manager.facts.get("host1").is_none());
        assert!(manager.facts.get("host2").is_some());
    }

    #[test]
    fn test_cache_manager_cleanup_all() {
        let config = CacheConfig {
            default_ttl: Duration::from_millis(50),
            ..Default::default()
        };

        let manager = CacheManager::with_config(config);

        manager.facts.insert_raw("host1", IndexMap::new());

        thread::sleep(Duration::from_millis(100));

        let result = manager.cleanup_all();
        assert!(result.total() >= 1);
    }

    #[test]
    fn test_disabled_cache_never_stores() {
        let cache: Cache<String, String> = Cache::new(CacheType::Facts, CacheConfig::disabled());

        cache.insert("key1".to_string(), "value1".to_string(), 10);
        // Should not be stored because max_entries is 0
        assert_eq!(cache.get(&"key1".to_string()), None);
    }
}

// ============================================================================
// Variable Cache Tests
// ============================================================================

mod variable_cache_tests {
    use super::*;
    use rustible::cache::variable::{CachedVariables, VariableCacheKey, VariableScope};

    fn sample_variables() -> IndexMap<String, JsonValue> {
        let mut vars = IndexMap::new();
        vars.insert("app_name".to_string(), json!("myapp"));
        vars.insert("app_port".to_string(), json!(8080));
        vars.insert("debug".to_string(), json!(true));
        vars
    }

    #[test]
    fn test_variable_cache_global() {
        let cache = VariableCache::new(CacheConfig::default());

        cache.insert_global(CachedVariables::new(sample_variables()));

        let cached = cache.get_global().unwrap();
        assert_eq!(cached.get_str("app_name"), Some("myapp"));
    }

    #[test]
    fn test_variable_cache_host() {
        let cache = VariableCache::new(CacheConfig::default());

        cache.insert_host("host1", CachedVariables::new(sample_variables()));

        let cached = cache.get_host("host1").unwrap();
        assert!(cached.get("app_port").is_some());
    }

    #[test]
    fn test_variable_cache_key_scopes() {
        let global = VariableCacheKey::global();
        assert_eq!(global.scope, VariableScope::Global);
        assert!(global.hostname.is_none());

        let play = VariableCacheKey::play("my-play");
        assert_eq!(play.scope, VariableScope::Play);
        assert_eq!(play.play_name, Some("my-play".to_string()));

        let host = VariableCacheKey::host("host1");
        assert_eq!(host.scope, VariableScope::Host);
        assert_eq!(host.hostname, Some("host1".to_string()));

        let host_play = VariableCacheKey::host_play("host1", "my-play");
        assert_eq!(host_play.scope, VariableScope::HostPlay);

        let task = VariableCacheKey::task("host1", "my-play", "my-task");
        assert_eq!(task.scope, VariableScope::Task);
    }

    #[test]
    fn test_variable_cache_merge() {
        let cache = VariableCache::new(CacheConfig::default());

        let mut global = sample_variables();
        global.insert("global_var".to_string(), json!("global"));
        cache.insert_global(CachedVariables::new(global));

        let mut host = IndexMap::new();
        host.insert("host_var".to_string(), json!("host"));
        host.insert("app_name".to_string(), json!("overridden"));
        cache.insert_host("host1", CachedVariables::new(host));

        let merged = cache.build_merged("host1", None);
        assert_eq!(merged.get_str("global_var"), Some("global"));
        assert_eq!(merged.get_str("host_var"), Some("host"));
        assert_eq!(merged.get_str("app_name"), Some("overridden")); // Host overrides global
    }

    #[test]
    fn test_variable_cache_template_caching() {
        let cache = VariableCache::new(CacheConfig::default());

        let template = "Hello {{ name }}!";
        let vars_hash = 12345u64;
        let result = "Hello World!".to_string();

        cache.insert_template(template, vars_hash, result.clone());

        let cached = cache.get_template(template, vars_hash).unwrap();
        assert_eq!(cached, result);

        // Different hash should miss
        assert!(cache.get_template(template, 99999).is_none());
    }

    #[test]
    fn test_variable_cache_invalidate_host() {
        let cache = VariableCache::new(CacheConfig::default());

        cache.insert_host("host1", CachedVariables::new(sample_variables()));
        cache.insert_host("host2", CachedVariables::new(sample_variables()));

        cache.invalidate_host("host1");

        assert!(cache.get_host("host1").is_none());
        assert!(cache.get_host("host2").is_some());
    }

    #[test]
    fn test_cached_variables_content_hash() {
        let vars1 = CachedVariables::new(sample_variables());
        let vars2 = CachedVariables::new(sample_variables());

        // Same content should have same hash
        assert!(vars1.content_matches(&vars2));

        // Different content should not match
        let mut different = sample_variables();
        different.insert("extra".to_string(), json!("value"));
        let vars3 = CachedVariables::new(different);

        assert!(!vars1.content_matches(&vars3));
    }

    #[test]
    fn test_cached_variables_merge() {
        let mut vars1 = CachedVariables::new(sample_variables());

        let mut other_map = IndexMap::new();
        other_map.insert("new_var".to_string(), json!("new_value"));
        other_map.insert("app_name".to_string(), json!("overridden"));
        let vars2 = CachedVariables::new(other_map);

        vars1.merge(&vars2);

        assert_eq!(vars1.get_str("new_var"), Some("new_value"));
        assert_eq!(vars1.get_str("app_name"), Some("overridden"));
        assert!(vars1.get("app_port").is_some()); // Original preserved
    }

    #[test]
    fn test_cached_variables_with_source() {
        let vars = CachedVariables::new(sample_variables())
            .with_source(PathBuf::from("/path/to/vars.yml"));

        assert_eq!(vars.source_files.len(), 1);
        assert_eq!(vars.source_files[0], PathBuf::from("/path/to/vars.yml"));
    }

    #[test]
    fn test_cached_variables_with_vault() {
        let vars = CachedVariables::new(sample_variables()).with_vault();
        assert!(vars.has_vault_values);
    }

    #[test]
    fn test_variable_cache_clear() {
        let cache = VariableCache::new(CacheConfig::default());

        cache.insert_global(CachedVariables::new(sample_variables()));
        cache.insert_host("host1", CachedVariables::new(sample_variables()));
        cache.insert_template("template", 123, "result".to_string());

        assert!(cache.len() >= 3);

        cache.clear();
        assert_eq!(cache.len(), 0);
    }
}

// ============================================================================
// Tiered Fact Cache Tests
// ============================================================================

mod tiered_fact_cache_tests {
    use super::*;
    use rustible::cache::tiered_facts::{
        classify_fact_volatility, CacheTier, FactVolatility, PartitionedFacts, TieredCacheConfig,
        TieredExpiry, TieredFactCache,
    };

    fn sample_facts() -> IndexMap<String, JsonValue> {
        let mut facts = IndexMap::new();
        facts.insert("ansible_os_family".to_string(), json!("Debian"));
        facts.insert("ansible_hostname".to_string(), json!("server1"));
        facts.insert("ansible_memfree_mb".to_string(), json!(1024));
        facts.insert("ansible_date_time".to_string(), json!("2024-01-01"));
        facts
    }

    #[test]
    fn test_fact_volatility_classification_static() {
        assert_eq!(
            classify_fact_volatility("ansible_architecture"),
            FactVolatility::Static
        );
        assert_eq!(
            classify_fact_volatility("ansible_os_family"),
            FactVolatility::Static
        );
        assert_eq!(
            classify_fact_volatility("ansible_processor_count"),
            FactVolatility::Static
        );
    }

    #[test]
    fn test_fact_volatility_classification_semi_static() {
        assert_eq!(
            classify_fact_volatility("ansible_hostname"),
            FactVolatility::SemiStatic
        );
        assert_eq!(
            classify_fact_volatility("ansible_default_ipv4"),
            FactVolatility::SemiStatic
        );
        assert_eq!(
            classify_fact_volatility("ansible_mounts"),
            FactVolatility::SemiStatic
        );
    }

    #[test]
    fn test_fact_volatility_classification_dynamic() {
        assert_eq!(
            classify_fact_volatility("ansible_memfree_mb"),
            FactVolatility::Dynamic
        );
        assert_eq!(
            classify_fact_volatility("ansible_loadavg"),
            FactVolatility::Dynamic
        );
    }

    #[test]
    fn test_fact_volatility_classification_volatile() {
        assert_eq!(
            classify_fact_volatility("ansible_date_time"),
            FactVolatility::Volatile
        );
        assert_eq!(
            classify_fact_volatility("ansible_uptime_seconds"),
            FactVolatility::Volatile
        );
    }

    #[test]
    fn test_volatility_ttl() {
        assert!(FactVolatility::Static.recommended_ttl() > Duration::from_secs(1800));
        assert!(FactVolatility::SemiStatic.recommended_ttl() >= Duration::from_secs(300));
        assert!(FactVolatility::Dynamic.recommended_ttl() >= Duration::from_secs(30));
        assert_eq!(FactVolatility::Volatile.recommended_ttl(), Duration::ZERO);
    }

    #[test]
    fn test_volatility_should_cache() {
        assert!(FactVolatility::Static.should_cache());
        assert!(FactVolatility::SemiStatic.should_cache());
        assert!(FactVolatility::Dynamic.should_cache());
        assert!(!FactVolatility::Volatile.should_cache());
    }

    #[test]
    fn test_volatility_preferred_tier() {
        assert_eq!(FactVolatility::Static.preferred_tier(), CacheTier::L3Cold);
        assert_eq!(
            FactVolatility::SemiStatic.preferred_tier(),
            CacheTier::L2Warm
        );
        assert_eq!(FactVolatility::Dynamic.preferred_tier(), CacheTier::L1Hot);
    }

    #[test]
    fn test_partitioned_facts_from_flat() {
        let facts = sample_facts();
        let partitioned = PartitionedFacts::from_flat("server1", facts, vec!["all".to_string()]);

        assert!(partitioned.static_facts.contains_key("ansible_os_family"));
        assert!(partitioned
            .semi_static_facts
            .contains_key("ansible_hostname"));
        assert!(partitioned.dynamic_facts.contains_key("ansible_memfree_mb"));
        assert!(partitioned.volatile_facts.contains_key("ansible_date_time"));
    }

    #[test]
    fn test_partitioned_facts_to_flat() {
        let facts = sample_facts();
        let partitioned =
            PartitionedFacts::from_flat("server1", facts.clone(), vec!["all".to_string()]);
        let flattened = partitioned.to_flat();

        assert_eq!(flattened.len(), facts.len());
        assert!(flattened.contains_key("ansible_os_family"));
        assert!(flattened.contains_key("ansible_hostname"));
    }

    #[test]
    fn test_partitioned_facts_get() {
        let facts = sample_facts();
        let partitioned = PartitionedFacts::from_flat("server1", facts, vec!["all".to_string()]);

        assert!(partitioned.get("ansible_os_family").is_some());
        assert!(partitioned.get("ansible_hostname").is_some());
        assert!(partitioned.get("ansible_memfree_mb").is_some());
        assert!(partitioned.get("nonexistent").is_none());
    }

    #[test]
    fn test_tiered_cache_basic() {
        let config = TieredCacheConfig::development();
        let cache = TieredFactCache::new(config);

        cache.insert("host1", sample_facts(), vec!["all".to_string()]);

        let cached = cache.get("host1").unwrap();
        assert!(cached.contains_key("ansible_os_family"));
    }

    #[test]
    fn test_tiered_cache_invalidation() {
        let config = TieredCacheConfig::development();
        let cache = TieredFactCache::new(config);

        cache.insert("host1", sample_facts(), vec![]);
        cache.insert("host2", sample_facts(), vec![]);

        assert_eq!(cache.len(), 2);

        cache.invalidate("host1");

        assert!(cache.get("host1").is_none());
        assert!(cache.get("host2").is_some());
    }

    #[test]
    fn test_tiered_cache_clear() {
        let config = TieredCacheConfig::development();
        let cache = TieredFactCache::new(config);

        cache.insert("host1", sample_facts(), vec![]);
        cache.insert("host2", sample_facts(), vec![]);

        cache.clear();

        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_tiered_cache_metrics() {
        let config = TieredCacheConfig::development();
        let cache = TieredFactCache::new(config);

        // Miss
        assert!(cache.get("nonexistent").is_none());

        let metrics = cache.metrics();
        assert_eq!(metrics.l1_misses.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.l3_misses.load(Ordering::Relaxed), 1);

        // Insert and hit
        cache.insert("host1", sample_facts(), vec![]);
        assert!(cache.get("host1").is_some());
        assert_eq!(metrics.l1_hits.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_tiered_expiry() {
        let expiry = TieredExpiry::new();

        // Nothing should be expired immediately
        assert!(!expiry.static_expired());
        assert!(!expiry.semi_static_expired());
        assert!(!expiry.dynamic_expired());
        assert!(!expiry.all_expired());
    }

    #[test]
    fn test_tiered_expiry_needs_refresh() {
        let expiry = TieredExpiry::new();
        let needs = expiry.needs_refresh();

        // Nothing should need refresh immediately
        assert!(needs.is_empty());
    }

    #[test]
    fn test_tiered_cache_tier_counts() {
        let config = TieredCacheConfig::development();
        let cache = TieredFactCache::new(config);

        cache.insert("host1", sample_facts(), vec![]);

        let (l1, _l2, l3) = cache.tier_counts();
        assert_eq!(l1, 1);
        assert_eq!(l3, 0);
    }

    #[test]
    fn test_cache_tier_latency() {
        assert!(CacheTier::L1Hot.typical_latency() < CacheTier::L2Warm.typical_latency());
    }

    #[test]
    fn test_tiered_cache_get_by_volatility() {
        let config = TieredCacheConfig::development();
        let cache = TieredFactCache::new(config);

        cache.insert("host1", sample_facts(), vec![]);

        // Get only static facts
        let static_facts = cache.get_by_volatility("host1", &[FactVolatility::Static]);
        assert!(static_facts.is_some());
        let facts = static_facts.unwrap();
        assert!(facts.contains_key("ansible_os_family"));
        assert!(!facts.contains_key("ansible_memfree_mb")); // Dynamic, not included
    }
}

// ============================================================================
// Playbook Cache Tests
// ============================================================================

mod playbook_cache_tests {
    use super::*;
    use rustible::cache::playbook::CachedPlaybook;
    use rustible::executor::playbook::{Play, Playbook};

    fn sample_playbook() -> Playbook {
        let mut playbook = Playbook::new("test-playbook");
        playbook.plays.push(Play::new("Test Play", "all"));
        playbook
    }

    #[test]
    fn test_playbook_cache_inline() {
        let cache = PlaybookCache::new(CacheConfig::default());

        let content = r#"
        - name: Test Play
          hosts: all
          tasks: []
        "#;

        cache.insert_inline(content, sample_playbook());

        let cached = cache.get_by_content(content).unwrap();
        assert_eq!(cached.name, "test-playbook");
    }

    #[test]
    fn test_playbook_cache_clear() {
        let cache = PlaybookCache::new(CacheConfig::default());

        let content = "- name: Test\n  hosts: all";
        cache.insert_inline(content, sample_playbook());

        assert!(!cache.is_empty());

        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cached_playbook_size() {
        let cached = CachedPlaybook::new(sample_playbook(), None);
        let size = cached.size_bytes();

        // Should have some reasonable size
        assert!(size > 0);
        assert!(size < 10000); // Shouldn't be huge for a simple playbook
    }

    #[test]
    fn test_cached_playbook_with_parse_time() {
        let cached = CachedPlaybook::new(sample_playbook(), None).with_parse_time(150);

        assert_eq!(cached.parse_time_ms, 150);
    }

    #[test]
    fn test_cached_playbook_add_dependency() {
        let mut cached = CachedPlaybook::new(sample_playbook(), None);

        cached.add_dependency(PathBuf::from("/path/to/role"));
        cached.add_dependency(PathBuf::from("/path/to/vars.yml"));

        assert_eq!(cached.dependencies.len(), 2);

        // Adding duplicate should not increase count
        cached.add_dependency(PathBuf::from("/path/to/role"));
        assert_eq!(cached.dependencies.len(), 2);
    }
}

// ============================================================================
// Role Cache Tests
// ============================================================================

mod role_cache_tests {
    use super::*;
    use rustible::cache::role::{CachedRole, RoleCacheKey, RoleMetadata};
    use rustible::executor::playbook::Role;

    fn sample_role() -> Role {
        Role::new("test-role")
    }

    #[test]
    fn test_role_cache_basic() {
        let cache = RoleCache::new(CacheConfig::default());
        let key = RoleCacheKey::simple("test-role");

        cache.insert(key.clone(), sample_role());

        let cached = cache.get(&key).unwrap();
        assert_eq!(cached.name, "test-role");
    }

    #[test]
    fn test_role_cache_key_with_path() {
        let key = RoleCacheKey::with_path("my-role", PathBuf::from("/path/to/role"));

        assert_eq!(key.name, "my-role");
        assert_eq!(key.path, Some(PathBuf::from("/path/to/role")));
    }

    #[test]
    fn test_role_cache_key_with_overrides() {
        let key = RoleCacheKey::with_overrides(
            "my-role",
            Some(PathBuf::from("/path/to/role")),
            Some("install.yml".to_string()),
            None,
            None,
        );

        assert_eq!(key.name, "my-role");
        assert_eq!(key.tasks_from, Some("install.yml".to_string()));
        assert!(key.vars_from.is_none());
    }

    #[test]
    fn test_role_cache_invalidate_by_name() {
        let cache = RoleCache::new(CacheConfig::default());

        cache.insert(RoleCacheKey::simple("role1"), sample_role());
        cache.insert(RoleCacheKey::simple("role2"), sample_role());

        cache.invalidate_by_name("role1");

        assert!(cache.get(&RoleCacheKey::simple("role1")).is_none());
        assert!(cache.get(&RoleCacheKey::simple("role2")).is_some());
    }

    #[test]
    fn test_role_cache_clear() {
        let cache = RoleCache::new(CacheConfig::default());

        cache.insert(RoleCacheKey::simple("role1"), sample_role());
        cache.insert(RoleCacheKey::simple("role2"), sample_role());

        assert_eq!(cache.len(), 2);

        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_role_cache_role_names() {
        let cache = RoleCache::new(CacheConfig::default());

        cache.insert(RoleCacheKey::simple("role-a"), sample_role());
        cache.insert(RoleCacheKey::simple("role-b"), sample_role());
        cache.insert(RoleCacheKey::simple("role-c"), sample_role());

        let names = cache.role_names();
        assert_eq!(names.len(), 3);
    }

    #[test]
    fn test_cached_role_size() {
        let cached = CachedRole::new(sample_role(), None);
        let size = cached.size_bytes();

        assert!(size > 0);
        assert!(size < 10000);
    }

    #[test]
    fn test_cached_role_with_load_time() {
        let cached = CachedRole::new(sample_role(), None).with_load_time(250);

        assert_eq!(cached.load_time_ms, 250);
    }

    #[test]
    fn test_cached_role_with_metadata() {
        let metadata = RoleMetadata {
            author: Some("Test Author".to_string()),
            description: Some("A test role".to_string()),
            ..Default::default()
        };

        let cached = CachedRole::new(sample_role(), None).with_metadata(metadata);

        assert_eq!(cached.metadata.author, Some("Test Author".to_string()));
    }

    #[test]
    fn test_role_cache_insert_with_timing() {
        let cache = RoleCache::new(CacheConfig::default());
        let key = RoleCacheKey::simple("test-role");

        cache.insert_with_timing(key.clone(), sample_role(), 100);

        assert!(cache.get(&key).is_some());
    }
}

// ============================================================================
// Cache Manager Tests
// ============================================================================

mod cache_manager_tests {
    use super::*;

    #[test]
    fn test_cache_manager_creation() {
        let manager = CacheManager::new();

        assert_eq!(manager.facts.len(), 0);
        assert_eq!(manager.playbooks.len(), 0);
        assert_eq!(manager.roles.len(), 0);
        assert_eq!(manager.variables.len(), 0);
    }

    #[test]
    fn test_cache_manager_with_config() {
        let config = CacheConfig::production();
        let manager = CacheManager::with_config(config);

        // Manager should be created with production settings
        assert_eq!(manager.facts.len(), 0);
    }

    #[test]
    fn test_cache_manager_disabled() {
        let manager = CacheManager::disabled();

        // Insert should not store anything
        manager.facts.insert_raw("host1", IndexMap::new());
        assert_eq!(manager.facts.len(), 0);
    }

    #[test]
    fn test_cache_manager_status() {
        let manager = CacheManager::new();

        let status = manager.status();
        assert!(status.enabled);
        assert_eq!(status.total_entries, 0);
    }

    #[test]
    fn test_cache_manager_metrics() {
        let manager = CacheManager::new();

        let summary = manager.metrics();

        // Initial metrics should be zero
        assert_eq!(summary.facts.hits.load(Ordering::Relaxed), 0);
        assert_eq!(summary.playbooks.hits.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_cache_manager_overall_hit_rate() {
        let manager = CacheManager::new();

        // Insert and access
        manager.facts.insert_raw("host1", IndexMap::new());
        manager.facts.get("host1"); // Hit
        manager.facts.get("nonexistent"); // Miss

        let summary = manager.metrics();
        let hit_rate = summary.overall_hit_rate();

        // Should be 0.5 (1 hit, 1 miss)
        assert!((hit_rate - 0.5).abs() < 0.01);
    }
}

// ============================================================================
// Cache Metrics Tests
// ============================================================================

mod cache_metrics_tests {
    use super::*;

    #[test]
    fn test_cache_metrics_hit_recording() {
        let metrics = CacheMetrics::new();

        metrics.record_hit();
        metrics.record_hit();
        metrics.record_miss();

        assert_eq!(metrics.hits.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.misses.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_cache_metrics_hit_rate() {
        let metrics = CacheMetrics::new();

        metrics.record_hit();
        metrics.record_hit();
        metrics.record_hit();
        metrics.record_miss();

        assert!((metrics.hit_rate() - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_cache_metrics_hit_rate_zero() {
        let metrics = CacheMetrics::new();

        // No hits or misses
        assert_eq!(metrics.hit_rate(), 0.0);
    }

    #[test]
    fn test_cache_metrics_eviction_tracking() {
        let metrics = CacheMetrics::new();

        metrics.record_eviction();
        metrics.record_eviction();

        assert_eq!(metrics.evictions.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_cache_metrics_invalidation_tracking() {
        let metrics = CacheMetrics::new();

        metrics.record_invalidation();

        assert_eq!(metrics.invalidations.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_cache_metrics_reset() {
        let metrics = CacheMetrics::new();

        metrics.record_hit();
        metrics.record_miss();
        metrics.record_eviction();

        metrics.reset();

        assert_eq!(metrics.hits.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.misses.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.evictions.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_cache_metrics_summary() {
        let metrics = CacheMetrics::new();

        metrics.record_hit();
        metrics.record_miss();

        let summary = metrics.summary();
        assert!(summary.contains("Hits: 1"));
        assert!(summary.contains("Misses: 1"));
        assert!(summary.contains("Hit Rate: 50.00%"));
    }
}

// ============================================================================
// Cache Config Tests
// ============================================================================

mod cache_config_tests {
    use super::*;

    #[test]
    fn test_cache_config_default() {
        let config = CacheConfig::default();

        assert_eq!(config.default_ttl, Duration::from_secs(300));
        assert_eq!(config.max_entries, 10_000);
        assert!(config.track_dependencies);
        assert!(config.enable_metrics);
    }

    #[test]
    fn test_cache_config_development() {
        let config = CacheConfig::development();

        assert!(config.default_ttl < CacheConfig::default().default_ttl);
        assert!(config.max_entries < CacheConfig::default().max_entries);
    }

    #[test]
    fn test_cache_config_production() {
        let config = CacheConfig::production();

        assert!(config.default_ttl >= CacheConfig::default().default_ttl);
        assert!(config.max_entries >= CacheConfig::default().max_entries);
    }

    #[test]
    fn test_cache_config_disabled() {
        let config = CacheConfig::disabled();

        assert_eq!(config.default_ttl, Duration::ZERO);
        assert_eq!(config.max_entries, 0);
        assert!(!config.track_dependencies);
        assert!(!config.enable_metrics);
    }
}

// ============================================================================
// Cache Dependency Tests
// ============================================================================

mod cache_dependency_tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_cache_dependency_file_exists() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "test content").unwrap();

        let dep = CacheDependency::file(file_path.clone());
        assert!(dep.is_some());

        let dep = dep.unwrap();
        assert!(!dep.is_invalidated());
    }

    #[test]
    fn test_cache_dependency_file_not_exists() {
        let dep = CacheDependency::file(PathBuf::from("/nonexistent/path/file.txt"));
        assert!(dep.is_none());
    }

    #[test]
    fn test_cache_dependency_file_modified() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "test content").unwrap();

        let dep = CacheDependency::file(file_path.clone()).unwrap();

        // Modify the file
        thread::sleep(Duration::from_millis(100)); // Ensure time difference
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "modified content").unwrap();

        assert!(dep.is_invalidated());
    }

    #[test]
    fn test_cache_dependency_cache_key() {
        let dep = CacheDependency::cache_key(CacheType::Facts, "host1");

        match dep {
            CacheDependency::CacheKey { cache_type, key } => {
                assert_eq!(cache_type, CacheType::Facts);
                assert_eq!(key, "host1");
            }
            _ => panic!("Expected CacheKey variant"),
        }
    }
}

// ============================================================================
// Concurrent Access Tests
// ============================================================================

mod concurrent_access_tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn test_concurrent_cache_access() {
        let cache: Arc<Cache<String, String>> =
            Arc::new(Cache::new(CacheType::Facts, CacheConfig::default()));

        let num_threads = 10;
        let ops_per_thread = 100;
        let counter = Arc::new(AtomicUsize::new(0));

        let handles: Vec<_> = (0..num_threads)
            .map(|thread_id| {
                let cache = Arc::clone(&cache);
                let counter = Arc::clone(&counter);

                thread::spawn(move || {
                    for op in 0..ops_per_thread {
                        let key = format!("key-{}-{}", thread_id, op);
                        let value = format!("value-{}-{}", thread_id, op);

                        cache.insert(key.clone(), value.clone(), 10);

                        if cache.get(&key).is_some() {
                            counter.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        // Most gets should succeed (some might race with evictions)
        let successful_gets = counter.load(Ordering::Relaxed);
        assert!(successful_gets > (num_threads * ops_per_thread) / 2);
    }

    #[test]
    fn test_concurrent_fact_cache_access() {
        let cache = Arc::new(FactCache::new(CacheConfig::default()));

        let handles: Vec<_> = (0..5)
            .map(|thread_id| {
                let cache = Arc::clone(&cache);

                thread::spawn(move || {
                    for i in 0..20 {
                        let hostname = format!("host-{}-{}", thread_id, i);
                        let mut facts = IndexMap::new();
                        facts.insert("thread".to_string(), json!(thread_id));
                        facts.insert("iteration".to_string(), json!(i));

                        cache.insert_raw(&hostname, facts);
                        cache.get(&hostname);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        // Cache should have entries from all threads
        assert!(!cache.is_empty());
    }
}
