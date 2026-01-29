# Jinja2 Filter Compatibility

> **Last Updated:** 2026-01-26
> **Rustible Version:** 0.1.x

This document tracks the compatibility between Ansible's Jinja2 filters and Rustible's MiniJinja-based template engine.

---

## Summary

| Category | Ansible | Rustible | Coverage |
|----------|---------|----------|----------|
| String Filters | 25+ | 11 | ~44% |
| List Filters | 20+ | 10 | ~50% |
| Dict Filters | 10+ | 5 | ~50% |
| Math Filters | 10+ | Built-in | MiniJinja native |
| Type Conversion | 6 | 5 | 83% |
| Path Filters | 6 | 4 | 67% |
| Encoding Filters | 6 | 8 | 100%+ |
| Ansible-Specific | 15+ | 8 | ~53% |

---

## Implemented Filters

### String Filters

| Filter | Ansible | Rustible | Notes |
|--------|---------|----------|-------|
| `default` / `d` | Yes | Yes | Full support including boolean parameter |
| `lower` | Yes | Yes | |
| `upper` | Yes | Yes | |
| `capitalize` | Yes | Yes | |
| `title` | Yes | Yes | |
| `trim` | Yes | Yes | |
| `replace` | Yes | Yes | |
| `regex_replace` | Yes | Yes | |
| `regex_search` | Yes | Yes | |
| `split` | Yes | Yes | |
| `join` | Yes | Yes | |
| `quote` | Yes | Yes | Shell quoting |
| `systemd_escape` | Yes | Yes | Rustible-specific |

### Not Yet Implemented (String)

| Filter | Priority | Notes |
|--------|----------|-------|
| `center` | Low | Text centering |
| `ljust` / `rjust` | Low | Text justification |
| `wordwrap` | Low | Word wrapping |
| `truncate` | Medium | Truncate with ellipsis |
| `urlsplit` | Medium | URL parsing |
| `urlencode` / `urldecode` | Medium | URL encoding |
| `indent` | Medium | Text indentation |
| `comment` | Low | Add comment markers |
| `human_readable` | Low | Human-readable sizes |
| `human_to_bytes` | Low | Parse human sizes |

### List/Sequence Filters

| Filter | Ansible | Rustible | Notes |
|--------|---------|----------|-------|
| `first` | Yes | Yes | |
| `last` | Yes | Yes | |
| `length` / `count` | Yes | Yes | |
| `unique` | Yes | Yes | |
| `sort` | Yes | Yes | |
| `reverse` | Yes | Yes | |
| `flatten` | Yes | Yes | |
| `list` | Yes | Yes | Convert to list |
| `selectattr` | Yes | Yes | |
| `rejectattr` | Yes | Yes | |
| `map` | Yes | Yes | Attribute mapping |

### Not Yet Implemented (List)

| Filter | Priority | Notes |
|--------|----------|-------|
| `min` | High | MiniJinja built-in available |
| `max` | High | MiniJinja built-in available |
| `sum` | High | MiniJinja built-in available |
| `batch` | Low | Batch items |
| `slice` | Low | Slice list |
| `zip` | Medium | Zip lists |
| `zip_longest` | Low | Zip with fill |
| `product` | Low | Cartesian product |
| `permutations` | Low | Permutations |
| `combinations` | Low | Combinations |
| `groupby` | Medium | Group by attribute |
| `random` | Medium | Random element |
| `shuffle` | Low | Shuffle list |

### Dictionary Filters

| Filter | Ansible | Rustible | Notes |
|--------|---------|----------|-------|
| `combine` | Yes | Yes | Deep merge dictionaries |
| `dict2items` | Yes | Yes | Convert dict to list of items |
| `items2dict` | Yes | Yes | Convert list to dict |

### Not Yet Implemented (Dict)

| Filter | Priority | Notes |
|--------|----------|-------|
| `dictsort` | Medium | Sort dictionary |
| `difference` | Medium | Set difference |
| `intersect` | Medium | Set intersection |
| `union` | Medium | Set union |
| `symmetric_difference` | Low | Symmetric difference |

### Type Conversion Filters

| Filter | Ansible | Rustible | Notes |
|--------|---------|----------|-------|
| `int` | Yes | Yes | |
| `float` | Yes | Yes | |
| `string` | Yes | Yes | |
| `bool` | Yes | Yes | Ansible truthy semantics |
| `list` | Yes | Yes | |

### Not Yet Implemented (Type)

| Filter | Priority | Notes |
|--------|----------|-------|
| `type_debug` | Low | Debug type info |

### Path Filters

| Filter | Ansible | Rustible | Notes |
|--------|---------|----------|-------|
| `basename` | Yes | Yes | |
| `dirname` | Yes | Yes | |
| `expanduser` | Yes | Yes | |
| `realpath` | Yes | Yes | |

### Not Yet Implemented (Path)

| Filter | Priority | Notes |
|--------|----------|-------|
| `relpath` | Medium | Relative path |
| `splitext` | Medium | Split extension |
| `win_basename` | Low | Windows paths |
| `win_dirname` | Low | Windows paths |

### Encoding Filters

| Filter | Ansible | Rustible | Notes |
|--------|---------|----------|-------|
| `b64encode` | Yes | Yes | |
| `b64decode` | Yes | Yes | |
| `to_json` | Yes | Yes | |
| `to_nice_json` | Yes | Yes | Pretty-printed |
| `from_json` | Yes | Yes | |
| `to_yaml` | Yes | Yes | |
| `to_nice_yaml` | Yes | Yes | Pretty-printed |
| `from_yaml` | Yes | Yes | |
| `from_yaml_all` | Yes | Yes | Multi-document YAML |

### Not Yet Implemented (Encoding)

| Filter | Priority | Notes |
|--------|----------|-------|
| `to_uuid` | Low | Generate UUID |
| `hash` | Medium | Various hash algorithms |
| `checksum` | Medium | File checksum |
| `password_hash` | High | Password hashing |

### Ansible-Specific Filters

| Filter | Ansible | Rustible | Notes |
|--------|---------|----------|-------|
| `mandatory` | Yes | Yes | Fail if undefined |
| `ternary` | Yes | Yes | Conditional value |

### Not Yet Implemented (Ansible-Specific)

| Filter | Priority | Notes |
|--------|----------|-------|
| `ipaddr` | High | IP address manipulation |
| `regex_findall` | High | Find all regex matches |
| `subelements` | Medium | Nested loop helper |
| `extract` | Medium | Extract from mapping |
| `json_query` | Medium | JMESPath queries |
| `community.general.json_query` | Medium | JMESPath queries |
| `to_datetime` | Low | Date/time parsing |
| `strftime` | Low | Date formatting |

---

## Jinja2 Tests

### Implemented Tests

| Test | Ansible | Rustible | Notes |
|------|---------|----------|-------|
| `defined` | Yes | Yes | |
| `undefined` | Yes | Yes | |
| `none` | Yes | Yes | |
| `truthy` | Yes | Yes | |
| `falsy` | Yes | Yes | |
| `boolean` | Yes | Yes | |
| `integer` | Yes | Yes | |
| `float` | Yes | Yes | |
| `number` | Yes | Yes | |
| `string` | Yes | Yes | |
| `mapping` | Yes | Yes | |
| `iterable` | Yes | Yes | |
| `sequence` | Yes | Yes | |
| `sameas` | Yes | Yes | |
| `contains` | Yes | Yes | |
| `match` | Yes | Yes | Regex match |
| `search` | Yes | Yes | Regex search |
| `startswith` | Yes | Yes | |
| `endswith` | Yes | Yes | |
| `file` | Yes | Yes | Is file |
| `directory` | Yes | Yes | Is directory |
| `link` | Yes | Yes | Is symlink |
| `exists` | Yes | Yes | Path exists |
| `abs` | Yes | Yes | Is absolute path |
| `success` | Yes | Yes | Task result |
| `failed` | Yes | Yes | Task result |
| `changed` | Yes | Yes | Task result |
| `skipped` | Yes | Yes | Task result |

### Not Yet Implemented (Tests)

| Test | Priority | Notes |
|------|----------|-------|
| `callable` | Low | Is callable |
| `even` / `odd` | Low | Number parity |
| `divisibleby` | Low | Divisibility |
| `equalto` | Low | Equality test |
| `greaterthan` / `lessthan` | Low | Comparisons |
| `subset` / `superset` | Medium | Set relations |
| `all` / `any` | Medium | List predicates |

---

## Known Differences

### 1. Boolean Handling

Ansible accepts various truthy strings (`yes`, `no`, `true`, `false`, `on`, `off`). Rustible's `bool` filter handles these correctly:

```yaml
# Both work in Rustible
- when: "{{ 'yes' | bool }}"
- when: "{{ enable_feature | bool }}"
```

### 2. Undefined Variable Behavior

Rustible uses MiniJinja's `Chainable` undefined behavior, matching Ansible's default:

```yaml
# Both return empty string, not error
- debug: msg="{{ undefined_var }}"
```

### 3. Filter Chaining

Filter chaining works identically:

```yaml
- debug: msg="{{ items | sort | unique | join(', ') }}"
```

---

## Adding Missing Filters

To implement a missing filter, add it to `src/template.rs`:

```rust
// 1. Implement the filter function
fn filter_myfilter(value: &str) -> String {
    // Implementation
}

// 2. Register in register_filters()
env.add_filter("myfilter", filter_myfilter);

// 3. Add tests in tests/ansible_compat/jinja2_filters.rs
#[test]
fn test_filter_myfilter() {
    let result = render("{{ 'input' | myfilter }}", json!({}));
    assert_eq!(result, "expected");
}
```

---

## Priority Roadmap

### v0.2 (High Priority)

- [ ] `min` / `max` / `sum` (expose MiniJinja builtins)
- [ ] `regex_findall`
- [ ] `password_hash`
- [ ] `ipaddr` (basic support)

### v0.3 (Medium Priority)

- [ ] `hash` / `checksum`
- [ ] `groupby`
- [ ] `zip`
- [ ] `json_query`

### v1.0 (Full Parity)

- [ ] All remaining string filters
- [ ] All remaining list filters
- [ ] Full `ipaddr` support

---

*For the latest filter implementations, see `src/template.rs`*
