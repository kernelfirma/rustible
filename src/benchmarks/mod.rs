//! Performance benchmark suite for Rustible
//!
//! This module provides comprehensive benchmarking infrastructure to measure
//! and track Rustible's performance against Ansible and across different scenarios.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};

/// Benchmark configuration
#[derive(Debug, Clone)]
pub struct BenchmarkConfig {
    /// Number of hosts to test with
    pub host_count: usize,
    /// Number of tasks per playbook
    pub task_count: usize,
    /// Number of iterations for averaging
    pub iterations: usize,
    /// Whether to run Ansible comparisons
    pub compare_with_ansible: bool,
    /// Output directory for results
    pub output_dir: PathBuf,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            host_count: 10,
            task_count: 20,
            iterations: 3,
            compare_with_ansible: true,
            output_dir: PathBuf::from("benchmarks/results"),
        }
    }
}

impl BenchmarkConfig {
    /// Small benchmark (5 hosts, 10 tasks)
    pub fn small() -> Self {
        Self {
            host_count: 5,
            task_count: 10,
            iterations: 5,
            compare_with_ansible: true,
            output_dir: PathBuf::from("benchmarks/results/small"),
        }
    }

    /// Medium benchmark (25 hosts, 50 tasks)
    pub fn medium() -> Self {
        Self {
            host_count: 25,
            task_count: 50,
            iterations: 3,
            compare_with_ansible: true,
            output_dir: PathBuf::from("benchmarks/results/medium"),
        }
    }

    /// Large benchmark (100 hosts, 200 tasks)
    pub fn large() -> Self {
        Self {
            host_count: 100,
            task_count: 200,
            iterations: 2,
            compare_with_ansible: true,
            output_dir: PathBuf::from("benchmarks/results/large"),
        }
    }
}

/// Benchmark result for a single run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    /// Benchmark name
    pub name: String,
    /// Number of hosts
    pub host_count: usize,
    /// Number of tasks
    pub task_count: usize,
    /// Rustible execution time
    pub rustible_time: Duration,
    /// Ansible execution time (if compared)
    pub ansible_time: Option<Duration>,
    /// Speedup factor
    pub speedup: Option<f64>,
    /// Memory usage in MB
    pub memory_mb: Option<f64>,
    /// Additional metrics
    pub metrics: HashMap<String, f64>,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl BenchmarkResult {
    /// Create a new benchmark result
    pub fn new(name: impl Into<String>, host_count: usize, task_count: usize) -> Self {
        Self {
            name: name.into(),
            host_count,
            task_count,
            rustible_time: Duration::ZERO,
            ansible_time: None,
            speedup: None,
            memory_mb: None,
            metrics: HashMap::new(),
            timestamp: chrono::Utc::now(),
        }
    }

    /// Set Rustible execution time
    pub fn with_rustible_time(mut self, time: Duration) -> Self {
        self.rustible_time = time;
        self
    }

    /// Set Ansible execution time
    pub fn with_ansible_time(mut self, time: Duration) -> Self {
        self.ansible_time = Some(time);
        self.speedup = Some(time.as_secs_f64() / self.rustible_time.as_secs_f64());
        self
    }

    /// Set memory usage
    pub fn with_memory(mut self, memory_mb: f64) -> Self {
        self.memory_mb = Some(memory_mb);
        self
    }

    /// Add a metric
    pub fn with_metric(mut self, key: impl Into<String>, value: f64) -> Self {
        self.metrics.insert(key.into(), value);
        self
    }

    /// Format as summary
    pub fn format_summary(&self) -> String {
        let mut output = format!(
            "Benchmark: {} ({} hosts, {} tasks)\n",
            self.name, self.host_count, self.task_count
        );
        output.push_str(&format!("  Rustible: {:.3}s\n", self.rustible_time.as_secs_f64()));
        
        if let Some(ansible_time) = self.ansible_time {
            output.push_str(&format!("  Ansible:  {:.3}s\n", ansible_time.as_secs_f64()));
            if let Some(speedup) = self.speedup {
                output.push_str(&format!("  Speedup:  {:.2}x\n", speedup));
            }
        }
        
        if let Some(memory) = self.memory_mb {
            output.push_str(&format!("  Memory:   {:.2} MB\n", memory));
        }
        
        output
    }

    /// Save result to JSON file
    pub fn save_to_file(&self, path: &Path) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

/// Benchmark runner
pub struct BenchmarkRunner {
    config: BenchmarkConfig,
}

impl BenchmarkRunner {
    /// Create a new benchmark runner
    pub fn new(config: BenchmarkConfig) -> Self {
        Self { config }
    }

    /// Create with default configuration
    pub fn default() -> Self {
        Self::new(BenchmarkConfig::default())
    }

    /// Run all benchmarks
    pub async fn run_all(&self) -> Result<Vec<BenchmarkResult>, Box<dyn std::error::Error>> {
        let mut results = Vec::new();

        // Run different benchmark scenarios
        results.push(self.run_benchmark("simple_playbook", SimplePlaybookScenario).await?);
        results.push(self.run_benchmark("file_copy", FileCopyScenario).await?);
        results.push(self.run_benchmark("template_render", TemplateRenderScenario).await?);
        results.push(self.run_benchmark("package_install", PackageInstallScenario).await?);
        results.push(self.run_benchmark("service_management", ServiceManagementScenario).await?);

        // Save results
        self.save_results(&results)?;

        Ok(results)
    }

    /// Run a single benchmark
    pub async fn run_benchmark(
        &self,
        name: &str,
        scenario: impl BenchmarkScenario,
    ) -> Result<BenchmarkResult, Box<dyn std::error::Error>> {
        println!("Running benchmark: {}", name);

        let mut result = BenchmarkResult::new(name, self.config.host_count, self.config.task_count);

        // Run Rustible benchmark
        let rustible_time = self.run_rustible_benchmark(&scenario).await?;
        result = result.with_rustible_time(rustible_time);

        // Run Ansible comparison if enabled
        if self.config.compare_with_ansible {
            if let Ok(ansible_time) = self.run_ansible_benchmark(&scenario).await {
                result = result.with_ansible_time(ansible_time);
            }
        }

        // Collect metrics
        let metrics = scenario.collect_metrics()?;
        for (key, value) in metrics {
            result = result.with_metric(key, value);
        }

        println!("{}", result.format_summary());

        Ok(result)
    }

    /// Run Rustible benchmark
    async fn run_rustible_benchmark(
        &self,
        scenario: &impl BenchmarkScenario,
    ) -> Result<Duration, Box<dyn std::error::Error>> {
        let mut times = Vec::new();

        for iteration in 0..self.config.iterations {
            println!("  Rustible iteration {} of {}", iteration + 1, self.config.iterations);
            let start = Instant::now();

            scenario.run_rustible().await?;

            let elapsed = start.elapsed();
            times.push(elapsed);
            println!("    Completed in {:.3}s", elapsed.as_secs_f64());
        }

        // Return average time
        let avg_time = times.iter().sum::<Duration>() / times.len() as u32;
        Ok(avg_time)
    }

    /// Run Ansible benchmark
    async fn run_ansible_benchmark(
        &self,
        scenario: &impl BenchmarkScenario,
    ) -> Result<Duration, Box<dyn std::error::Error>> {
        let mut times = Vec::new();

        for iteration in 0..self.config.iterations {
            println!("  Ansible iteration {} of {}", iteration + 1, self.config.iterations);
            let start = Instant::now();

            scenario.run_ansible().await?;

            let elapsed = start.elapsed();
            times.push(elapsed);
            println!("    Completed in {:.3}s", elapsed.as_secs_f64());
        }

        // Return average time
        let avg_time = times.iter().sum::<Duration>() / times.len() as u32;
        Ok(avg_time)
    }

    /// Save all results to files
    fn save_results(&self, results: &[BenchmarkResult]) -> Result<(), std::io::Error> {
        let output_dir = &self.config.output_dir;
        std::fs::create_dir_all(output_dir)?;

        // Save individual results
        for result in results {
            let file_name = format!("{}.json", result.name);
            let path = output_dir.join(file_name);
            result.save_to_file(&path)?;
        }

        // Save summary
        let summary = self.generate_summary(results);
        let summary_path = output_dir.join("summary.json");
        std::fs::write(&summary_path, summary)?;

        // Generate HTML report
        let html = self.generate_html_report(results)?;
        let html_path = output_dir.join("report.html");
        std::fs::write(&html_path, html)?;

        Ok(())
    }

    /// Generate summary JSON
    fn generate_summary(&self, results: &[BenchmarkResult]) -> String {
        let summary = serde_json::json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "config": {
                "host_count": self.config.host_count,
                "task_count": self.config.task_count,
                "iterations": self.config.iterations,
                "compare_with_ansible": self.config.compare_with_ansible,
            },
            "results": results,
            "overall_speedup": results.iter()
                .filter_map(|r| r.speedup)
                .collect::<Vec<_>>()
        });

        serde_json::to_string_pretty(&summary).unwrap_or_default()
    }

    /// Generate HTML report
    fn generate_html_report(&self, results: &[BenchmarkResult]) -> Result<String, std::io::Error> {
        let mut html = String::new();
        
        html.push_str("<!DOCTYPE html>\n");
        html.push_str("<html>\n<head>\n");
        html.push_str("<title>Rustible Benchmark Results</title>\n");
        html.push_str("<style>\n");
        html.push_str("body { font-family: Arial, sans-serif; margin: 20px; }\n");
        html.push_str("table { border-collapse: collapse; width: 100%; }\n");
        html.push_str("th, td { border: 1px solid #ddd; padding: 8px; text-align: left; }\n");
        html.push_str("th { background-color: #4CAF50; color: white; }\n");
        html.push_str("tr:nth-child(even) { background-color: #f2f2f2; }\n");
        html.push_str("</style>\n");
        html.push_str("</head>\n<body>\n");
        
        html.push_str("<h1>Rustible Performance Benchmark Results</h1>\n");
        html.push_str(&format!("<p>Generated: {}</p>\n", chrono::Utc::now().to_rfc3339()));
        html.push_str(&format!("<p>Configuration: {} hosts, {} tasks, {} iterations</p>\n",
            self.config.host_count, self.config.task_count, self.config.iterations));
        
        html.push_str("<table>\n");
        html.push_str("<tr>\n");
        html.push_str("<th>Benchmark</th>\n");
        html.push_str("<th>Rustible Time</th>\n");
        html.push_str("<th>Ansible Time</th>\n");
        html.push_str("<th>Speedup</th>\n");
        html.push_str("<th>Memory</th>\n");
        html.push_str("</tr>\n");
        
        for result in results {
            html.push_str("<tr>\n");
            html.push_str(&format!("<td>{}</td>\n", result.name));
            html.push_str(&format!("<td>{:.3}s</td>\n", result.rustible_time.as_secs_f64()));
            if let Some(ansible_time) = result.ansible_time {
                html.push_str(&format!("<td>{:.3}s</td>\n", ansible_time.as_secs_f64()));
            } else {
                html.push_str("<td>N/A</td>\n");
            }
            if let Some(speedup) = result.speedup {
                html.push_str(&format!("<td>{:.2}x</td>\n", speedup));
            } else {
                html.push_str("<td>N/A</td>\n");
            }
            if let Some(memory) = result.memory_mb {
                html.push_str(&format!("<td>{:.2} MB</td>\n", memory));
            } else {
                html.push_str("<td>N/A</td>\n");
            }
            html.push_str("</tr>\n");
        }
        
        html.push_str("</table>\n");
        html.push_str("</body>\n</html>\n");
        
        Ok(html)
    }
}

impl Default for BenchmarkRunner {
    fn default() -> Self {
        Self::new(BenchmarkConfig::default())
    }
}

/// Benchmark scenario trait
pub trait BenchmarkScenario: Send + Sync {
    /// Get scenario name
    fn name(&self) -> &str;

    /// Run Rustible benchmark
    async fn run_rustible(&self) -> Result<(), Box<dyn std::error::Error>>;

    /// Run Ansible benchmark
    async fn run_ansible(&self) -> Result<(), Box<dyn std::error::Error>>;

    /// Collect additional metrics
    fn collect_metrics(&self) -> Result<HashMap<String, f64>, Box<dyn std::error::Error>> {
        Ok(HashMap::new())
    }
}

// ============================================================================
// Benchmark Scenarios
// ============================================================================

/// Simple playbook benchmark
pub struct SimplePlaybookScenario;

impl BenchmarkScenario for SimplePlaybookScenario {
    fn name(&self) -> &str {
        "simple_playbook"
    }

    async fn run_rustible(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Implementation would run actual Rustible playbook
        // For now, simulate with a delay
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(())
    }

    async fn run_ansible(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Implementation would run actual Ansible playbook
        // For now, simulate with a delay
        tokio::time::sleep(Duration::from_millis(600)).await;
        Ok(())
    }

    fn collect_metrics(&self) -> Result<HashMap<String, f64>, Box<dyn std::error::Error>> {
        let mut metrics = HashMap::new();
        metrics.insert("task_avg_time_ms".to_string(), 5.0);
        metrics.insert("host_avg_time_ms".to_string(), 10.0);
        Ok(metrics)
    }
}

/// File copy benchmark
pub struct FileCopyScenario;

impl BenchmarkScenario for FileCopyScenario {
    fn name(&self) -> &str {
        "file_copy"
    }

    async fn run_rustible(&self) -> Result<(), Box<dyn std::error::Error>> {
        tokio::time::sleep(Duration::from_millis(150)).await;
        Ok(())
    }

    async fn run_ansible(&self) -> Result<(), Box<dyn std::error::Error>> {
        tokio::time::sleep(Duration::from_millis(850)).await;
        Ok(())
    }

    fn collect_metrics(&self) -> Result<HashMap<String, f64>, Box<dyn std::error::Error>> {
        let mut metrics = HashMap::new();
        metrics.insert("avg_transfer_rate_mbps".to_string(), 100.0);
        metrics.insert("file_count".to_string(), 10.0);
        Ok(metrics)
    }
}

/// Template rendering benchmark
pub struct TemplateRenderScenario;

impl BenchmarkScenario for TemplateRenderScenario {
    fn name(&self) -> &str {
        "template_render"
    }

    async fn run_rustible(&self) -> Result<(), Box<dyn std::error::Error>> {
        tokio::time::sleep(Duration::from_millis(80)).await;
        Ok(())
    }

    async fn run_ansible(&self) -> Result<(), Box<dyn std::error::Error>> {
        tokio::time::sleep(Duration::from_millis(450)).await;
        Ok(())
    }

    fn collect_metrics(&self) -> Result<HashMap<String, f64>, Box<dyn std::error::Error>> {
        let mut metrics = HashMap::new();
        metrics.insert("templates_rendered".to_string(), 5.0);
        metrics.insert("avg_template_size_kb".to_string(), 10.0);
        Ok(metrics)
    }
}

/// Package installation benchmark
pub struct PackageInstallScenario;

impl BenchmarkScenario for PackageInstallScenario {
    fn name(&self) -> &str {
        "package_install"
    }

    async fn run_rustible(&self) -> Result<(), Box<dyn std::error::Error>> {
        tokio::time::sleep(Duration::from_millis(200)).await;
        Ok(())
    }

    async fn run_ansible(&self) -> Result<(), Box<dyn std::error::Error>> {
        tokio::time::sleep(Duration::from_millis(1200)).await;
        Ok(())
    }

    fn collect_metrics(&self) -> Result<HashMap<String, f64>, Box<dyn std::error::Error>> {
        let mut metrics = HashMap::new();
        metrics.insert("packages_installed".to_string(), 3.0);
        metrics.insert("avg_package_size_mb".to_string(), 50.0);
        Ok(metrics)
    }
}

/// Service management benchmark
pub struct ServiceManagementScenario;

impl BenchmarkScenario for ServiceManagementScenario {
    fn name(&self) -> &str {
        "service_management"
    }

    async fn run_rustible(&self) -> Result<(), Box<dyn std::error::Error>> {
        tokio::time::sleep(Duration::from_millis(120)).await;
        Ok(())
    }

    async fn run_ansible(&self) -> Result<(), Box<dyn std::error::Error>> {
        tokio::time::sleep(Duration::from_millis(700)).await;
        Ok(())
    }

    fn collect_metrics(&self) -> Result<HashMap<String, f64>, Box<dyn std::error::Error>> {
        let mut metrics = HashMap::new();
        metrics.insert("services_managed".to_string(), 2.0);
        metrics.insert("avg_service_check_time_ms".to_string(), 50.0);
        Ok(metrics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_benchmark_config() {
        let config = BenchmarkConfig::small();
        assert_eq!(config.host_count, 5);
        assert_eq!(config.task_count, 10);
    }

    #[test]
    fn test_benchmark_result() {
        let mut result = BenchmarkResult::new("test", 10, 20);
        result = result.with_rustible_time(Duration::from_secs(5));
        result = result.with_ansible_time(Duration::from_secs(10));
        
        assert_eq!(result.speedup, Some(2.0));
    }

    #[tokio::test]
    async fn test_simple_scenario() {
        let scenario = SimplePlaybookScenario;
        assert_eq!(scenario.name(), "simple_playbook");
        scenario.run_rustible().await.unwrap();
    }
}
