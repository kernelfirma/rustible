# Bolt's Journal

## 2024-03-24 - [Optimized Regex Filters in Templates]
**Learning:** The `regex_replace` and `regex_search` filters in `src/template.rs` were recompiling regexes on every call using `regex::Regex::new`. This is a significant performance bottleneck, especially in loops or when processing large templates. The project already had a thread-safe regex cache available via `crate::utils::get_regex` which uses `DashMap` and `once_cell`.
**Action:** Always check for existing caching mechanisms before implementing new ones. When working with regexes in hot paths (like filters), ensure they are cached. Using `crate::utils::get_regex` reduced execution time by ~70% (from ~4.9µs to ~1.4µs per call).
