# Bolt's Journal

## 2024-03-24 - [Optimized Regex Filters in Templates]
**Learning:** The `regex_replace` and `regex_search` filters in `src/template.rs` were recompiling regexes on every call using `regex::Regex::new`. This is a significant performance bottleneck, especially in loops or when processing large templates. The project already had a thread-safe regex cache available via `crate::utils::get_regex` which uses `DashMap` and `once_cell`.
**Action:** Always check for existing caching mechanisms before implementing new ones. When working with regexes in hot paths (like filters), ensure they are cached. Using `crate::utils::get_regex` reduced execution time by ~70% (from ~4.9µs to ~1.4µs per call).
## 2024-05-22 - [Lazy Initialization of Template Engines]
**Learning:** Instantiating `TemplateEngine` (wrapping `minijinja::Environment`) is expensive.
**Action:** Use `once_cell` or `lazy_static` to create instances once and reuse them, especially in modules that are called frequently.

## 2024-05-23 - [Regex Compilation in Loops]
**Learning:** Recompiling `Regex` in a loop (even indirectly via helper functions) is a significant performance anti-pattern in Rust, as regex compilation is expensive.
**Action:** When a module accepts a regex parameter that is used in a loop (like `wait_for` checking a file), compile the regex once during parameter parsing or validation and pass the compiled `Regex` object to the loop/helper functions.

## 2024-05-24 - [Unnecessary String Cloning in Serialization]
**Learning:** Creating intermediate `String` objects (via `clone()` or `to_string()`) just to store them in a `Vec<String>` for a final `join()` operation is wasteful.
**Action:** Use `Vec<&str>` or `Vec<Cow<str>>` to hold references to existing strings, and pre-allocate the vector with `Vec::with_capacity()` when the number of elements is known. This avoids heap allocations for temporary strings.

## 2024-05-25 - [Cow into_owned trap]
**Learning:** Calling `.into_owned()` on a `Cow<'_, str>` returned from a helper function (like `shell_escape`) forces allocation even if the helper returned `Cow::Borrowed`. When collecting into a `Vec`, ensure the target type is `Vec<Cow<'_, str>>` and avoid early conversion to `String` to preserve zero-allocation benefits for safe strings.
**Action:** Use `Cow` throughout the transformation chain where possible.

## 2024-05-25 - [Lineinfile Optimization]
**Learning:** `lineinfile` module was unconditionally cloning the file content for diff generation even when diffs were not requested. It also allocated a vector of matching indices for regex replacement.
**Action:** Guard expensive clones with feature flags (like `diff_mode`). Use in-place iteration or single-pass search (`position`, `find`) instead of `collect()` + loop when modifying data, especially for "first match" scenarios.

## 2024-05-26 - [File Module Syscall Reduction]
**Learning:** Recursive file operations using `walkdir` can trigger excessive syscalls if internal helpers (`set_permissions`, `set_owner`) re-stat the file. Furthermore, checking global system state (like `sestatus`) inside a file loop spawns a subprocess for every file, causing massive slowdowns.
**Action:** Reuse `metadata` obtained from `walkdir` or a single `stat` call across multiple attribute setters. Cache system checks (like SELinux status) outside the recursive loop.

## 2024-05-27 - [Optimized Context Serialization]
**Learning:** Using `BTreeSet<&str>` to sort and deduplicate keys for deterministic serialization of merged contexts is significantly slower (~15%) than using `Vec<&str>` with `sort_unstable` and `dedup`, due to the overhead of tree node allocations and pointer chasing.
**Action:** When preparing a list of keys for serialization where the number of keys is moderate to large (e.g. merging vars + facts), prefer flat `Vec` with `sort_unstable` and `dedup` over `BTreeSet`. Pre-calculate capacity using `Vec::with_capacity` to avoid reallocations.

## 2024-05-24 - [Optimize template_string]
**Learning:** `template_string` function in `src/cli/commands/run.rs` was recompiling `Regex::new(r"\{\{\s*([^}]+?)\s*\}\}").unwrap()` every time it was called, resulting in huge performance overhead. Caching the regex and performing a fast check avoids significant processing time.
**Action:** Always check if regexes in frequently-used paths can be static or cached, and perform early string content checks if applicable.
