# Feature: Implement Full Jinja2 Filter Compatibility

## Problem Statement
Rustible currently supports 40+ Jinja2 filters via MiniJinja, but lacks many Ansible-specific filters that are commonly used in production playbooks. This breaks compatibility with existing Ansible playbooks.

## Current State
- 40+ basic filters supported
- Missing 30+ Ansible-specific filters
- Some filters have different behavior
- Filter documentation incomplete

## Proposed Solution

### Phase 1: Missing Core Filters (v0.1.x)
1. **Mathematical filters**
   ```rust
   // src/template/filters/math.rs
   use minijinja::Value;
   
   pub fn min(value: &Value, args: &[Value]) -> Result<Value, Error> {
       if let Some(seq) = value.as_seq() {
           let result = seq.iter()
               .filter_map(|v| v.as_f64())
               .fold(f64::INFINITY, |a, b| a.min(b));
           Ok(Value::from(result))
       } else {
           Err(Error::new(ErrorKind::InvalidOperation, "min requires a sequence"))
       }
   }
   
   pub fn max(value: &Value, args: &[Value]) -> Result<Value, Error> {
       if let Some(seq) = value.as_seq() {
           let result = seq.iter()
               .filter_map(|v| v.as_f64())
               .fold(f64::NEG_INFINITY, |a, b| a.max(b));
           Ok(Value::from(result))
       } else {
           Err(Error::new(ErrorKind::InvalidOperation, "max requires a sequence"))
       }
   }
   
   pub fn sum(value: &Value, args: &[Value]) -> Result<Value, Error> {
       if let Some(seq) = value.as_seq() {
           let result: f64 = seq.iter()
               .filter_map(|v| v.as_f64())
               .sum();
           Ok(Value::from(result))
       } else {
           Err(Error::new(ErrorKind::InvalidOperation, "sum requires a sequence"))
       }
   }
   
   pub fn product(value: &Value, args: &[Value]) -> Result<Value, Error> {
       if let Some(seq) = value.as_seq() {
           let result: f64 = seq.iter()
               .filter_map(|v| v.as_f64())
               .product();
           Ok(Value::from(result))
       } else {
           Err(Error::new(ErrorKind::InvalidOperation, "product requires a sequence"))
       }
   }
   ```

2. **List filters**
   ```rust
   pub fn difference(value: &Value, args: &[Value]) -> Result<Value, Error> {
       let list1 = value.as_seq().ok_or_else(|| {
           Error::new(ErrorKind::InvalidOperation, "difference requires a sequence")
       })?;
       
       let list2 = args.first().and_then(|v| v.as_seq()).ok_or_else(|| {
           Error::new(ErrorKind::InvalidOperation, "difference requires second argument")
       })?;
       
       let set1: HashSet<Value> = list1.iter().cloned().collect();
       let set2: HashSet<Value> = list2.iter().cloned().collect();
       
       let result: Vec<Value> = set1.difference(&set2).cloned().collect();
       Ok(Value::from(result))
   }
   
   pub fn symmetric_difference(value: &Value, args: &[Value]) -> Result<Value, Error> {
       // Similar to difference but returns items in either list, not both
       let list1 = value.as_seq()?;
       let list2 = args.first()?.as_seq()?;
       
       let set1: HashSet<Value> = list1.iter().cloned().collect();
       let set2: HashSet<Value> = list2.iter().cloned().collect();
       
       let result: Vec<Value> = set1.symmetric_difference(&set2).cloned().collect();
       Ok(Value::from(result))
   }
   
   pub fn unique_union(value: &Value, args: &[Value]) -> Result<Value, Error> {
       let list1 = value.as_seq()?;
       let list2 = args.first()?.as_seq()?;
       
       let result: HashSet<Value> = list1.iter()
           .chain(list2.iter())
           .cloned()
           .collect();
       
       Ok(Value::from(result.into_iter().collect::<Vec<_>>()))
   }
   ```

### Phase 2: Ansible-Specific Filters (v0.1.x)
1. **Password hashing**
   ```rust
   // src/template/filters/password.rs
   use argon2::{Argon2, PasswordHasher};
   use argon2::password_hash::{SaltString, rand_core::OsRng};
   
   pub fn password_hash(value: &Value, args: &[Value]) -> Result<Value, Error> {
       let password = value.as_str().ok_or_else(|| {
           Error::new(ErrorKind::InvalidOperation, "password_hash requires a string")
       })?;
       
       let hash_type = args.first()
           .and_then(|v| v.as_str())
           .unwrap_or("sha256");
       
       let hash = match hash_type {
           "sha256" | "sha512" | "bcrypt" => {
               let argon2 = Argon2::default();
               let salt = SaltString::generate(&mut OsRng);
               argon2.hash_password(password.as_bytes(), &salt)
                   .map_err(|e| Error::new(ErrorKind::RuntimeError, e.to_string()))?
                   .to_string()
           }
           _ => return Err(Error::new(ErrorKind::InvalidOperation, "Unsupported hash type")),
       };
       
       Ok(Value::from(hash))
   }
   ```

2. **IP address manipulation**
   ```rust
   // src/template/filters/ipaddr.rs
   use ipnetwork::IpNetwork;
   
   pub fn ipaddr(value: &Value, args: &[Value]) -> Result<Value, Error> {
       let ip_str = value.as_str()?;
       let query = args.first()
           .and_then(|v| v.as_str())
           .unwrap_or("address");
       
       let ip = IpNetwork::from_str(ip_str)
           .map_err(|e| Error::new(ErrorKind::RuntimeError, e.to_string()))?;
       
       match query {
           "address" => Ok(Value::from(ip.ip().to_string())),
           "network" => Ok(Value::from(ip.network().to_string())),
           "broadcast" => Ok(Value::from(ip.broadcast().to_string())),
           "netmask" => Ok(Value::from(ip.netmask().to_string())),
           "prefix" => Ok(Value::from(ip.prefix() as i64)),
           "version" => Ok(Value::from(match ip {
               IpNetwork::V4(_) => 4,
               IpNetwork::V6(_) => 6,
           })),
           "is_ipv4" => Ok(Value::from(matches!(ip, IpNetwork::V4(_)))),
           "is_ipv6" => Ok(Value::from(matches!(ip, IpNetwork::V6(_)))),
           _ => Err(Error::new(ErrorKind::InvalidOperation, "Unsupported ipaddr query")),
       }
   }
   ```

3. **JSON/JMESPath filters**
   ```rust
   // src/template/filters/json.rs
   use jmespath::{Expression, Variable};
   
   pub fn json_query(value: &Value, args: &[Value]) -> Result<Value, Error> {
       let json_str = value.as_str()?;
       let query_str = args.first()
           .and_then(|v| v.as_str())
           .ok_or_else(|| Error::new(ErrorKind::InvalidOperation, "json_query requires a query"))?;
       
       let json_value: serde_json::Value = serde_json::from_str(json_str)
           .map_err(|e| Error::new(ErrorKind::RuntimeError, e.to_string()))?;
       
       let expr = Expression::new(query_str)
           .map_err(|e| Error::new(ErrorKind::RuntimeError, e.to_string()))?;
       
       let variable = Variable::from(json_value);
       let result = expr.search(variable)
           .map_err(|e| Error::new(ErrorKind::RuntimeError, e.to_string()))?;
       
       Ok(Value::from(result))
   }
   ```

### Phase 3: Advanced Filters (v0.2.x)
1. **String manipulation**
   ```rust
   pub fn regex_escape(value: &Value, args: &[Value]) -> Result<Value, Error> {
       let s = value.as_str()?;
       let escaped = regex::escape(s);
       Ok(Value::from(escaped))
   }
   
   pub fn regex_findall(value: &Value, args: &[Value]) -> Result<Value, Error> {
       let s = value.as_str()?;
       let pattern = args.first()?.as_str()?;
       
       let re = regex::Regex::new(pattern)
           .map_err(|e| Error::new(ErrorKind::RuntimeError, e.to_string()))?;
       
       let matches: Vec<Value> = re.find_iter(s)
           .map(|m| Value::from(m.as_str().to_string()))
           .collect();
       
       Ok(Value::from(matches))
   }
   
   pub fn regex_replace(value: &Value, args: &[Value]) -> Result<Value, Error> {
       let s = value.as_str()?;
       let pattern = args.get(0)?.as_str()?;
       let replacement = args.get(1)?.as_str()?;
       
       let re = regex::Regex::new(pattern)
           .map_err(|e| Error::new(ErrorKind::RuntimeError, e.to_string()))?;
       
       let result = re.replace_all(s, replacement);
       Ok(Value::from(result.into_owned()))
   }
   ```

2. **Data structure filters**
   ```rust
   pub fn subelements(value: &Value, args: &[Value]) -> Result<Value, Error> {
       let list = value.as_seq()?;
       let key = args.first()?.as_str()?;
       
       let mut result = Vec::new();
       for item in list {
           if let Some(sub_item) = item.get_attr(key) {
               if let Some(sub_list) = sub_item.as_seq() {
                   for sub in sub_list {
                       result.push(Value::from(vec![item.clone(), sub.clone()]));
                   }
               }
           }
       }
       
       Ok(Value::from(result))
   }
   
   pub fn dict2items(value: &Value, args: &[Value]) -> Result<Value, Error> {
       let dict = value.as_object()
           .ok_or_else(|| Error::new(ErrorKind::InvalidOperation, "dict2items requires a dict"))?;
       
       let items: Vec<Value> = dict.iter()
           .map(|(k, v)| {
               Value::from(vec![Value::from(k.clone()), v.clone()])
           })
           .collect();
       
       Ok(Value::from(items))
   }
   ```

### Phase 4: Testing and Documentation (v0.2.x)
1. **Filter test suite**
   ```rust
   #[cfg(test)]
   mod tests {
       use super::*;
       
       #[test]
       fn test_min_filter() {
           let value = Value::from(vec![5, 2, 8, 1, 9]);
           let result = min(&value, &[]).unwrap();
           assert_eq!(result.as_i64(), Some(1));
       }
       
       #[test]
       fn test_ipaddr_filter() {
           let value = Value::from("192.168.1.0/24");
           let args = vec![Value::from("network")];
           let result = ipaddr(&value, &args).unwrap();
           assert_eq!(result.as_str(), Some("192.168.1.0"));
       }
   }
   ```

2. **Filter documentation**
   - Document each filter with examples
   - Show Ansible compatibility
   - Note any behavioral differences
   - Include type signatures

## Expected Outcomes
- 100% Ansible filter compatibility
- All 70+ Jinja2 filters implemented
- Comprehensive test coverage
- Complete documentation
- Performance optimizations

## Success Criteria
- [ ] All math filters implemented (min, max, sum, product, etc.)
- [ ] All list filters implemented (difference, unique, etc.)
- [ ] All string filters implemented (regex_*, etc.)
- [ ] All dict filters implemented (dict2items, items2dict, etc.)
- [ ] Ansible-specific filters (password_hash, ipaddr, json_query)
- [ ] 90% test coverage for filters
- [ ] Filter documentation complete
- [ ] Filter performance optimized
- [ ] No filter regressions

## Implementation Details

### Filter Registration
```rust
// src/template/mod.rs
use minijinja::{Environment, Value};

pub fn create_template_engine() -> Environment<'static> {
    let mut env = Environment::new();
    
    // Register all filters
    env.add_filter("min", filters::math::min);
    env.add_filter("max", filters::math::max);
    env.add_filter("sum", filters::math::sum);
    env.add_filter("password_hash", filters::password::password_hash);
    env.add_filter("ipaddr", filters::ipaddr::ipaddr);
    env.add_filter("json_query", filters::json::json_query);
    env.add_filter("regex_findall", filters::regex::regex_findall);
    env.add_filter("subelements", filters::data::subelements);
    env.add_filter("dict2items", filters::data::dict2items);
    // ... and so on
    
    env
}
```

## Related Issues
- #006: Pre-execution Validation
- #011: Module Parity
- #013: Module Documentation

## Additional Notes
This is a **P1 (High)** feature as filter compatibility is essential for Ansible playbook migration. Should be targeted for v0.1.x core filters with v0.2.x complete coverage.
