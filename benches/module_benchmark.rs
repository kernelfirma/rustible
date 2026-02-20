//! Module Execution Performance Benchmarks for Rustible
//!
//! This benchmark suite provides comprehensive performance testing for:
//!
//! 1. PARAMETER PARSING:
//!    - Simple parameter extraction
//!    - Complex nested parameter handling
//!    - Type coercion and validation
//!    - Vector/array parameter parsing
//!
//! 2. MODULE DISPATCH:
//!    - Registry lookup performance
//!    - Module instantiation overhead
//!    - Classification-based routing
//!    - Parallelization hint retrieval
//!
//! 3. EXECUTION OVERHEAD:
//!    - Context creation and setup
//!    - Local vs remote dispatch
//!    - Check mode overhead
//!    - Diff mode overhead
//!
//! 4. RESULT SERIALIZATION:
//!    - Simple output serialization
//!    - Complex output with data
//!    - Command output handling
//!    - Diff generation
//!
//! 5. MODULE TYPE COMPARISON:
//!    - LocalLogic modules (debug, set_fact, assert)
//!    - NativeTransport modules (copy, template, file)
//!    - RemoteCommand modules (command, shell, service)
//!    - Comparison with Ansible module execution times
//!
//! Run with: cargo bench --bench module_benchmark

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::collections::HashMap;
use std::sync::Arc;

use rustible::modules::{
    validate_command_args, Diff, ModuleClassification, ModuleContext, ModuleOutput, ModuleParams,
    ModuleRegistry, ParallelizationHint, ParamExt,
};

// ============================================================================
// TEST DATA GENERATORS
// ============================================================================

/// Generate simple module parameters
fn generate_simple_params() -> ModuleParams {
    let mut params = HashMap::new();
    params.insert("msg".to_string(), serde_json::json!("Hello, World!"));
    params
}

/// Generate command module parameters
fn generate_command_params() -> ModuleParams {
    let mut params = HashMap::new();
    params.insert("cmd".to_string(), serde_json::json!("echo hello"));
    params.insert("chdir".to_string(), serde_json::json!("/tmp"));
    params.insert("creates".to_string(), serde_json::json!("/tmp/marker"));
    params.insert("removes".to_string(), serde_json::json!("/tmp/old_file"));
    params.insert("warn".to_string(), serde_json::json!(true));
    params
}

/// Generate complex nested parameters
fn generate_complex_params() -> ModuleParams {
    let mut params = HashMap::new();
    params.insert("src".to_string(), serde_json::json!("/path/to/source.j2"));
    params.insert("dest".to_string(), serde_json::json!("/etc/config.conf"));
    params.insert("owner".to_string(), serde_json::json!("root"));
    params.insert("group".to_string(), serde_json::json!("root"));
    params.insert("mode".to_string(), serde_json::json!("0644"));
    params.insert("backup".to_string(), serde_json::json!(true));
    params.insert("force".to_string(), serde_json::json!(false));
    params.insert(
        "validate".to_string(),
        serde_json::json!("/usr/sbin/nginx -t -c %s"),
    );
    params.insert(
        "env".to_string(),
        serde_json::json!({
            "PATH": "/usr/local/bin:/usr/bin:/bin",
            "HOME": "/root",
            "LANG": "en_US.UTF-8"
        }),
    );
    params
}

/// Generate package module parameters with list
fn generate_package_params() -> ModuleParams {
    let mut params = HashMap::new();
    params.insert(
        "name".to_string(),
        serde_json::json!(["nginx", "python3", "vim", "curl", "git"]),
    );
    params.insert("state".to_string(), serde_json::json!("present"));
    params.insert("update_cache".to_string(), serde_json::json!(true));
    params
}

/// Generate user module parameters
#[allow(dead_code)]
fn generate_user_params() -> ModuleParams {
    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("deploy"));
    params.insert("uid".to_string(), serde_json::json!(1001));
    params.insert("group".to_string(), serde_json::json!("deploy"));
    params.insert(
        "groups".to_string(),
        serde_json::json!(["docker", "sudo", "adm"]),
    );
    params.insert("shell".to_string(), serde_json::json!("/bin/bash"));
    params.insert("home".to_string(), serde_json::json!("/home/deploy"));
    params.insert("create_home".to_string(), serde_json::json!(true));
    params.insert("state".to_string(), serde_json::json!("present"));
    params.insert("generate_ssh_key".to_string(), serde_json::json!(true));
    params.insert("ssh_key_type".to_string(), serde_json::json!("ed25519"));
    params
}

/// Generate module context with variables
fn generate_context_with_vars(num_vars: usize) -> ModuleContext {
    let mut vars = HashMap::new();
    for i in 0..num_vars {
        vars.insert(
            format!("var_{}", i),
            serde_json::json!(format!("value_{}", i)),
        );
    }

    let mut facts = HashMap::new();
    facts.insert("ansible_os_family".to_string(), serde_json::json!("Debian"));
    facts.insert(
        "ansible_distribution".to_string(),
        serde_json::json!("Ubuntu"),
    );
    facts.insert(
        "ansible_distribution_version".to_string(),
        serde_json::json!("22.04"),
    );

    ModuleContext::new().with_vars(vars).with_facts(facts)
}

// ============================================================================
// PARAMETER PARSING BENCHMARKS
// ============================================================================

fn bench_validate_command_args(c: &mut Criterion) {
    let mut group = c.benchmark_group("validate_command_args");

    // Fast path: safe alphanumeric
    let safe_simple = "nginx -t";
    group.bench_function("safe_simple", |b| {
        b.iter(|| validate_command_args(black_box(safe_simple)))
    });

    // Slow path: safe but quoted (this is what we are optimizing)
    let safe_quoted = "echo \"hello world\"";
    group.bench_function("safe_quoted", |b| {
        b.iter(|| validate_command_args(black_box(safe_quoted)))
    });

    // Error path: dangerous
    let dangerous = "sh -c 'echo pwned' #";
    group.bench_function("dangerous", |b| {
        b.iter(|| validate_command_args(black_box(dangerous)))
    });

    group.finish();
}

fn bench_parameter_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("parameter_parsing");

    let simple_params = generate_simple_params();
    let command_params = generate_command_params();
    let complex_params = generate_complex_params();
    let package_params = generate_package_params();

    // Benchmark get_string
    group.bench_function("get_string_exists", |b| {
        b.iter(|| {
            let result = command_params.get_string(black_box("cmd"));
            black_box(result)
        })
    });

    group.bench_function("get_string_missing", |b| {
        b.iter(|| {
            let result = command_params.get_string(black_box("nonexistent"));
            black_box(result)
        })
    });

    // Benchmark get_string_required
    group.bench_function("get_string_required", |b| {
        b.iter(|| {
            let result = command_params.get_string_required(black_box("cmd"));
            black_box(result)
        })
    });

    // Benchmark get_bool
    group.bench_function("get_bool_true", |b| {
        b.iter(|| {
            let result = command_params.get_bool(black_box("warn"));
            black_box(result)
        })
    });

    group.bench_function("get_bool_or_default", |b| {
        b.iter(|| {
            let result = command_params.get_bool_or(black_box("missing"), true);
            black_box(result)
        })
    });

    // Benchmark get_vec_string
    group.bench_function("get_vec_string", |b| {
        b.iter(|| {
            let result = package_params.get_vec_string(black_box("name"));
            black_box(result)
        })
    });

    // Benchmark get_i64/get_u32
    let mut numeric_params = HashMap::new();
    numeric_params.insert("port".to_string(), serde_json::json!(8080));
    numeric_params.insert("timeout".to_string(), serde_json::json!(300));

    group.bench_function("get_i64", |b| {
        b.iter(|| {
            let result = numeric_params.get_i64(black_box("timeout"));
            black_box(result)
        })
    });

    group.bench_function("get_u32", |b| {
        b.iter(|| {
            let result = numeric_params.get_u32(black_box("port"));
            black_box(result)
        })
    });

    // Benchmark parameter cloning (important for module dispatch)
    group.bench_function("params_clone_simple", |b| {
        b.iter(|| black_box(simple_params.clone()))
    });

    group.bench_function("params_clone_complex", |b| {
        b.iter(|| black_box(complex_params.clone()))
    });

    group.finish();
}

fn bench_parameter_validation_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("parameter_validation");

    let registry = ModuleRegistry::with_builtins();

    // Get various modules
    let debug_module = registry.get("debug").unwrap();
    let command_module = registry.get("command").unwrap();
    let copy_module = registry.get("copy").unwrap();
    let apt_module = registry.get("apt").unwrap();

    // Create valid parameters for each
    let mut debug_params = HashMap::new();
    debug_params.insert("msg".to_string(), serde_json::json!("Hello"));

    let mut command_params = HashMap::new();
    command_params.insert("cmd".to_string(), serde_json::json!("echo hello"));

    let mut copy_params = HashMap::new();
    copy_params.insert("src".to_string(), serde_json::json!("/tmp/src"));
    copy_params.insert("dest".to_string(), serde_json::json!("/tmp/dest"));

    let mut apt_params = HashMap::new();
    apt_params.insert("name".to_string(), serde_json::json!("nginx"));
    apt_params.insert("state".to_string(), serde_json::json!("present"));

    // Benchmark validate_params for each module type
    group.bench_function("validate_debug", |b| {
        b.iter(|| {
            let result = debug_module.validate_params(black_box(&debug_params));
            black_box(result)
        })
    });

    group.bench_function("validate_command", |b| {
        b.iter(|| {
            let result = command_module.validate_params(black_box(&command_params));
            black_box(result)
        })
    });

    group.bench_function("validate_copy", |b| {
        b.iter(|| {
            let result = copy_module.validate_params(black_box(&copy_params));
            black_box(result)
        })
    });

    group.bench_function("validate_apt", |b| {
        b.iter(|| {
            let result = apt_module.validate_params(black_box(&apt_params));
            black_box(result)
        })
    });

    // Benchmark required_params check
    group.bench_function("required_params_debug", |b| {
        b.iter(|| {
            let required = debug_module.required_params();
            black_box(required)
        })
    });

    group.bench_function("required_params_copy", |b| {
        b.iter(|| {
            let required = copy_module.required_params();
            black_box(required)
        })
    });

    group.finish();
}

// ============================================================================
// MODULE DISPATCH BENCHMARKS
// ============================================================================

fn bench_module_registry_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("module_dispatch_registry");

    // Benchmark registry creation
    group.bench_function("registry_creation_empty", |b| {
        b.iter(|| {
            let registry = ModuleRegistry::new();
            black_box(registry)
        })
    });

    group.bench_function("registry_creation_builtins", |b| {
        b.iter(|| {
            let registry = ModuleRegistry::with_builtins();
            black_box(registry)
        })
    });

    let registry = ModuleRegistry::with_builtins();

    // Benchmark module lookup - various common modules
    let modules_to_lookup = [
        "debug",
        "command",
        "shell",
        "copy",
        "file",
        "template",
        "apt",
        "yum",
        "dnf",
        "pip",
        "service",
        "user",
        "group",
        "lineinfile",
        "blockinfile",
        "git",
        "set_fact",
        "assert",
        "stat",
    ];

    for module_name in modules_to_lookup.iter() {
        group.bench_function(format!("lookup_{}", module_name), |b| {
            b.iter(|| {
                let module = registry.get(black_box(module_name));
                black_box(module)
            })
        });
    }

    // Benchmark lookup miss
    group.bench_function("lookup_missing", |b| {
        b.iter(|| {
            let module = registry.get(black_box("nonexistent_module_xyz"));
            black_box(module)
        })
    });

    // Benchmark contains check
    group.bench_function("contains_hit", |b| {
        b.iter(|| registry.contains(black_box("command")))
    });

    group.bench_function("contains_miss", |b| {
        b.iter(|| registry.contains(black_box("nonexistent")))
    });

    // Benchmark names listing
    group.bench_function("list_names", |b| {
        b.iter(|| {
            let names = registry.names();
            black_box(names)
        })
    });

    group.finish();
}

fn bench_module_classification(c: &mut Criterion) {
    let mut group = c.benchmark_group("module_dispatch_classification");

    let registry = ModuleRegistry::with_builtins();

    // Benchmark classification retrieval for different module types
    let classification_tests = [
        ("debug", ModuleClassification::LocalLogic),
        ("set_fact", ModuleClassification::LocalLogic),
        ("assert", ModuleClassification::LocalLogic),
        ("copy", ModuleClassification::NativeTransport),
        ("template", ModuleClassification::NativeTransport),
        ("file", ModuleClassification::NativeTransport),
        ("command", ModuleClassification::RemoteCommand),
        ("shell", ModuleClassification::RemoteCommand),
        ("apt", ModuleClassification::RemoteCommand),
        ("service", ModuleClassification::RemoteCommand),
    ];

    for (module_name, expected_class) in classification_tests.iter() {
        group.bench_function(format!("classification_{}", module_name), |b| {
            let module = registry.get(module_name).unwrap();
            b.iter(|| {
                let class = module.classification();
                assert_eq!(class, *expected_class);
                black_box(class)
            })
        });
    }

    group.finish();
}

fn bench_parallelization_hints(c: &mut Criterion) {
    let mut group = c.benchmark_group("module_dispatch_parallelization");

    let registry = ModuleRegistry::with_builtins();

    // Benchmark parallelization hint retrieval
    let hint_tests = ["debug", "command", "copy", "apt", "yum", "service", "file"];

    for module_name in hint_tests.iter() {
        group.bench_function(format!("hint_{}", module_name), |b| {
            let module = registry.get(module_name).unwrap();
            b.iter(|| {
                let hint = module.parallelization_hint();
                black_box(hint)
            })
        });
    }

    // Benchmark hint matching logic
    group.bench_function("hint_is_fully_parallel", |b| {
        let module = registry.get("debug").unwrap();
        b.iter(|| {
            let hint = module.parallelization_hint();
            matches!(hint, ParallelizationHint::FullyParallel)
        })
    });

    group.bench_function("hint_is_host_exclusive", |b| {
        let module = registry.get("apt").unwrap();
        b.iter(|| {
            let hint = module.parallelization_hint();
            matches!(hint, ParallelizationHint::HostExclusive)
        })
    });

    group.finish();
}

// ============================================================================
// EXECUTION OVERHEAD BENCHMARKS
// ============================================================================

fn bench_context_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("execution_context");

    // Benchmark context creation with different variable counts
    group.bench_function("context_empty", |b| {
        b.iter(|| {
            let ctx = ModuleContext::new();
            black_box(ctx)
        })
    });

    group.bench_function("context_default", |b| {
        b.iter(|| {
            let ctx = ModuleContext::default();
            black_box(ctx)
        })
    });

    group.bench_function("context_check_mode", |b| {
        b.iter(|| {
            let ctx = ModuleContext::new().with_check_mode(true);
            black_box(ctx)
        })
    });

    group.bench_function("context_diff_mode", |b| {
        b.iter(|| {
            let ctx = ModuleContext::new()
                .with_check_mode(true)
                .with_diff_mode(true);
            black_box(ctx)
        })
    });

    // Benchmark with different variable counts
    for num_vars in [10, 50, 100, 500].iter() {
        group.throughput(Throughput::Elements(*num_vars as u64));
        group.bench_with_input(
            BenchmarkId::new("with_vars", num_vars),
            num_vars,
            |b, &n| {
                b.iter(|| {
                    let ctx = generate_context_with_vars(black_box(n));
                    black_box(ctx)
                })
            },
        );
    }

    // Benchmark context cloning (needed for check mode execution)
    let context_with_vars = generate_context_with_vars(100);
    group.bench_function("context_clone_100_vars", |b| {
        b.iter(|| black_box(context_with_vars.clone()))
    });

    group.finish();
}

fn bench_module_execution_local_logic(c: &mut Criterion) {
    let mut group = c.benchmark_group("execution_local_logic");

    let registry = ModuleRegistry::with_builtins();
    let context = ModuleContext::new();

    // Debug module execution
    let debug_module = registry.get("debug").unwrap();
    let mut debug_params = HashMap::new();
    debug_params.insert("msg".to_string(), serde_json::json!("Hello, World!"));

    group.bench_function("debug_execute", |b| {
        b.iter(|| {
            let result = debug_module.execute(black_box(&debug_params), black_box(&context));
            black_box(result)
        })
    });

    // Debug module check mode
    let check_context = ModuleContext::new().with_check_mode(true);
    group.bench_function("debug_check", |b| {
        b.iter(|| {
            let result = debug_module.check(black_box(&debug_params), black_box(&check_context));
            black_box(result)
        })
    });

    // Set_fact module execution
    let set_fact_module = registry.get("set_fact").unwrap();
    let mut set_fact_params = HashMap::new();
    set_fact_params.insert("my_var".to_string(), serde_json::json!("my_value"));
    set_fact_params.insert("another_var".to_string(), serde_json::json!(42));

    group.bench_function("set_fact_execute", |b| {
        b.iter(|| {
            let result = set_fact_module.execute(black_box(&set_fact_params), black_box(&context));
            black_box(result)
        })
    });

    // Assert module execution
    let assert_module = registry.get("assert").unwrap();
    let mut assert_params = HashMap::new();
    assert_params.insert("that".to_string(), serde_json::json!(["true", "1 == 1"]));
    assert_params.insert(
        "success_msg".to_string(),
        serde_json::json!("All assertions passed"),
    );

    group.bench_function("assert_execute", |b| {
        b.iter(|| {
            let result = assert_module.execute(black_box(&assert_params), black_box(&context));
            black_box(result)
        })
    });

    group.finish();
}

fn bench_module_execution_check_mode_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("execution_check_mode_overhead");

    let registry = ModuleRegistry::with_builtins();
    let normal_context = ModuleContext::new();
    let check_context = ModuleContext::new().with_check_mode(true);

    // Command module - compare normal vs check mode
    let command_module = registry.get("command").unwrap();
    let mut command_params = HashMap::new();
    command_params.insert("cmd".to_string(), serde_json::json!("echo hello"));
    command_params.insert("creates".to_string(), serde_json::json!("/")); // Will skip

    group.bench_function("command_normal_skip", |b| {
        b.iter(|| {
            let result =
                command_module.execute(black_box(&command_params), black_box(&normal_context));
            black_box(result)
        })
    });

    group.bench_function("command_check_mode", |b| {
        b.iter(|| {
            let result =
                command_module.check(black_box(&command_params), black_box(&check_context));
            black_box(result)
        })
    });

    // File module - compare normal vs check mode
    let file_module = registry.get("file").unwrap();
    let mut file_params = HashMap::new();
    file_params.insert("path".to_string(), serde_json::json!("/tmp/test_file"));
    file_params.insert("state".to_string(), serde_json::json!("touch"));

    group.bench_function("file_check_mode", |b| {
        b.iter(|| {
            let result = file_module.check(black_box(&file_params), black_box(&check_context));
            black_box(result)
        })
    });

    group.finish();
}

// ============================================================================
// RESULT SERIALIZATION BENCHMARKS
// ============================================================================

fn bench_result_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialization_result_creation");

    // ModuleOutput creation - different methods
    group.bench_function("output_ok", |b| {
        b.iter(|| {
            let output = ModuleOutput::ok(black_box("Operation succeeded"));
            black_box(output)
        })
    });

    group.bench_function("output_changed", |b| {
        b.iter(|| {
            let output = ModuleOutput::changed(black_box("File modified"));
            black_box(output)
        })
    });

    group.bench_function("output_failed", |b| {
        b.iter(|| {
            let output = ModuleOutput::failed(black_box("Operation failed: permission denied"));
            black_box(output)
        })
    });

    group.bench_function("output_skipped", |b| {
        b.iter(|| {
            let output = ModuleOutput::skipped(black_box("Condition not met"));
            black_box(output)
        })
    });

    // With data
    group.bench_function("output_with_data_single", |b| {
        b.iter(|| {
            let output = ModuleOutput::changed("File copied")
                .with_data("path", serde_json::json!("/etc/config.conf"));
            black_box(output)
        })
    });

    group.bench_function("output_with_data_multiple", |b| {
        b.iter(|| {
            let output = ModuleOutput::changed("File copied")
                .with_data("path", serde_json::json!("/etc/config.conf"))
                .with_data("owner", serde_json::json!("root"))
                .with_data("group", serde_json::json!("root"))
                .with_data("mode", serde_json::json!("0644"))
                .with_data("checksum", serde_json::json!("abc123def456"));
            black_box(output)
        })
    });

    // With command output
    group.bench_function("output_with_command", |b| {
        b.iter(|| {
            let output = ModuleOutput::changed("Command executed").with_command_output(
                Some("Hello, World!\n".to_string()),
                Some(String::new()),
                Some(0),
            );
            black_box(output)
        })
    });

    // With diff
    group.bench_function("output_with_diff", |b| {
        b.iter(|| {
            let diff = Diff::new("old content", "new content");
            let output = ModuleOutput::changed("File modified").with_diff(diff);
            black_box(output)
        })
    });

    group.finish();
}

fn bench_result_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialization_json");

    // Create various outputs for serialization
    let simple_output = ModuleOutput::ok("Success");

    let medium_output = ModuleOutput::changed("Configuration updated")
        .with_data("path", serde_json::json!("/etc/nginx/nginx.conf"))
        .with_data("owner", serde_json::json!("root"))
        .with_data("mode", serde_json::json!("0644"));

    let complex_output = ModuleOutput::changed("Configuration updated")
        .with_data("path", serde_json::json!("/etc/nginx/nginx.conf"))
        .with_data("owner", serde_json::json!("root"))
        .with_data("group", serde_json::json!("root"))
        .with_data("mode", serde_json::json!("0644"))
        .with_data(
            "backup",
            serde_json::json!("/etc/nginx/nginx.conf.12345.backup"),
        )
        .with_data("checksum", serde_json::json!("sha256:abc123def456789..."))
        .with_data("size", serde_json::json!(4096))
        .with_command_output(
            Some("nginx: configuration file /etc/nginx/nginx.conf test is successful".to_string()),
            Some(String::new()),
            Some(0),
        )
        .with_diff(
            Diff::new("worker_processes 4;", "worker_processes auto;")
                .with_details("@@ -1 +1 @@\n-worker_processes 4;\n+worker_processes auto;"),
        );

    group.bench_function("serialize_simple", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&simple_output));
            black_box(json)
        })
    });

    group.bench_function("serialize_medium", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&medium_output));
            black_box(json)
        })
    });

    group.bench_function("serialize_complex", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&complex_output));
            black_box(json)
        })
    });

    // Pretty print serialization
    group.bench_function("serialize_complex_pretty", |b| {
        b.iter(|| {
            let json = serde_json::to_string_pretty(black_box(&complex_output));
            black_box(json)
        })
    });

    // Benchmark deserialization
    let complex_json = serde_json::to_string(&complex_output).unwrap();
    group.bench_function("deserialize_complex", |b| {
        b.iter(|| {
            let output: ModuleOutput = serde_json::from_str(black_box(&complex_json)).unwrap();
            black_box(output)
        })
    });

    group.finish();
}

fn bench_diff_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialization_diff");

    // Simple diff
    group.bench_function("diff_simple", |b| {
        b.iter(|| {
            let diff = Diff::new(black_box("old value"), black_box("new value"));
            black_box(diff)
        })
    });

    // Diff with details
    group.bench_function("diff_with_details", |b| {
        b.iter(|| {
            let diff = Diff::new("old value", "new value")
                .with_details("--- old\n+++ new\n@@ -1 +1 @@\n-old value\n+new value");
            black_box(diff)
        })
    });

    // Large diff (simulating file content)
    let old_content = "line\n".repeat(100);
    let new_content = "modified_line\n".repeat(100);

    group.bench_function("diff_large", |b| {
        b.iter(|| {
            let diff = Diff::new(
                black_box(old_content.clone()),
                black_box(new_content.clone()),
            );
            black_box(diff)
        })
    });

    group.finish();
}

// ============================================================================
// MODULE TYPE COMPARISON BENCHMARKS
// ============================================================================

fn bench_module_type_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("module_type_comparison");

    let registry = ModuleRegistry::with_builtins();
    let context = ModuleContext::new();

    // LocalLogic modules (fastest - no I/O)
    let debug_module = registry.get("debug").unwrap();
    let mut debug_params = HashMap::new();
    debug_params.insert("msg".to_string(), serde_json::json!("Hello"));

    group.bench_function("local_logic_debug", |b| {
        b.iter(|| {
            let result = debug_module.execute(black_box(&debug_params), black_box(&context));
            black_box(result)
        })
    });

    let set_fact_module = registry.get("set_fact").unwrap();
    let mut set_fact_params = HashMap::new();
    set_fact_params.insert("my_fact".to_string(), serde_json::json!("value"));

    group.bench_function("local_logic_set_fact", |b| {
        b.iter(|| {
            let result = set_fact_module.execute(black_box(&set_fact_params), black_box(&context));
            black_box(result)
        })
    });

    // NativeTransport modules (medium - local file I/O when no connection)
    // These will skip or use local paths in test mode
    let stat_module = registry.get("stat").unwrap();
    let mut stat_params = HashMap::new();
    stat_params.insert("path".to_string(), serde_json::json!("/tmp"));

    group.bench_function("native_transport_stat", |b| {
        b.iter(|| {
            let result = stat_module.execute(black_box(&stat_params), black_box(&context));
            black_box(result)
        })
    });

    // RemoteCommand modules (test check mode to avoid actual execution)
    let command_module = registry.get("command").unwrap();
    let mut command_params = HashMap::new();
    command_params.insert("cmd".to_string(), serde_json::json!("echo hello"));
    command_params.insert("creates".to_string(), serde_json::json!("/")); // Skip via creates

    group.bench_function("remote_command_skip", |b| {
        b.iter(|| {
            let result = command_module.execute(black_box(&command_params), black_box(&context));
            black_box(result)
        })
    });

    let check_context = ModuleContext::new().with_check_mode(true);
    group.bench_function("remote_command_check", |b| {
        let mut params = HashMap::new();
        params.insert("cmd".to_string(), serde_json::json!("echo hello"));
        b.iter(|| {
            let result = command_module.check(black_box(&params), black_box(&check_context));
            black_box(result)
        })
    });

    group.finish();
}

/// Benchmark full module dispatch cycle (lookup + validate + execute)
fn bench_full_dispatch_cycle(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_dispatch_cycle");

    let registry = ModuleRegistry::with_builtins();
    let context = ModuleContext::new();

    // Debug module full cycle
    let mut debug_params = HashMap::new();
    debug_params.insert("msg".to_string(), serde_json::json!("Hello"));

    group.bench_function("debug_full_cycle", |b| {
        b.iter(|| {
            // Lookup
            let module = registry.get(black_box("debug")).unwrap();
            // Validate
            module.validate_params(black_box(&debug_params)).ok();
            // Execute
            let result = module.execute(black_box(&debug_params), black_box(&context));
            black_box(result)
        })
    });

    // Command module full cycle (with skip)
    let mut command_params = HashMap::new();
    command_params.insert("cmd".to_string(), serde_json::json!("echo hello"));
    command_params.insert("creates".to_string(), serde_json::json!("/")); // Skip

    group.bench_function("command_full_cycle_skip", |b| {
        b.iter(|| {
            let module = registry.get(black_box("command")).unwrap();
            module.validate_params(black_box(&command_params)).ok();
            let result = module.execute(black_box(&command_params), black_box(&context));
            black_box(result)
        })
    });

    // Use registry.execute() method
    group.bench_function("registry_execute_debug", |b| {
        b.iter(|| {
            let result = registry.execute(
                black_box("debug"),
                black_box(&debug_params),
                black_box(&context),
            );
            black_box(result)
        })
    });

    group.finish();
}

/// Benchmark module caching opportunities
fn bench_module_caching(c: &mut Criterion) {
    let mut group = c.benchmark_group("module_caching");

    let registry = ModuleRegistry::with_builtins();

    // Simulate repeated lookups (cache effectiveness)
    group.bench_function("repeated_lookups_same", |b| {
        b.iter(|| {
            for _ in 0..100 {
                let module = registry.get(black_box("command"));
                black_box(module);
            }
        })
    });

    // Simulate varied lookups
    let module_names = [
        "debug", "command", "shell", "copy", "file", "template", "apt", "yum", "service", "user",
    ];

    group.bench_function("repeated_lookups_varied", |b| {
        b.iter(|| {
            for name in module_names.iter().cycle().take(100) {
                let module = registry.get(black_box(name));
                black_box(module);
            }
        })
    });

    // Benchmark Arc clone (modules are Arc-wrapped)
    let module = registry.get("command").unwrap();
    group.bench_function("arc_clone", |b| {
        b.iter(|| {
            let cloned = Arc::clone(black_box(&module));
            black_box(cloned)
        })
    });

    group.finish();
}

// ============================================================================
// ANSIBLE COMPARISON BASELINE
// ============================================================================

/// Establish baseline metrics for comparison with Ansible
/// These benchmarks measure the pure Rust overhead without actual I/O
fn bench_ansible_comparison_baseline(c: &mut Criterion) {
    let mut group = c.benchmark_group("ansible_comparison");

    // Note: Ansible module execution typically includes:
    // 1. Python interpreter startup (~50-100ms for each module invocation)
    // 2. Module code loading and parsing
    // 3. JSON argument parsing
    // 4. Actual execution
    // 5. JSON result serialization
    // 6. Result parsing by controller
    //
    // Rustible eliminates steps 1-2 entirely for native modules

    let registry = ModuleRegistry::with_builtins();
    let context = ModuleContext::new();

    // Measure equivalent operations to Ansible's module execution

    // 1. Argument parsing (Ansible: json.loads)
    let args_json = r#"{"msg": "Hello, World!", "verbosity": 0}"#;
    group.bench_function("arg_parse_json", |b| {
        b.iter(|| {
            let params: ModuleParams = serde_json::from_str(black_box(args_json)).unwrap();
            black_box(params)
        })
    });

    // 2. Module dispatch + execution
    let mut debug_params = HashMap::new();
    debug_params.insert("msg".to_string(), serde_json::json!("Hello, World!"));

    group.bench_function("module_execute_debug", |b| {
        let module = registry.get("debug").unwrap();
        b.iter(|| {
            let result = module.execute(black_box(&debug_params), black_box(&context));
            black_box(result)
        })
    });

    // 3. Result serialization (Ansible: json.dumps)
    let output = ModuleOutput::changed("File modified")
        .with_data("path", serde_json::json!("/etc/config.conf"))
        .with_data("changed", serde_json::json!(true));

    group.bench_function("result_serialize_json", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&output)).unwrap();
            black_box(json)
        })
    });

    // 4. Complete round-trip (parse args -> execute -> serialize result)
    group.bench_function("full_roundtrip", |b| {
        let module = registry.get("debug").unwrap();
        b.iter(|| {
            // Parse args
            let params: ModuleParams = serde_json::from_str(black_box(args_json)).unwrap();
            // Execute
            let result = module
                .execute(black_box(&params), black_box(&context))
                .unwrap();
            // Serialize result
            let json = serde_json::to_string(&result).unwrap();
            black_box(json)
        })
    });

    // Reference: Ansible typical timings for comparison
    // - Python interpreter startup: 50-100ms
    // - Simple module (debug): 100-200ms total
    // - Complex module (template): 200-500ms total
    // - Package module (apt/yum): 1-10s (mostly waiting for package manager)
    //
    // Rustible targets:
    // - LocalLogic modules: < 1ms (1000x faster than Ansible)
    // - NativeTransport modules: < 10ms for small files
    // - RemoteCommand modules: ~same as Ansible (limited by SSH/network)

    group.finish();
}

// ============================================================================
// CRITERION GROUPS AND MAIN
// ============================================================================

criterion_group!(
    parameter_benches,
    bench_validate_command_args,
    bench_parameter_parsing,
    bench_parameter_validation_overhead,
);

criterion_group!(
    dispatch_benches,
    bench_module_registry_operations,
    bench_module_classification,
    bench_parallelization_hints,
);

criterion_group!(
    execution_benches,
    bench_context_creation,
    bench_module_execution_local_logic,
    bench_module_execution_check_mode_overhead,
);

criterion_group!(
    serialization_benches,
    bench_result_creation,
    bench_result_serialization,
    bench_diff_generation,
);

criterion_group!(
    comparison_benches,
    bench_module_type_comparison,
    bench_full_dispatch_cycle,
    bench_module_caching,
    bench_ansible_comparison_baseline,
);

criterion_main!(
    parameter_benches,
    dispatch_benches,
    execution_benches,
    serialization_benches,
    comparison_benches,
);
