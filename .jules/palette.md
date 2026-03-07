# Palette's Journal

## 2024-05-22 - CLI UX Constraints
**Learning:** This project is a CLI tool (Rustible) that mimics Ansible. It explicitly prohibits emojis and brackets in status labels to maintain a clean, professional, and Ansible-compatible output style.
**Action:** When improving UX, focus on text clarity, color (using `colored` crate), and helpful error messages rather than graphical elements or emojis.

## 2024-05-22 - Progress Indication
**Learning:** The project uses `indicatif` for progress bars. Users value standard progress reporting.
**Action:** Ensure long-running operations have progress bars or spinners where appropriate, but respect the "no-emoji" rule.

## 2024-05-22 - Interactive Multi-input Feedback
**Learning:** For interactive command-line loops (like `get_extra_vars`), users need immediate feedback after entering a valid item, and a clear view of accumulated items. Dimming previous inputs and bolding new success items improves readability.
**Action:** When implementing multi-input prompts, always display the accumulated list (dimmed) and show an immediate success confirmation for the most recent entry.
