## 2024-01-12 - HINT vs INFO for actionable feedback
**Learning:** Users often miss actionable advice when it's buried in standard INFO logs, especially when running with default verbosity. Elevating these suggestions to a distinct "HINT" level (styled in cyan) makes them pop out without being alarming like warnings or errors.
**Action:** When providing specific, actionable fixes (like "Use X instead of Y"), use `ctx.output.hint()` instead of `ctx.output.info()`.
