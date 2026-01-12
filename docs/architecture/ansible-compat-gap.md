# Ansible Compatibility Gap Plan

## Status
Draft

## Problem Statement
Rustible is close to Ansible compatibility but still missing edge cases in module behavior, filter coverage, boolean coercion, block parsing, and FQCN handling.

## Goals
- Inventory missing behaviors and prioritize by usage.
- Close Jinja2 filter/test gaps.
- Address listed edge cases (boolean compat, block parsing, FQCN, CLI behavior).
- Update compatibility matrix and add conformance tests.

## Workstreams
### 1) Module Behavior Inventory
- Create a matrix for the top 20 modules by usage.
- Add integration tests comparing Rustible output to Ansible output.
- Tag each gap with severity and user impact.

### 2) Jinja2 Filter/Test Coverage
- Implement remaining filters to match Ansible semantics.
- Add filter-specific golden tests with Ansible fixtures.

### 3) Edge Case Fixes
- Boolean compatibility: accept Ansible truthy/falsey strings.
- Block parsing: ensure nested block/rescue/always order matches Ansible.
- FQCN handling: resolve `ansible.builtin.*` to built-in modules.
- CLI parity: align flags and default behaviors with `ansible-playbook`.

### 4) Compatibility Matrix
- Update docs/compat matrix after each milestone.
- Track versioned status (v0.2, v0.3).

## Next Steps
- Add a compatibility test harness to `tests/compat/`.
- Produce a v0.2 gap list with owners and timelines.
