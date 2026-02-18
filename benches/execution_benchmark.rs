//! Comprehensive benchmarks for Rustible performance characteristics
//!
//! This benchmark suite measures:
//! - Playbook parsing speed (simple and complex)
//! - Inventory parsing speed (different sizes)
//! - Template rendering speed (variables, loops, filters)
//! - Variable resolution and merging
//! - Connection pool operations
//! - Task execution overhead
//! - Handler notification performance
//! - Parallel execution scaling
//! - Memory usage patterns

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use indexmap::IndexMap;
use rustible::connection::{
    ConnectionConfig, ConnectionFactory,
};
use rustible::executor::playbook::Playbook;
use rustible::inventory::{Group, Host, Inventory};
use rustible::template::TemplateEngine;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Runtime;

// ============================================================================
// Test Data Generators
// ============================================================================

/// Generate a simple playbook YAML string
fn generate_simple_playbook() -> String {
    r#"
- name: Simple Test Play
  hosts: all
  gather_facts: false
  tasks:
    - name: Debug message
      debug:
        msg: "Hello World"

    - name: Set fact
      set_fact:
        test_var: "test_value"

    - name: Another debug
      debug:
        msg: "{{ test_var }}"
"#
    .to_string()
}

/// Generate a complex playbook with multiple plays, roles, handlers
fn generate_complex_playbook() -> String {
    r#"
- name: Complex Web Server Setup
  hosts: webservers
  gather_facts: true
  vars:
    http_port: 80
    max_clients: 200
  roles:
    - common
    - nginx
  pre_tasks:
    - name: Update package cache
      apt:
        update_cache: yes
      when: ansible_os_family == "Debian"

  tasks:
    - name: Install nginx
      package:
        name: nginx
        state: present
      notify: restart nginx

    - name: Template nginx config
      template:
        src: nginx.conf.j2
        dest: /etc/nginx/nginx.conf
      notify:
        - reload nginx
        - send notification

    - name: Create web directories
      file:
        path: "/var/www/{{ item }}"
        state: directory
        owner: www-data
        group: www-data
      loop:
        - html
        - logs
        - cache

    - name: Deploy application
      copy:
        src: "app/"
        dest: /var/www/html/
      register: deploy_result

    - name: Check deployment
      assert:
        that:
          - deploy_result.changed
        fail_msg: "Deployment failed"

  post_tasks:
    - name: Verify nginx is running
      service:
        name: nginx
        state: started
        enabled: yes

  handlers:
    - name: restart nginx
      service:
        name: nginx
        state: restarted

    - name: reload nginx
      service:
        name: nginx
        state: reloaded

    - name: send notification
      debug:
        msg: "Configuration changed"

- name: Database Setup
  hosts: databases
  gather_facts: true
  vars:
    db_port: 5432
  tasks:
    - name: Install PostgreSQL
      package:
        name: postgresql
        state: present

    - name: Configure PostgreSQL
      lineinfile:
        path: /etc/postgresql/postgresql.conf
        regexp: "^port ="
        line: "port = {{ db_port }}"
      notify: restart postgresql

  handlers:
    - name: restart postgresql
      service:
        name: postgresql
        state: restarted
"#
    .to_string()
}

/// Generate inventory with specified number of hosts
fn generate_inventory_yaml(num_hosts: usize) -> String {
    let mut yaml = String::from("all:\n  children:\n    webservers:\n      hosts:\n");

    for i in 0..num_hosts {
        yaml.push_str(&format!(
            "        web{:04}:\n          ansible_host: 10.0.{}.{}\n          ansible_port: 22\n",
            i,
            i / 256,
            i % 256
        ));
    }

    yaml.push_str("    databases:\n      hosts:\n");
    for i in 0..std::cmp::min(num_hosts / 10, 100) {
        yaml.push_str(&format!(
            "        db{:03}:\n          ansible_host: 10.1.{}.{}\n",
            i,
            i / 256,
            i % 256
        ));
    }

    yaml.push_str("  vars:\n    ansible_user: admin\n    environment: production\n");

    yaml
}

/// Generate INI inventory
fn generate_inventory_ini(num_hosts: usize) -> String {
    let mut ini = String::from("[webservers]\n");

    for i in 0..num_hosts {
        ini.push_str(&format!(
            "web{:04} ansible_host=10.0.{}.{}\n",
            i,
            i / 256,
            i % 256
        ));
    }

    ini.push_str("\n[databases]\n");
    for i in 0..std::cmp::min(num_hosts / 10, 100) {
        ini.push_str(&format!(
            "db{:03} ansible_host=10.1.{}.{}\n",
            i,
            i / 256,
            i % 256
        ));
    }

    ini.push_str("\n[webservers:vars]\nhttp_port=80\n");
    ini.push_str("\n[all:vars]\nansible_user=admin\n");

    ini
}

/// Generate template with variables
fn generate_template_simple() -> String {
    "Hello {{ name }}, you have {{ count }} messages!".to_string()
}

/// Generate template with loops and filters
fn generate_template_complex() -> String {
    r#"
# Configuration file
server_name: {{ server_name }}
port: {{ port }}

users:
{% for user in users %}
  - name: {{ user.name }}
    email: {{ user.email }}
    role: {{ user.role }}
{% endfor %}

settings:
  max_connections: {{ max_connections }}
  timeout: {{ timeout }}
  enabled: {{ enabled }}
"#
    .to_string()
}

/// Generate variables for templating
fn generate_template_vars_simple() -> HashMap<String, serde_json::Value> {
    let mut vars = HashMap::new();
    vars.insert("name".to_string(), serde_json::json!("Alice"));
    vars.insert("count".to_string(), serde_json::json!(42));
    vars
}

fn generate_template_vars_complex() -> HashMap<String, serde_json::Value> {
    let mut vars = HashMap::new();
    vars.insert(
        "server_name".to_string(),
        serde_json::json!("production.example.com"),
    );
    vars.insert("port".to_string(), serde_json::json!(8080));
    vars.insert("max_connections".to_string(), serde_json::json!(1000));
    vars.insert("timeout".to_string(), serde_json::json!(30));
    vars.insert("enabled".to_string(), serde_json::json!(true));

    let users = serde_json::json!([
        {"name": "alice", "email": "alice@example.com", "role": "admin"},
        {"name": "bob", "email": "bob@example.com", "role": "user"},
        {"name": "charlie", "email": "charlie@example.com", "role": "user"},
        {"name": "diana", "email": "diana@example.com", "role": "moderator"},
    ]);
    vars.insert("users".to_string(), users);

    vars
}

// ============================================================================
// Playbook Parsing Benchmarks
// ============================================================================

fn bench_playbook_parsing_simple(c: &mut Criterion) {
    let yaml = generate_simple_playbook();

    c.bench_function("playbook_parse_simple", |b| {
        b.iter(|| {
            let result = Playbook::parse(black_box(&yaml), None);
            black_box(result)
        })
    });
}

fn bench_playbook_parsing_complex(c: &mut Criterion) {
    let yaml = generate_complex_playbook();

    c.bench_function("playbook_parse_complex", |b| {
        b.iter(|| {
            let result = Playbook::parse(black_box(&yaml), None);
            black_box(result)
        })
    });
}

fn bench_playbook_parsing_cached(c: &mut Criterion) {
    let yaml = generate_complex_playbook();

    c.bench_function("playbook_parse_complex_repeated", |b| {
        b.iter(|| {
            // Simulates repeated parsing (e.g., for validation)
            for _ in 0..10 {
                let result = Playbook::parse(black_box(&yaml), None);
                black_box(result).ok();
            }
        })
    });
}

// ============================================================================
// Inventory Parsing Benchmarks
// ============================================================================

fn bench_inventory_parsing_yaml(c: &mut Criterion) {
    let mut group = c.benchmark_group("inventory_parse_yaml");

    for size in [10, 100, 1000].iter() {
        let yaml = generate_inventory_yaml(*size);
        group.throughput(Throughput::Elements(*size as u64));

        // Write to a temp file and load it
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                use std::io::Write;
                let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
                tmpfile.write_all(yaml.as_bytes()).unwrap();
                tmpfile.flush().unwrap();
                let result = Inventory::load(black_box(tmpfile.path()));
                black_box(result)
            })
        });
    }

    group.finish();
}

fn bench_inventory_parsing_ini(c: &mut Criterion) {
    let mut group = c.benchmark_group("inventory_parse_ini");

    for size in [10, 100, 1000].iter() {
        let ini = generate_inventory_ini(*size);
        group.throughput(Throughput::Elements(*size as u64));

        // Write to a temp file and load it
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                use std::io::Write;
                let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
                tmpfile.write_all(ini.as_bytes()).unwrap();
                tmpfile.flush().unwrap();
                let result = Inventory::load(black_box(tmpfile.path()));
                black_box(result)
            })
        });
    }

    group.finish();
}

fn bench_inventory_pattern_matching(c: &mut Criterion) {
    // Create inventory from temp file
    let yaml = generate_inventory_yaml(100);
    use std::io::Write;
    let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
    tmpfile.write_all(yaml.as_bytes()).unwrap();
    tmpfile.flush().unwrap();
    let inv = Inventory::load(tmpfile.path()).unwrap();

    let mut group = c.benchmark_group("inventory_pattern_matching");

    // Test different pattern types
    let patterns = vec![
        ("all", "all hosts"),
        ("webservers", "single group"),
        ("web*", "wildcard pattern"),
        ("~web\\d+", "regex pattern"),
        ("webservers:databases", "union"),
        ("webservers:&databases", "intersection"),
        ("all:!databases", "exclusion"),
    ];

    for (pattern, name) in patterns {
        group.bench_function(name, |b| {
            b.iter(|| {
                let result = inv.get_hosts_for_pattern(black_box(pattern));
                black_box(result)
            })
        });
    }

    group.finish();
}

// ============================================================================
// Template Rendering Benchmarks
// ============================================================================

fn bench_template_rendering_simple(c: &mut Criterion) {
    let engine = TemplateEngine::new();
    let template = generate_template_simple();
    let vars = generate_template_vars_simple();

    c.bench_function("template_render_simple", |b| {
        b.iter(|| {
            let result = engine.render(black_box(&template), black_box(&vars));
            black_box(result)
        })
    });
}

fn bench_template_rendering_complex(c: &mut Criterion) {
    let engine = TemplateEngine::new();
    let template = generate_template_complex();
    let vars = generate_template_vars_complex();

    c.bench_function("template_render_complex", |b| {
        b.iter(|| {
            let result = engine.render(black_box(&template), black_box(&vars));
            black_box(result)
        })
    });
}

fn bench_template_variable_interpolation(c: &mut Criterion) {
    let mut group = c.benchmark_group("template_variable_interpolation");

    let engine = TemplateEngine::new();

    // Test with different numbers of variables
    for num_vars in [1, 10, 50, 100].iter() {
        let mut template = String::new();
        let mut vars = HashMap::new();

        for i in 0..*num_vars {
            template.push_str(&format!("var{}: {{{{ var{} }}}} ", i, i));
            vars.insert(
                format!("var{}", i),
                serde_json::json!(format!("value{}", i)),
            );
        }

        group.throughput(Throughput::Elements(*num_vars as u64));
        group.bench_with_input(BenchmarkId::from_parameter(num_vars), num_vars, |b, _| {
            b.iter(|| {
                let result = engine.render(black_box(&template), black_box(&vars));
                black_box(result)
            })
        });
    }

    group.finish();
}

// ============================================================================
// Variable Resolution and Merging Benchmarks
// ============================================================================

fn bench_variable_resolution(c: &mut Criterion) {
    let mut group = c.benchmark_group("variable_resolution");

    // Create inventory with group hierarchy
    let mut inv = Inventory::new();

    // Add groups with vars
    let mut all_group = Group::new("all");
    all_group.set_var("global_var", serde_yaml::Value::String("global".into()));
    all_group.set_var("override_me", serde_yaml::Value::String("all".into()));
    inv.add_group(all_group).unwrap();

    let mut web_group = Group::new("webservers");
    web_group.set_var("web_var", serde_yaml::Value::String("web".into()));
    web_group.set_var("override_me", serde_yaml::Value::String("web".into()));
    inv.add_group(web_group).unwrap();

    // Add host with vars
    let mut host = Host::new("web001");
    host.set_var("host_var", serde_yaml::Value::String("host".into()));
    host.set_var("override_me", serde_yaml::Value::String("host".into()));
    host.add_to_group("webservers".to_string());
    host.add_to_group("all".to_string());
    inv.add_host(host).unwrap();

    group.bench_function("get_host_vars", |b| {
        b.iter(|| {
            let host = inv.get_host("web001").unwrap();
            let vars = inv.get_host_vars(black_box(host));
            black_box(vars)
        })
    });

    group.bench_function("get_host_group_hierarchy", |b| {
        b.iter(|| {
            let host = inv.get_host("web001").unwrap();
            let hierarchy = inv.get_host_group_hierarchy(black_box(host));
            black_box(hierarchy)
        })
    });

    group.finish();
}

fn bench_variable_merging(c: &mut Criterion) {
    let mut group = c.benchmark_group("variable_merging");

    // Create multiple variable sets to merge
    for num_vars in [10, 50, 100, 500].iter() {
        let mut base_vars = IndexMap::new();
        let mut override_vars = IndexMap::new();

        for i in 0..*num_vars {
            base_vars.insert(
                format!("var{}", i),
                serde_yaml::Value::String(format!("base{}", i)),
            );
            if i % 2 == 0 {
                override_vars.insert(
                    format!("var{}", i),
                    serde_yaml::Value::String(format!("override{}", i)),
                );
            }
        }

        group.throughput(Throughput::Elements(*num_vars as u64));
        group.bench_with_input(BenchmarkId::from_parameter(num_vars), num_vars, |b, _| {
            b.iter(|| {
                let mut merged = base_vars.clone();
                merged.extend(black_box(override_vars.clone()));
                black_box(merged)
            })
        });
    }

    group.finish();
}

// ============================================================================
// Connection Pool Benchmarks
// ============================================================================

fn bench_connection_pool_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("connection_pool");

    let config = ConnectionConfig::default();

    // Test pool operations
    group.bench_function("factory_create", |b| {
        b.iter(|| {
            let factory = ConnectionFactory::new(black_box(config.clone()));
            black_box(factory)
        })
    });

    group.bench_function("get_local_connection", |b| {
        let factory = Arc::new(ConnectionFactory::new(config.clone()));
        b.to_async(&rt).iter(|| {
            let factory = Arc::clone(&factory);
            async move {
                let conn = factory.get_connection(black_box("localhost")).await;
                black_box(conn)
            }
        })
    });

    // Test pool reuse
    group.bench_function("connection_reuse", |b| {
        let factory = Arc::new(ConnectionFactory::new(config.clone()));
        b.to_async(&rt).iter(|| {
            let factory = Arc::clone(&factory);
            async move {
                // First call creates connection
                let _conn1 = factory.get_connection("localhost").await.unwrap();
                // Second call should reuse
                let conn2 = factory.get_connection("localhost").await;
                black_box(conn2)
            }
        })
    });

    group.finish();
}

fn bench_connection_pool_scaling(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("connection_pool_scaling");

    let config = ConnectionConfig::default();

    // Test with different pool sizes
    for pool_size in [1, 5, 10].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(pool_size),
            pool_size,
            |b, &size| {
                let factory = Arc::new(ConnectionFactory::with_pool_size(config.clone(), size));
                b.to_async(&rt).iter(|| {
                    let factory = Arc::clone(&factory);
                    async move {
                        // Try to get multiple connections sequentially
                        for i in 0..size {
                            let host = format!("localhost_{}", i);
                            let conn = factory.get_connection(&host).await;
                            let _ = black_box(conn);
                        }
                    }
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// Task Execution Benchmarks
// ============================================================================

fn bench_task_execution_overhead(c: &mut Criterion) {
    let _rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("task_execution");

    // Parse a playbook with various task types
    let yaml = generate_simple_playbook();
    let playbook = Playbook::parse(&yaml, None).unwrap();

    group.bench_function("task_parsing", |b| {
        b.iter(|| {
            let pb = Playbook::parse(black_box(&yaml), None);
            black_box(pb)
        })
    });

    // Benchmark task cloning (important for parallel execution)
    let task = &playbook.plays[0].tasks[0];
    group.bench_function("task_clone", |b| {
        b.iter(|| {
            let cloned = black_box(task).clone();
            black_box(cloned)
        })
    });

    group.finish();
}

// ============================================================================
// Handler Notification Benchmarks
// ============================================================================

fn bench_handler_notifications(c: &mut Criterion) {
    let mut group = c.benchmark_group("handler_notifications");

    // Test handler lookup and notification
    let yaml = generate_complex_playbook();
    let playbook = Playbook::parse(&yaml, None).unwrap();
    let play = &playbook.plays[0];

    group.bench_function("handler_lookup", |b| {
        b.iter(|| {
            let handler_name = "restart nginx";
            let handler = play
                .handlers
                .iter()
                .find(|h| h.name == handler_name || h.listen.contains(&handler_name.to_string()));
            black_box(handler)
        })
    });

    // Test notification tracking
    use std::collections::HashSet;
    group.bench_function("notification_tracking", |b| {
        b.iter(|| {
            let mut notified: HashSet<String> = HashSet::new();
            for task in &play.tasks {
                for handler in &task.notify {
                    notified.insert(black_box(handler.clone()));
                }
            }
            black_box(notified)
        })
    });

    group.finish();
}

// ============================================================================
// Parallel Execution Scaling Benchmarks
// ============================================================================

fn bench_parallel_execution_scaling(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("parallel_execution");

    // Simulate parallel task execution with different fork counts
    for fork_count in [1, 2, 5, 10, 20].iter() {
        group.throughput(Throughput::Elements(*fork_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(fork_count),
            fork_count,
            |b, &forks| {
                b.to_async(&rt).iter(|| async move {
                    // Simulate parallel execution of tasks
                    let mut handles = vec![];
                    for i in 0..forks {
                        handles.push(tokio::spawn(async move {
                            // Simulate some work
                            tokio::time::sleep(tokio::time::Duration::from_micros(10)).await;
                            i
                        }));
                    }
                    for handle in handles {
                        black_box(handle.await).ok();
                    }
                })
            },
        );
    }

    group.finish();
}

fn bench_async_task_spawning(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("async_task_spawning");

    for task_count in [10, 50, 100, 500].iter() {
        group.throughput(Throughput::Elements(*task_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(task_count),
            task_count,
            |b, &count| {
                b.to_async(&rt).iter(|| async move {
                    let mut handles = vec![];
                    for i in 0..count {
                        handles.push(tokio::spawn(async move { i * 2 }));
                    }
                    for handle in handles {
                        black_box(handle.await).ok();
                    }
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// Memory Usage Pattern Benchmarks
// ============================================================================

fn bench_memory_allocation_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_allocation");

    // Benchmark inventory memory allocation
    group.bench_function("inventory_small_alloc", |b| {
        b.iter(|| {
            let yaml = generate_inventory_yaml(10);
            use std::io::Write;
            let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
            tmpfile.write_all(yaml.as_bytes()).unwrap();
            tmpfile.flush().unwrap();
            let inv = Inventory::load(black_box(tmpfile.path())).ok();
            black_box(inv)
        })
    });

    group.bench_function("inventory_large_alloc", |b| {
        b.iter(|| {
            let yaml = generate_inventory_yaml(1000);
            use std::io::Write;
            let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
            tmpfile.write_all(yaml.as_bytes()).unwrap();
            tmpfile.flush().unwrap();
            let inv = Inventory::load(black_box(tmpfile.path())).ok();
            black_box(inv)
        })
    });

    // Benchmark playbook memory allocation
    group.bench_function("playbook_alloc", |b| {
        b.iter(|| {
            let yaml = generate_complex_playbook();
            let pb = Playbook::parse(black_box(&yaml), None);
            black_box(pb)
        })
    });

    // Benchmark variable storage
    group.bench_function("variables_alloc_100", |b| {
        b.iter(|| {
            let mut vars = IndexMap::new();
            for i in 0..100 {
                vars.insert(
                    format!("var{}", i),
                    serde_json::json!(format!("value{}", i)),
                );
            }
            black_box(vars)
        })
    });

    group.finish();
}

fn bench_data_structure_cloning(c: &mut Criterion) {
    let mut group = c.benchmark_group("data_structure_cloning");

    // Benchmark cloning of common data structures
    let yaml = generate_complex_playbook();
    let playbook = Playbook::parse(&yaml, None).unwrap();

    group.bench_function("playbook_clone", |b| {
        b.iter(|| {
            let cloned = black_box(&playbook).clone();
            black_box(cloned)
        })
    });

    let inventory_yaml = generate_inventory_yaml(100);
    use std::io::Write;
    let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
    tmpfile.write_all(inventory_yaml.as_bytes()).unwrap();
    tmpfile.flush().unwrap();
    let inventory = Inventory::load(tmpfile.path()).unwrap();

    group.bench_function("inventory_clone", |b| {
        b.iter(|| {
            let cloned = black_box(&inventory).clone();
            black_box(cloned)
        })
    });

    group.finish();
}

// ============================================================================
// Criterion Groups and Main
// ============================================================================

criterion_group!(
    playbook_benches,
    bench_playbook_parsing_simple,
    bench_playbook_parsing_complex,
    bench_playbook_parsing_cached,
);

criterion_group!(
    inventory_benches,
    bench_inventory_parsing_yaml,
    bench_inventory_parsing_ini,
    bench_inventory_pattern_matching,
);

criterion_group!(
    template_benches,
    bench_template_rendering_simple,
    bench_template_rendering_complex,
    bench_template_variable_interpolation,
);

criterion_group!(
    variable_benches,
    bench_variable_resolution,
    bench_variable_merging,
);

criterion_group!(
    connection_benches,
    bench_connection_pool_operations,
    bench_connection_pool_scaling,
);

criterion_group!(task_benches, bench_task_execution_overhead,);

criterion_group!(handler_benches, bench_handler_notifications,);

criterion_group!(
    parallel_benches,
    bench_parallel_execution_scaling,
    bench_async_task_spawning,
);

criterion_group!(
    memory_benches,
    bench_memory_allocation_patterns,
    bench_data_structure_cloning,
);

criterion_main!(
    playbook_benches,
    inventory_benches,
    template_benches,
    variable_benches,
    connection_benches,
    task_benches,
    handler_benches,
    parallel_benches,
    memory_benches,
);
