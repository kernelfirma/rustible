## 2024-01-12 - HINT vs INFO for actionable feedback
**Learning:** Users often miss actionable advice when it's buried in standard INFO logs, especially when running with default verbosity. Elevating these suggestions to a distinct "HINT" level (styled in cyan) makes them pop out without being alarming like warnings or errors.
**Action:** When providing specific, actionable fixes (like "Use X instead of Y"), use `ctx.output.hint()` instead of `ctx.output.info()`.

## 2024-05-24 - [Improved Debug Module Visibility]
**Learning:** CLI tools often hide module-specific output in "simplified" execution paths unless explicitly handled. Users coming from Ansible expect `debug` output to be visible inline with the task status, not buried in verbose logs.
**Action:** When implementing CLI task runners, ensure that task results can propagate an optional "message" field that is displayed alongside the status (e.g., `ok: [host] => message`), even for non-failure states.

## 2024-05-24 - [CLI Progress Indicators]
**Learning:** `indicatif` progress bars in `OutputFormatter` require explicit initialization via `init_progress()` before they can be used. Without this call, `create_spinner()` returns `None` or fails silently, leaving the user staring at a static screen during long operations.
**Action:** Always call `ctx.output.init_progress()` at the start of any long-running CLI command execution (like `run` or `provision`) to enable visual feedback.

## 2024-05-27 - [Aligned Task Status Labels]
**Learning:** In CLI outputs with repetitive structured data (like task statuses), inconsistent label widths (e.g., "ok" vs "unreachable") create a "jagged" edge that increases cognitive load when scanning.
**Action:** Use fixed-width padding for status labels to align subsequent data (hostnames) vertically, improving readability and scanability.

## 2026-01-19 - [Semantic Emojis in Interactive Menus]
**Learning:** Pure text menus in CLIs can feel dense and hard to scan quickly. Adding relevant semantic emojis (e.g., 🚀 for run, 🔍 for check) serves as a visual anchor that improves scanability and adds a touch of modern delight.
**Action:** When designing `dialoguer` menus, prefix items with consistent, semantic emojis. Ensure spacing accounts for emoji width (often 2 chars) to maintain visual alignment.

## 2024-05-22 - Consistent List Item Icons
**Learning:** Adding consistent semantic emojis (e.g., 📖, 📄, 🏷️) to interactive list items significantly improves scannability and visual consistency with the main menu.
**Action:** Always verify that interactive selection lists (Select, MultiSelect) use consistent icon prefixes for items, especially if the parent menu uses them.

## 2024-05-28 - [Unicode vs Emoji Usage]
**Learning:** The codebase avoids emojis in banners (e.g., `[==== SUCCESS ====]`) but uses them in interactive menus. For CLI output like task status, text-like Unicode symbols (e.g. `✎` instead of `📝`) are preferred to maintain alignment and professional appearance.
**Action:** Use Unicode symbols that are 1-cell wide for tabular output; reserve colorful emojis for interactive prompts.
