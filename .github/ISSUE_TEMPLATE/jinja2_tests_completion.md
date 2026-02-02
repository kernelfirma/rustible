#179 [MEDIUM] Implement Jinja2 Tests for Feature Parity

## Problem Statement
Rustible's template engine (MiniJinja) does not implement Jinja2 tests (`is defined`, `is none`, `is string`, etc.). This forces users to use workarounds and breaks compatibility with existing Ansible playbooks that rely on these tests.

## Current State
| Test | Status | Ansible Usage |
|------|--------|---------------|
| `is defined` | ❌ Not implemented | Very common - variable existence check |
| `is undefined` | ❌ Not implemented | Common - variable absence check |
| `is none` / `is null` | ❌ Not implemented | Common - null checking |
| `is string` | ❌ Not implemented | Medium - type checking |
| `is number` | ❌ Not implemented | Medium - type checking |
| `is mapping` / `is dict` | ❌ Not implemented | Medium - type checking |
| `is sequence` / `is list` | ❌ Not implemented | Medium - type checking |
| `is iterable` | ❌ Not implemented | Medium - loop compatibility |
| `is callable` | ❌ Not implemented | Low - function checking |
| `is sameas` | ❌ Not implemented | Low - identity checking |
| `is boolean` | ❌ Not implemented | Medium - type checking |
| `is filter` / `is test` | ❌ Not implemented | Low - introspection |
| `is odd` / `is even` | ❌ Not implemented | Low - numeric tests |
| `is divisibleby` | ❌ Not implemented | Low - numeric tests |
| `is escaped` | ❌ Not implemented | Low - security check |
| `is in` | ⚠️ Partial | Collection membership |
| `is subset` / `is superset` | ❌ Not implemented | Medium - set operations |

## Impact on Ansible Compatibility
These tests are used in many Ansible playbooks:

```yaml
# Common pattern that FAILS in Rustible
- name: Check if variable is defined
  debug:
    msg: "Variable exists"
  when: my_var is defined

# Type checking that FAILS
- name: Ensure value is a list
  debug:
    msg: "Is a list"
  when: my_list is list

# Null checking that FAILS
- name: Check for null
  debug:
    msg: "Value is null"
  when: my_var is none
```

## Workarounds Required
Users currently must use verbose alternatives:
```yaml
# Instead of: when: my_var is defined
when: my_var | default(None) != None

# Instead of: when: my_list is list
type_debug: "{{ my_list | type_debug }}"
when: type_debug == 'list'
```

## Proposed Implementation

### Option 1: MiniJinja Native Tests
```rust
// src/template/tests.rs
use minijinja::Environment;

pub fn register_tests(env: &mut Environment) {
    // Type tests
    env.add_test("string", |v| v.is_string());
    env.add_test("number", |v| v.is_number());
    env.add_test("mapping", |v| v.is_object());
    env.add_test("sequence", |v| v.is_sequence());
    env.add_test("iterable", |v| v.is_iterable());
    env.add_test("callable", |v| v.is_callable());
    
    // Existence tests (special handling required)
    env.add_test("defined", |v, state| !v.is_undefined());
    env.add_test("undefined", |v| v.is_undefined());
    
    // Null tests
    env.add_test("none", |v| v.is_none());
    env.add_test("null", |v| v.is_none());
    env.add_test("boolean", |v| v.is_bool());
    
    // Collection tests
    env.add_test("in", |item, container| container.contains(item));
    env.add_test("subset", |subset, superset| is_subset(subset, superset));
    env.add_test("superset", |superset, subset| is_superset(superset, subset));
    
    // Numeric tests
    env.add_test("odd", |v| v.as_i64().map(|n| n % 2 != 0).unwrap_or(false));
    env.add_test("even", |v| v.as_i64().map(|n| n % 2 == 0).unwrap_or(false));
    env.add_test("divisibleby", |v, n| is_divisible_by(v, n));
    
    // Identity
    env.add_test("sameas", |v, other| v == other);
}
```

### Option 2: Custom Template Engine Enhancement
If MiniJinja limitations prevent full implementation, enhance the template preprocessing:

```rust
// Pre-process templates to convert Jinja2 tests
fn preprocess_jinja2_tests(template: &str) -> String {
    // Convert: {% if var is defined %}
    // To:      {% if var | default(UndefinedSentinel) != UndefinedSentinel %}
    
    // Convert: {% if var is list %}
    // To:      {% if var | type_debug == 'list' %}
}
```

### Implementation Tasks
- [ ] Implement `is defined` / `is undefined` tests
- [ ] Implement `is none` / `is null` tests
- [ ] Implement type tests (`string`, `number`, `mapping`, `sequence`, `iterable`)
- [ ] Implement collection tests (`in`, `subset`, `superset`)
- [ ] Implement numeric tests (`odd`, `even`, `divisibleby`)
- [ ] Implement `is boolean` test
- [ ] Implement `is sameas` test
- [ ] Add test for `is filter` and `is test` introspection
- [ ] Update documentation with supported tests
- [ ] Add compatibility tests against Ansible behavior

## Test Compatibility Matrix
| Test | Ansible Behavior | Rustible Target |
|------|-----------------|-----------------|
| `is defined` | Variable exists in context | Match exactly |
| `is undefined` | Variable does not exist | Match exactly |
| `is none` | Value is Python None | Value is JSON null |
| `is string` | isinstance(v, str) | v.is_string() |
| `is mapping` | isinstance(v, dict) | v.is_object() |
| `is sequence` | isinstance(v, list) | v.is_array() |
| `is iterable` | hasattr(v, '__iter__') | v.is_iterable() |

## Acceptance Criteria
- [ ] All common tests (`defined`, `undefined`, `none`, `string`, `mapping`) work
- [ ] Existing Ansible playbooks using these tests work without modification
- [ ] Test behavior matches Ansible Jinja2 behavior
- [ ] Performance impact <5% on template rendering
- [ ] Documentation lists all supported tests

## Priority
**MEDIUM** - Important for Ansible compatibility; workarounds exist

## Related
- Template engine: `src/template.rs` (MiniJinja)
- MiniJinja docs: https://docs.rs/minijinja/

## Labels
`medium`, `ansible-compatible`, `templates`, `feature-parity`
