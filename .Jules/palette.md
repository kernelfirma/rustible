## 2024-05-23 - CLI Silence is Golden (until it isn't)
**Learning:** CLI tools that rely solely on `tracing` or logging libraries for output often default to silence (e.g., only showing warnings/errors), leaving users wondering if the tool is working. Hooking internal execution events to a dedicated `OutputFormatter` provides necessary visual feedback without compromising the logging strategy.
**Action:** When working on CLI tools, ensure there's a default "user-facing" output stream separate from debug logs, and wire it up to execution events early.
