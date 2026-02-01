//! Benchmark command implementation.

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use rustible::benchmarks::{
    BenchmarkConfig, BenchmarkResult, BenchmarkRunner, BenchmarkScenarioSet,
    ComparisonPlaybookConfig,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

use super::CommandContext;

#[derive(Parser, Debug, Clone)]
pub struct BenchArgs {
    /// Benchmark suite to run
    #[arg(long, default_value = "simulated")]
    pub suite: BenchSuite,

    /// Inventory file for comparison benchmarks (auto-generated if omitted)
    #[arg(long)]
    pub inventory: Option<PathBuf>,

    /// Root directory for comparison playbooks
    #[arg(long, default_value = "benchmarks/comparison")]
    pub comparison_dir: PathBuf,

    /// Host count for generated inventory (comparison suite)
    #[arg(long, default_value_t = 10)]
    pub host_count: usize,

    /// Task count (simulated suite)
    #[arg(long, default_value_t = 20)]
    pub task_count: usize,

    /// Number of iterations per scenario
    #[arg(long, default_value_t = 3)]
    pub iterations: usize,

    /// Compare against Ansible where available
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    pub compare_ansible: bool,

    /// Output directory for benchmark results
    #[arg(long)]
    pub output_dir: Option<PathBuf>,

    /// Baseline summary JSON for regression checks
    #[arg(long)]
    pub baseline: Option<PathBuf>,

    /// Budget config TOML file for regression checks
    #[arg(long)]
    pub budgets: Option<PathBuf>,

    /// Rustible binary to invoke (comparison suite)
    #[arg(long)]
    pub rustible_bin: Option<PathBuf>,

    /// Ansible-playbook binary to invoke (comparison suite)
    #[arg(long)]
    pub ansible_bin: Option<PathBuf>,

    /// Forks/parallelism to use
    #[arg(long, default_value_t = 10)]
    pub forks: usize,
}

#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum BenchSuite {
    Simulated,
    Comparison,
}

impl BenchArgs {
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        ctx.output.banner("RUSTIBLE BENCHMARKS");

        let mut config = BenchmarkConfig {
            host_count: self.host_count,
            task_count: self.task_count,
            iterations: self.iterations,
            compare_with_ansible: self.compare_ansible,
            output_dir: self
                .output_dir
                .clone()
                .unwrap_or_else(|| PathBuf::from("benchmarks/results")),
            scenario_set: BenchmarkScenarioSet::Simulated,
            forks: self.forks,
        };

        // Keep temp inventory alive for the duration of the run
        let mut temp_inventory: Option<NamedTempFile> = None;

        if matches!(self.suite, BenchSuite::Comparison) {
            let inventory_path = if let Some(path) = &self.inventory {
                path.clone()
            } else {
                let file = generate_local_inventory(self.host_count)?;
                let path = file.path().to_path_buf();
                temp_inventory = Some(file);
                path
            };

            let mut env = HashMap::new();
            env.insert("RUSTIBLE_BENCH".to_string(), "1".to_string());

            config.scenario_set = BenchmarkScenarioSet::ComparisonPlaybooks(ComparisonPlaybookConfig {
                root_dir: self.comparison_dir.clone(),
                inventory_path,
                rustible_bin: self
                    .rustible_bin
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("rustible")),
                ansible_bin: self
                    .ansible_bin
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("ansible-playbook")),
                forks: self.forks,
                rustible_args: Vec::new(),
                ansible_args: Vec::new(),
                env,
            });
        }

        let runner = BenchmarkRunner::new(config);
        let results = runner
            .run_all()
            .await
            .context("Benchmark execution failed")?;

        if let Some(budget_path) = &self.budgets {
            let baseline = self
                .baseline
                .as_ref()
                .map(|path| load_baseline(path))
                .transpose()?;
            let violations = check_budgets(&results, baseline.as_ref(), budget_path)?;
            if !violations.is_empty() {
                for violation in violations {
                    ctx.output.warning(&violation);
                }
                ctx.output.error("Performance regression budgets failed");
                return Ok(2);
            }
        }

        drop(temp_inventory);

        ctx.output.success("Benchmarks completed");
        Ok(0)
    }
}

fn generate_local_inventory(host_count: usize) -> Result<NamedTempFile> {
    let mut file = NamedTempFile::new().context("Failed to create temp inventory file")?;

    use std::io::Write;
    writeln!(file, "[benchmark]")?;
    for i in 0..host_count {
        writeln!(
            file,
            "host{:05} ansible_host=127.0.0.1 ansible_connection=local",
            i
        )?;
    }
    writeln!(file, "\n[benchmark:vars]\nansible_connection=local")?;
    Ok(file)
}

#[derive(Debug, Deserialize)]
struct BudgetFile {
    #[serde(default)]
    defaults: BudgetDefaults,
    #[serde(default)]
    scenarios: HashMap<String, ScenarioBudget>,
}

#[derive(Debug, Default, Deserialize, Clone)]
struct BudgetDefaults {
    max_regression_pct: Option<f64>,
    max_rustible_secs: Option<f64>,
    min_speedup: Option<f64>,
}

#[derive(Debug, Default, Deserialize, Clone)]
struct ScenarioBudget {
    max_regression_pct: Option<f64>,
    max_rustible_secs: Option<f64>,
    min_speedup: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct BaselineSummary {
    results: Vec<BenchmarkResult>,
}

fn load_baseline(path: &Path) -> Result<BaselineSummary> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read baseline summary: {}", path.display()))?;
    let summary: BaselineSummary = serde_json::from_str(&content)?;
    Ok(summary)
}

fn check_budgets(
    results: &[BenchmarkResult],
    baseline: Option<&BaselineSummary>,
    budget_path: &Path,
) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(budget_path)
        .with_context(|| format!("Failed to read budget file: {}", budget_path.display()))?;
    let budgets: BudgetFile = toml::from_str(&content)?;

    let baseline_map: HashMap<&str, &BenchmarkResult> = baseline
        .map(|summary| {
            summary
                .results
                .iter()
                .map(|r| (r.name.as_str(), r))
                .collect()
        })
        .unwrap_or_default();

    let mut violations = Vec::new();

    for result in results {
        let scenario_budget = budgets
            .scenarios
            .get(&result.name)
            .cloned()
            .unwrap_or_default();
        let max_regression_pct = scenario_budget
            .max_regression_pct
            .or(budgets.defaults.max_regression_pct);
        let max_rustible_secs = scenario_budget
            .max_rustible_secs
            .or(budgets.defaults.max_rustible_secs);
        let min_speedup = scenario_budget
            .min_speedup
            .or(budgets.defaults.min_speedup);

        let rustible_secs = result.rustible_time.as_secs_f64();

        if let Some(limit) = max_rustible_secs {
            if rustible_secs > limit {
                violations.push(format!(
                    "{} exceeded max time: {:.2}s > {:.2}s",
                    result.name, rustible_secs, limit
                ));
            }
        }

        if let Some(min_speedup) = min_speedup {
            if let Some(speedup) = result.speedup {
                if speedup < min_speedup {
                    violations.push(format!(
                        "{} speedup below budget: {:.2}x < {:.2}x",
                        result.name, speedup, min_speedup
                    ));
                }
            }
        }

        if let Some(max_regression_pct) = max_regression_pct {
            if let Some(baseline_result) = baseline_map.get(result.name.as_str()) {
                let baseline_secs = baseline_result.rustible_time.as_secs_f64();
                if baseline_secs > 0.0 {
                    let allowed = baseline_secs * (1.0 + max_regression_pct / 100.0);
                    if rustible_secs > allowed {
                        violations.push(format!(
                            "{} regressed {:.1}% (baseline {:.2}s, now {:.2}s, budget {:.1}%)",
                            result.name,
                            ((rustible_secs / baseline_secs) - 1.0) * 100.0,
                            baseline_secs,
                            rustible_secs,
                            max_regression_pct
                        ));
                    }
                }
            }
        }
    }

    Ok(violations)
}
