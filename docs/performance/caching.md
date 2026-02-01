---
summary: Overview of Rustible caching layers and how to tune them.
read_when: You need to understand or tune caching for performance.
---

# Caching

Rustible ships with several caching layers to reduce repeated work during runs:

- **Template cache**: LRU cache for rendered templates and expressions.
- **Inventory cache**: Optional in-memory caching for parsed inventories.
- **Playbook cache**: Parsed playbooks cached with dependency tracking.
- **Variable cache**: Resolved variable maps cached per host/play.
- **Role cache**: Cached role loads with invalidation on file changes.

## Tuning

The caching system is configured through `CacheConfig` and specific cache configs.
Defaults are optimized for short runs, while `CacheConfig::production()` targets
long-lived sessions with larger caches.

## Notes

- Cache entries expire via TTL and file dependency invalidation.
- Caches are currently memory-resident; on-disk cache persistence is planned.
