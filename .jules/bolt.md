# Bolt's Journal

## 2024-05-22 - [Lazy Initialization of Template Engines]
**Learning:** Instantiating `TemplateEngine` (wrapping `minijinja::Environment`) is expensive.
**Action:** Use `once_cell` or `lazy_static` to create instances once and reuse them, especially in modules that are called frequently.

## 2024-05-23 - [Regex Compilation in Loops]
**Learning:** Recompiling `Regex` in a loop (even indirectly via helper functions) is a significant performance anti-pattern in Rust, as regex compilation is expensive.
**Action:** When a module accepts a regex parameter that is used in a loop (like `wait_for` checking a file), compile the regex once during parameter parsing or validation and pass the compiled `Regex` object to the loop/helper functions.
