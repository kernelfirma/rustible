//! Ansible Parity Scorecard Test Suite for Issue #284
//!
//! Tracks usage-weighted Ansible parity scores per module/feature.
//! CI fails on regressions - scores must not decrease.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

// ============================================================================
// Parity Scoring System
// ============================================================================

/// Feature parity status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParityStatus {
    /// Feature fully implemented and tested
    Full,
    /// Feature partially implemented
    Partial,
    /// Feature planned but not yet implemented
    Planned,
    /// Feature not applicable to Rust implementation
    NotApplicable,
    /// Feature explicitly not planned
    NotPlanned,
}

impl ParityStatus {
    fn score(&self) -> f64 {
        match self {
            ParityStatus::Full => 1.0,
            ParityStatus::Partial => 0.5,
            ParityStatus::Planned => 0.0,
            ParityStatus::NotApplicable => 1.0, // Doesn't count against
            ParityStatus::NotPlanned => 0.0,
        }
    }

    fn counts_in_total(&self) -> bool {
        !matches!(self, ParityStatus::NotApplicable)
    }
}

/// Feature definition with parity tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feature {
    pub name: String,
    pub description: String,
    pub status: ParityStatus,
    pub weight: f64, // Usage weight (0.0 - 1.0)
    pub notes: Option<String>,
}

impl Feature {
    fn new(name: &str, description: &str, status: ParityStatus, weight: f64) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            status,
            weight,
            notes: None,
        }
    }

    fn with_note(mut self, note: &str) -> Self {
        self.notes = Some(note.to_string());
        self
    }

    fn weighted_score(&self) -> f64 {
        self.status.score() * self.weight
    }
}

/// Module parity definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleParity {
    pub name: String,
    pub description: String,
    pub usage_weight: f64, // How commonly used (0.0 - 1.0)
    pub features: Vec<Feature>,
}

impl ModuleParity {
    fn new(name: &str, description: &str, usage_weight: f64) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            usage_weight,
            features: Vec::new(),
        }
    }

    fn add_feature(&mut self, feature: Feature) -> &mut Self {
        self.features.push(feature);
        self
    }

    /// Calculate module parity score (0.0 - 1.0)
    fn score(&self) -> f64 {
        let applicable_features: Vec<&Feature> = self
            .features
            .iter()
            .filter(|f| f.status.counts_in_total())
            .collect();

        if applicable_features.is_empty() {
            return 1.0; // No features to measure
        }

        let total_weight: f64 = applicable_features.iter().map(|f| f.weight).sum();
        if total_weight == 0.0 {
            return 0.0;
        }

        let weighted_score: f64 = applicable_features.iter().map(|f| f.weighted_score()).sum();
        weighted_score / total_weight
    }

    /// Calculate weighted score for overall parity
    fn weighted_score(&self) -> f64 {
        self.score() * self.usage_weight
    }

    /// Count of fully implemented features
    fn full_count(&self) -> usize {
        self.features.iter().filter(|f| f.status == ParityStatus::Full).count()
    }

    /// Count of partially implemented features
    fn partial_count(&self) -> usize {
        self.features.iter().filter(|f| f.status == ParityStatus::Partial).count()
    }

    /// Total applicable feature count
    fn total_count(&self) -> usize {
        self.features.iter().filter(|f| f.status.counts_in_total()).count()
    }
}

/// Overall parity scorecard
#[derive(Debug, Serialize, Deserialize)]
pub struct ParityScorecard {
    pub modules: Vec<ModuleParity>,
    /// Minimum acceptable overall score (CI gate)
    pub minimum_score: f64,
}

impl ParityScorecard {
    fn new(minimum_score: f64) -> Self {
        Self {
            modules: Vec::new(),
            minimum_score,
        }
    }

    fn add_module(&mut self, module: ModuleParity) -> &mut Self {
        self.modules.push(module);
        self
    }

    /// Calculate overall parity score (0.0 - 1.0)
    fn overall_score(&self) -> f64 {
        let total_weight: f64 = self.modules.iter().map(|m| m.usage_weight).sum();
        if total_weight == 0.0 {
            return 0.0;
        }

        let weighted_score: f64 = self.modules.iter().map(|m| m.weighted_score()).sum();
        weighted_score / total_weight
    }

    /// Check if scorecard passes CI gate
    fn passes_ci(&self) -> bool {
        self.overall_score() >= self.minimum_score
    }

    /// Check for regressions against baseline
    fn has_regressions(&self, baseline: &ParityScorecard) -> Vec<String> {
        let mut regressions = Vec::new();

        // Check overall score regression
        if self.overall_score() < baseline.overall_score() - 0.001 {
            regressions.push(format!(
                "Overall score regressed from {:.1}% to {:.1}%",
                baseline.overall_score() * 100.0,
                self.overall_score() * 100.0
            ));
        }

        // Check per-module regressions
        let baseline_modules: HashMap<&str, &ModuleParity> = baseline
            .modules
            .iter()
            .map(|m| (m.name.as_str(), m))
            .collect();

        for module in &self.modules {
            if let Some(baseline_module) = baseline_modules.get(module.name.as_str()) {
                if module.score() < baseline_module.score() - 0.001 {
                    regressions.push(format!(
                        "Module '{}' regressed from {:.1}% to {:.1}%",
                        module.name,
                        baseline_module.score() * 100.0,
                        module.score() * 100.0
                    ));
                }
            }
        }

        regressions
    }

    /// Generate markdown scorecard
    fn to_markdown(&self) -> String {
        let mut md = String::new();

        md.push_str("# Ansible Parity Scorecard\n\n");
        md.push_str(&format!("**Overall Score: {:.1}%**\n\n", self.overall_score() * 100.0));
        md.push_str(&format!("CI Gate: {:.1}%\n\n", self.minimum_score * 100.0));

        md.push_str("## Module Scores\n\n");
        md.push_str("| Module | Score | Full | Partial | Total | Weight |\n");
        md.push_str("|--------|-------|------|---------|-------|--------|\n");

        for module in &self.modules {
            md.push_str(&format!(
                "| {} | {:.1}% | {} | {} | {} | {:.0}% |\n",
                module.name,
                module.score() * 100.0,
                module.full_count(),
                module.partial_count(),
                module.total_count(),
                module.usage_weight * 100.0
            ));
        }

        md
    }

    /// Generate JSON scorecard for CI
    fn to_json(&self) -> String {
        let mut modules_json = Vec::new();
        for module in &self.modules {
            modules_json.push(format!(
                r#"    {{"name": "{}", "score": {:.3}, "full": {}, "partial": {}, "total": {}, "weight": {:.2}}}"#,
                module.name,
                module.score(),
                module.full_count(),
                module.partial_count(),
                module.total_count(),
                module.usage_weight
            ));
        }

        format!(
            r#"{{
  "overall_score": {:.3},
  "minimum_score": {:.3},
  "passes_ci": {},
  "modules": [
{}
  ]
}}"#,
            self.overall_score(),
            self.minimum_score,
            self.passes_ci(),
            modules_json.join(",\n")
        )
    }
}

// ============================================================================
// Current Parity Scorecard Definition
// ============================================================================

/// Build the current parity scorecard
fn build_current_scorecard() -> ParityScorecard {
    let mut scorecard = ParityScorecard::new(0.70); // 70% minimum

    // File module (very high usage)
    let mut file_module = ModuleParity::new("file", "File and directory management", 0.95);
    file_module.add_feature(Feature::new("path", "Path management", ParityStatus::Full, 1.0));
    file_module.add_feature(Feature::new("state", "State management (file/directory/absent/link)", ParityStatus::Full, 1.0));
    file_module.add_feature(Feature::new("mode", "Permission management", ParityStatus::Full, 0.9));
    file_module.add_feature(Feature::new("owner", "Owner management", ParityStatus::Full, 0.8));
    file_module.add_feature(Feature::new("group", "Group management", ParityStatus::Full, 0.8));
    file_module.add_feature(Feature::new("recurse", "Recursive operations", ParityStatus::Full, 0.6));
    file_module.add_feature(Feature::new("src", "Source for links", ParityStatus::Full, 0.5));
    file_module.add_feature(Feature::new("follow", "Follow symlinks", ParityStatus::Partial, 0.4));
    file_module.add_feature(Feature::new("force", "Force operations", ParityStatus::Full, 0.4));
    file_module.add_feature(Feature::new("selevel", "SELinux level", ParityStatus::NotApplicable, 0.1));
    scorecard.add_module(file_module);

    // Copy module (very high usage)
    let mut copy_module = ModuleParity::new("copy", "Copy files to remote", 0.90);
    copy_module.add_feature(Feature::new("src", "Source file", ParityStatus::Full, 1.0));
    copy_module.add_feature(Feature::new("dest", "Destination path", ParityStatus::Full, 1.0));
    copy_module.add_feature(Feature::new("content", "Inline content", ParityStatus::Full, 0.9));
    copy_module.add_feature(Feature::new("backup", "Create backup", ParityStatus::Full, 0.6));
    copy_module.add_feature(Feature::new("mode", "File mode", ParityStatus::Full, 0.8));
    copy_module.add_feature(Feature::new("owner", "File owner", ParityStatus::Full, 0.7));
    copy_module.add_feature(Feature::new("group", "File group", ParityStatus::Full, 0.7));
    copy_module.add_feature(Feature::new("validate", "Validation command", ParityStatus::Partial, 0.4));
    copy_module.add_feature(Feature::new("remote_src", "Remote source", ParityStatus::Partial, 0.3));
    scorecard.add_module(copy_module);

    // Template module (very high usage)
    let mut template_module = ModuleParity::new("template", "Jinja2 template rendering", 0.90);
    template_module.add_feature(Feature::new("src", "Template source", ParityStatus::Full, 1.0));
    template_module.add_feature(Feature::new("dest", "Destination path", ParityStatus::Full, 1.0));
    template_module.add_feature(Feature::new("mode", "File mode", ParityStatus::Full, 0.8));
    template_module.add_feature(Feature::new("owner", "File owner", ParityStatus::Full, 0.7));
    template_module.add_feature(Feature::new("group", "File group", ParityStatus::Full, 0.7));
    template_module.add_feature(Feature::new("backup", "Create backup", ParityStatus::Full, 0.5));
    template_module.add_feature(Feature::new("validate", "Validation command", ParityStatus::Partial, 0.4));
    template_module.add_feature(Feature::new("block_start", "Block markers", ParityStatus::Full, 0.2));
    scorecard.add_module(template_module);

    // Package module (high usage)
    let mut package_module = ModuleParity::new("package", "Generic package management", 0.85);
    package_module.add_feature(Feature::new("name", "Package name(s)", ParityStatus::Full, 1.0));
    package_module.add_feature(Feature::new("state", "Package state", ParityStatus::Full, 1.0));
    package_module.add_feature(Feature::new("version", "Package version", ParityStatus::Full, 0.6));
    package_module.add_feature(Feature::new("use", "Package manager selection", ParityStatus::Partial, 0.4));
    scorecard.add_module(package_module);

    // Service module (high usage)
    let mut service_module = ModuleParity::new("service", "Service management", 0.85);
    service_module.add_feature(Feature::new("name", "Service name", ParityStatus::Full, 1.0));
    service_module.add_feature(Feature::new("state", "Service state", ParityStatus::Full, 1.0));
    service_module.add_feature(Feature::new("enabled", "Start on boot", ParityStatus::Full, 0.9));
    service_module.add_feature(Feature::new("pattern", "Process pattern match", ParityStatus::Partial, 0.3));
    service_module.add_feature(Feature::new("sleep", "Sleep between actions", ParityStatus::Partial, 0.2));
    scorecard.add_module(service_module);

    // Command module (high usage)
    let mut command_module = ModuleParity::new("command", "Execute commands", 0.80);
    command_module.add_feature(Feature::new("cmd", "Command string", ParityStatus::Full, 1.0));
    command_module.add_feature(Feature::new("argv", "Command as list", ParityStatus::Full, 0.7));
    command_module.add_feature(Feature::new("chdir", "Working directory", ParityStatus::Full, 0.8));
    command_module.add_feature(Feature::new("creates", "Skip if exists", ParityStatus::Full, 0.7));
    command_module.add_feature(Feature::new("removes", "Skip if not exists", ParityStatus::Full, 0.6));
    command_module.add_feature(Feature::new("stdin", "Standard input", ParityStatus::Partial, 0.3));
    scorecard.add_module(command_module);

    // Shell module (high usage)
    let mut shell_module = ModuleParity::new("shell", "Execute shell commands", 0.75);
    shell_module.add_feature(Feature::new("cmd", "Shell command", ParityStatus::Full, 1.0));
    shell_module.add_feature(Feature::new("chdir", "Working directory", ParityStatus::Full, 0.8));
    shell_module.add_feature(Feature::new("executable", "Shell executable", ParityStatus::Full, 0.5));
    shell_module.add_feature(Feature::new("creates", "Skip if exists", ParityStatus::Full, 0.6));
    shell_module.add_feature(Feature::new("removes", "Skip if not exists", ParityStatus::Full, 0.5));
    scorecard.add_module(shell_module);

    // User module (medium usage)
    let mut user_module = ModuleParity::new("user", "User management", 0.70);
    user_module.add_feature(Feature::new("name", "Username", ParityStatus::Full, 1.0));
    user_module.add_feature(Feature::new("state", "User state", ParityStatus::Full, 1.0));
    user_module.add_feature(Feature::new("uid", "User ID", ParityStatus::Full, 0.6));
    user_module.add_feature(Feature::new("groups", "Groups", ParityStatus::Full, 0.8));
    user_module.add_feature(Feature::new("shell", "Login shell", ParityStatus::Full, 0.7));
    user_module.add_feature(Feature::new("home", "Home directory", ParityStatus::Full, 0.7));
    user_module.add_feature(Feature::new("password", "Password hash", ParityStatus::Partial, 0.5));
    user_module.add_feature(Feature::new("ssh_key", "SSH key management", ParityStatus::Partial, 0.4));
    scorecard.add_module(user_module);

    // Group module (medium usage)
    let mut group_module = ModuleParity::new("group", "Group management", 0.65);
    group_module.add_feature(Feature::new("name", "Group name", ParityStatus::Full, 1.0));
    group_module.add_feature(Feature::new("state", "Group state", ParityStatus::Full, 1.0));
    group_module.add_feature(Feature::new("gid", "Group ID", ParityStatus::Full, 0.6));
    group_module.add_feature(Feature::new("system", "System group", ParityStatus::Full, 0.4));
    scorecard.add_module(group_module);

    // Lineinfile module (medium usage)
    let mut lineinfile_module = ModuleParity::new("lineinfile", "Line in file management", 0.70);
    lineinfile_module.add_feature(Feature::new("path", "File path", ParityStatus::Full, 1.0));
    lineinfile_module.add_feature(Feature::new("line", "Line content", ParityStatus::Full, 1.0));
    lineinfile_module.add_feature(Feature::new("regexp", "Regex match", ParityStatus::Full, 0.9));
    lineinfile_module.add_feature(Feature::new("state", "Line state", ParityStatus::Full, 1.0));
    lineinfile_module.add_feature(Feature::new("insertafter", "Insert after", ParityStatus::Full, 0.7));
    lineinfile_module.add_feature(Feature::new("insertbefore", "Insert before", ParityStatus::Full, 0.6));
    lineinfile_module.add_feature(Feature::new("backrefs", "Backreferences", ParityStatus::Partial, 0.4));
    lineinfile_module.add_feature(Feature::new("firstmatch", "First match only", ParityStatus::Full, 0.3));
    scorecard.add_module(lineinfile_module);

    // Debug module (high usage)
    let mut debug_module = ModuleParity::new("debug", "Debug output", 0.80);
    debug_module.add_feature(Feature::new("msg", "Message output", ParityStatus::Full, 1.0));
    debug_module.add_feature(Feature::new("var", "Variable output", ParityStatus::Full, 0.9));
    debug_module.add_feature(Feature::new("verbosity", "Verbosity level", ParityStatus::Full, 0.5));
    scorecard.add_module(debug_module);

    // Stat module (medium usage)
    let mut stat_module = ModuleParity::new("stat", "File statistics", 0.65);
    stat_module.add_feature(Feature::new("path", "File path", ParityStatus::Full, 1.0));
    stat_module.add_feature(Feature::new("follow", "Follow symlinks", ParityStatus::Full, 0.6));
    stat_module.add_feature(Feature::new("get_checksum", "Compute checksum", ParityStatus::Full, 0.8));
    stat_module.add_feature(Feature::new("checksum_algorithm", "Hash algorithm", ParityStatus::Full, 0.5));
    stat_module.add_feature(Feature::new("get_mime", "MIME type", ParityStatus::Partial, 0.3));
    stat_module.add_feature(Feature::new("get_attributes", "Extended attributes", ParityStatus::Partial, 0.2));
    scorecard.add_module(stat_module);

    // Git module (medium usage)
    let mut git_module = ModuleParity::new("git", "Git repository management", 0.60);
    git_module.add_feature(Feature::new("repo", "Repository URL", ParityStatus::Full, 1.0));
    git_module.add_feature(Feature::new("dest", "Destination path", ParityStatus::Full, 1.0));
    git_module.add_feature(Feature::new("version", "Version/branch/tag", ParityStatus::Full, 0.9));
    git_module.add_feature(Feature::new("force", "Force checkout", ParityStatus::Full, 0.6));
    git_module.add_feature(Feature::new("update", "Update existing", ParityStatus::Full, 0.7));
    git_module.add_feature(Feature::new("depth", "Clone depth", ParityStatus::Partial, 0.4));
    git_module.add_feature(Feature::new("key_file", "SSH key file", ParityStatus::Partial, 0.3));
    scorecard.add_module(git_module);

    // APT module (high usage on Debian/Ubuntu)
    let mut apt_module = ModuleParity::new("apt", "APT package management", 0.75);
    apt_module.add_feature(Feature::new("name", "Package name(s)", ParityStatus::Full, 1.0));
    apt_module.add_feature(Feature::new("state", "Package state", ParityStatus::Full, 1.0));
    apt_module.add_feature(Feature::new("update_cache", "Update cache", ParityStatus::Full, 0.9));
    apt_module.add_feature(Feature::new("cache_valid_time", "Cache validity", ParityStatus::Full, 0.6));
    apt_module.add_feature(Feature::new("deb", "Install .deb file", ParityStatus::Partial, 0.4));
    apt_module.add_feature(Feature::new("dpkg_options", "dpkg options", ParityStatus::Partial, 0.3));
    apt_module.add_feature(Feature::new("autoremove", "Auto remove", ParityStatus::Full, 0.5));
    scorecard.add_module(apt_module);

    // YUM module (high usage on RHEL/CentOS)
    let mut yum_module = ModuleParity::new("yum", "YUM package management", 0.70);
    yum_module.add_feature(Feature::new("name", "Package name(s)", ParityStatus::Full, 1.0));
    yum_module.add_feature(Feature::new("state", "Package state", ParityStatus::Full, 1.0));
    yum_module.add_feature(Feature::new("enablerepo", "Enable repo", ParityStatus::Partial, 0.5));
    yum_module.add_feature(Feature::new("disablerepo", "Disable repo", ParityStatus::Partial, 0.4));
    yum_module.add_feature(Feature::new("update_cache", "Update cache", ParityStatus::Full, 0.6));
    scorecard.add_module(yum_module);

    // Systemd module (high usage)
    let mut systemd_module = ModuleParity::new("systemd", "Systemd unit management", 0.80);
    systemd_module.add_feature(Feature::new("name", "Unit name", ParityStatus::Full, 1.0));
    systemd_module.add_feature(Feature::new("state", "Unit state", ParityStatus::Full, 1.0));
    systemd_module.add_feature(Feature::new("enabled", "Enable unit", ParityStatus::Full, 0.9));
    systemd_module.add_feature(Feature::new("daemon_reload", "Reload daemon", ParityStatus::Full, 0.8));
    systemd_module.add_feature(Feature::new("masked", "Mask unit", ParityStatus::Partial, 0.3));
    systemd_module.add_feature(Feature::new("scope", "Scope", ParityStatus::Partial, 0.2));
    scorecard.add_module(systemd_module);

    // URI module (medium usage)
    let mut uri_module = ModuleParity::new("uri", "HTTP requests", 0.55);
    uri_module.add_feature(Feature::new("url", "Request URL", ParityStatus::Full, 1.0));
    uri_module.add_feature(Feature::new("method", "HTTP method", ParityStatus::Full, 0.9));
    uri_module.add_feature(Feature::new("body", "Request body", ParityStatus::Full, 0.7));
    uri_module.add_feature(Feature::new("headers", "HTTP headers", ParityStatus::Full, 0.8));
    uri_module.add_feature(Feature::new("status_code", "Expected status", ParityStatus::Full, 0.7));
    uri_module.add_feature(Feature::new("return_content", "Return content", ParityStatus::Full, 0.6));
    uri_module.add_feature(Feature::new("validate_certs", "Cert validation", ParityStatus::Partial, 0.4));
    scorecard.add_module(uri_module);

    // Get_url module (medium usage)
    let mut get_url_module = ModuleParity::new("get_url", "Download files", 0.60);
    get_url_module.add_feature(Feature::new("url", "Download URL", ParityStatus::Full, 1.0));
    get_url_module.add_feature(Feature::new("dest", "Destination path", ParityStatus::Full, 1.0));
    get_url_module.add_feature(Feature::new("checksum", "Checksum validation", ParityStatus::Full, 0.7));
    get_url_module.add_feature(Feature::new("mode", "File mode", ParityStatus::Full, 0.6));
    get_url_module.add_feature(Feature::new("owner", "File owner", ParityStatus::Full, 0.5));
    get_url_module.add_feature(Feature::new("force", "Force download", ParityStatus::Full, 0.5));
    get_url_module.add_feature(Feature::new("validate_certs", "Cert validation", ParityStatus::Partial, 0.4));
    scorecard.add_module(get_url_module);

    // Set_fact module (high usage)
    let mut set_fact_module = ModuleParity::new("set_fact", "Set host facts", 0.80);
    set_fact_module.add_feature(Feature::new("key_value", "Set facts", ParityStatus::Full, 1.0));
    set_fact_module.add_feature(Feature::new("cacheable", "Cache facts", ParityStatus::Full, 0.5));
    scorecard.add_module(set_fact_module);

    // Wait_for module (medium usage)
    let mut wait_for_module = ModuleParity::new("wait_for", "Wait for conditions", 0.55);
    wait_for_module.add_feature(Feature::new("host", "Host to wait for", ParityStatus::Full, 0.8));
    wait_for_module.add_feature(Feature::new("port", "Port to wait for", ParityStatus::Full, 1.0));
    wait_for_module.add_feature(Feature::new("path", "Path to wait for", ParityStatus::Full, 0.8));
    wait_for_module.add_feature(Feature::new("state", "Wait state", ParityStatus::Full, 0.9));
    wait_for_module.add_feature(Feature::new("timeout", "Timeout", ParityStatus::Full, 0.8));
    wait_for_module.add_feature(Feature::new("delay", "Initial delay", ParityStatus::Full, 0.5));
    wait_for_module.add_feature(Feature::new("search_regex", "Search pattern", ParityStatus::Partial, 0.4));
    scorecard.add_module(wait_for_module);

    scorecard
}

/// Build baseline scorecard (previous known-good state)
fn build_baseline_scorecard() -> ParityScorecard {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("parity_scorecard_baseline.json");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("Missing baseline scorecard file at {}", path.display()));
    serde_json::from_str(&content)
        .unwrap_or_else(|err| panic!("Failed to parse baseline scorecard: {err}"))
}

#[test]
#[ignore]
fn write_baseline_snapshot() {
    let scorecard = build_current_scorecard();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("parity_scorecard_baseline.json");
    let serialized = serde_json::to_string_pretty(&scorecard)
        .expect("Should serialize baseline scorecard");
    fs::create_dir_all(path.parent().expect("baseline directory"))
        .expect("Should create baseline directory");
    fs::write(&path, serialized).expect("Should write baseline file");
}

// ============================================================================
// Tests: Scorecard Computation
// ============================================================================

#[test]
fn test_feature_scoring() {
    assert_eq!(ParityStatus::Full.score(), 1.0);
    assert_eq!(ParityStatus::Partial.score(), 0.5);
    assert_eq!(ParityStatus::Planned.score(), 0.0);
    assert_eq!(ParityStatus::NotApplicable.score(), 1.0);
    assert_eq!(ParityStatus::NotPlanned.score(), 0.0);
}

#[test]
fn test_feature_weighted_score() {
    let feature = Feature::new("test", "Test feature", ParityStatus::Full, 0.8);
    assert_eq!(feature.weighted_score(), 0.8);

    let partial = Feature::new("test", "Test feature", ParityStatus::Partial, 0.8);
    assert_eq!(partial.weighted_score(), 0.4);
}

#[test]
fn test_module_score_all_full() {
    let mut module = ModuleParity::new("test", "Test module", 1.0);
    module.add_feature(Feature::new("f1", "Feature 1", ParityStatus::Full, 1.0));
    module.add_feature(Feature::new("f2", "Feature 2", ParityStatus::Full, 1.0));

    assert!((module.score() - 1.0).abs() < 0.001);
}

#[test]
fn test_module_score_mixed() {
    let mut module = ModuleParity::new("test", "Test module", 1.0);
    module.add_feature(Feature::new("f1", "Feature 1", ParityStatus::Full, 1.0));
    module.add_feature(Feature::new("f2", "Feature 2", ParityStatus::Partial, 1.0));

    // (1.0 * 1.0 + 0.5 * 1.0) / 2.0 = 0.75
    assert!((module.score() - 0.75).abs() < 0.001);
}

#[test]
fn test_module_score_weighted_features() {
    let mut module = ModuleParity::new("test", "Test module", 1.0);
    module.add_feature(Feature::new("f1", "Feature 1", ParityStatus::Full, 0.8));
    module.add_feature(Feature::new("f2", "Feature 2", ParityStatus::Partial, 0.2));

    // (1.0 * 0.8 + 0.5 * 0.2) / (0.8 + 0.2) = 0.9
    assert!((module.score() - 0.9).abs() < 0.001);
}

#[test]
fn test_module_score_excludes_not_applicable() {
    let mut module = ModuleParity::new("test", "Test module", 1.0);
    module.add_feature(Feature::new("f1", "Feature 1", ParityStatus::Full, 1.0));
    module.add_feature(Feature::new("f2", "Feature 2", ParityStatus::NotApplicable, 1.0));

    // NotApplicable should not affect score
    assert!((module.score() - 1.0).abs() < 0.001);
}

#[test]
fn test_overall_score_computation() {
    let mut scorecard = ParityScorecard::new(0.70);

    let mut m1 = ModuleParity::new("m1", "Module 1", 0.5);
    m1.add_feature(Feature::new("f1", "F1", ParityStatus::Full, 1.0));

    let mut m2 = ModuleParity::new("m2", "Module 2", 0.5);
    m2.add_feature(Feature::new("f1", "F1", ParityStatus::Partial, 1.0));

    scorecard.add_module(m1);
    scorecard.add_module(m2);

    // (1.0 * 0.5 + 0.5 * 0.5) / (0.5 + 0.5) = 0.75
    assert!((scorecard.overall_score() - 0.75).abs() < 0.001);
}

#[test]
fn test_overall_score_weighted_modules() {
    let mut scorecard = ParityScorecard::new(0.70);

    let mut m1 = ModuleParity::new("m1", "Module 1", 0.8);
    m1.add_feature(Feature::new("f1", "F1", ParityStatus::Full, 1.0));

    let mut m2 = ModuleParity::new("m2", "Module 2", 0.2);
    m2.add_feature(Feature::new("f1", "F1", ParityStatus::Partial, 1.0));

    scorecard.add_module(m1);
    scorecard.add_module(m2);

    // (1.0 * 0.8 + 0.5 * 0.2) / (0.8 + 0.2) = 0.9
    assert!((scorecard.overall_score() - 0.9).abs() < 0.001);
}

// ============================================================================
// Tests: CI Gate
// ============================================================================

#[test]
fn test_ci_passes_above_threshold() {
    let mut scorecard = ParityScorecard::new(0.70);

    let mut m1 = ModuleParity::new("m1", "Module 1", 1.0);
    m1.add_feature(Feature::new("f1", "F1", ParityStatus::Full, 1.0));

    scorecard.add_module(m1);

    assert!(scorecard.passes_ci());
}

#[test]
fn test_ci_fails_below_threshold() {
    let mut scorecard = ParityScorecard::new(0.70);

    let mut m1 = ModuleParity::new("m1", "Module 1", 1.0);
    m1.add_feature(Feature::new("f1", "F1", ParityStatus::Partial, 1.0));

    scorecard.add_module(m1);

    // Score is 0.5, threshold is 0.7
    assert!(!scorecard.passes_ci());
}

#[test]
fn test_current_scorecard_passes_ci() {
    let scorecard = build_current_scorecard();
    assert!(
        scorecard.passes_ci(),
        "Current scorecard should pass CI gate. Score: {:.1}%, Minimum: {:.1}%",
        scorecard.overall_score() * 100.0,
        scorecard.minimum_score * 100.0
    );
}

// ============================================================================
// Tests: Regression Detection
// ============================================================================

#[test]
fn test_no_regressions_when_equal() {
    let scorecard = build_current_scorecard();
    let baseline = build_baseline_scorecard();

    let regressions = scorecard.has_regressions(&baseline);
    assert!(
        regressions.is_empty(),
        "Should have no regressions. Found: {:?}",
        regressions
    );
}

#[test]
fn test_detects_overall_regression() {
    let mut baseline = ParityScorecard::new(0.70);
    let mut m1 = ModuleParity::new("m1", "Module 1", 1.0);
    m1.add_feature(Feature::new("f1", "F1", ParityStatus::Full, 1.0));
    baseline.add_module(m1);

    let mut current = ParityScorecard::new(0.70);
    let mut m1 = ModuleParity::new("m1", "Module 1", 1.0);
    m1.add_feature(Feature::new("f1", "F1", ParityStatus::Partial, 1.0));
    current.add_module(m1);

    let regressions = current.has_regressions(&baseline);
    assert!(!regressions.is_empty());
    assert!(regressions.iter().any(|r| r.contains("Overall score regressed")));
}

#[test]
fn test_detects_module_regression() {
    let mut baseline = ParityScorecard::new(0.70);
    let mut m1 = ModuleParity::new("file", "File module", 1.0);
    m1.add_feature(Feature::new("path", "Path", ParityStatus::Full, 1.0));
    baseline.add_module(m1);

    let mut current = ParityScorecard::new(0.70);
    let mut m1 = ModuleParity::new("file", "File module", 1.0);
    m1.add_feature(Feature::new("path", "Path", ParityStatus::Partial, 1.0));
    current.add_module(m1);

    let regressions = current.has_regressions(&baseline);
    assert!(!regressions.is_empty());
    assert!(regressions.iter().any(|r| r.contains("file") && r.contains("regressed")));
}

#[test]
fn test_current_scorecard_no_regressions() {
    let current = build_current_scorecard();
    let baseline = build_baseline_scorecard();

    let regressions = current.has_regressions(&baseline);
    assert!(
        regressions.is_empty(),
        "Current scorecard has regressions against baseline: {:?}",
        regressions
    );
}

// ============================================================================
// Tests: Scorecard Output Formats
// ============================================================================

#[test]
fn test_markdown_output() {
    let scorecard = build_current_scorecard();
    let md = scorecard.to_markdown();

    assert!(md.contains("# Ansible Parity Scorecard"));
    assert!(md.contains("Overall Score"));
    assert!(md.contains("Module Scores"));
    assert!(md.contains("| file |"));
    assert!(md.contains("| copy |"));
}

#[test]
fn test_json_output() {
    let scorecard = build_current_scorecard();
    let json = scorecard.to_json();

    assert!(json.contains("\"overall_score\""));
    assert!(json.contains("\"minimum_score\""));
    assert!(json.contains("\"passes_ci\""));
    assert!(json.contains("\"modules\""));
    assert!(json.contains("\"name\": \"file\""));
}

#[test]
fn test_json_is_valid() {
    let scorecard = build_current_scorecard();
    let json = scorecard.to_json();

    // Basic JSON structure validation
    assert!(json.starts_with('{'));
    assert!(json.ends_with('}'));
    assert!(json.contains("\"modules\": ["));
}

// ============================================================================
// Tests: Current Scorecard Coverage
// ============================================================================

#[test]
fn test_scorecard_has_core_modules() {
    let scorecard = build_current_scorecard();

    let expected_modules = [
        "file", "copy", "template", "package", "service",
        "command", "shell", "user", "group", "lineinfile",
        "debug", "stat", "git", "apt", "yum", "systemd",
        "uri", "get_url", "set_fact", "wait_for"
    ];

    let module_names: Vec<&str> = scorecard.modules.iter().map(|m| m.name.as_str()).collect();

    for expected in &expected_modules {
        assert!(
            module_names.contains(expected),
            "Scorecard missing core module: {}",
            expected
        );
    }
}

#[test]
fn test_scorecard_modules_have_features() {
    let scorecard = build_current_scorecard();

    for module in &scorecard.modules {
        assert!(
            !module.features.is_empty(),
            "Module '{}' has no features defined",
            module.name
        );
    }
}

#[test]
fn test_scorecard_has_minimum_overall_score() {
    let scorecard = build_current_scorecard();

    // Ensure overall score is reasonable
    assert!(
        scorecard.overall_score() > 0.70,
        "Overall score should be above 70%, got {:.1}%",
        scorecard.overall_score() * 100.0
    );
}

// ============================================================================
// CI Regression Guards
// ============================================================================

#[test]
fn test_ci_guard_minimum_score_threshold() {
    let scorecard = build_current_scorecard();

    // Guard: minimum score threshold must be at least 70%
    assert!(
        scorecard.minimum_score >= 0.70,
        "CI gate minimum score should be at least 70%"
    );
}

#[test]
fn test_ci_guard_module_count() {
    let scorecard = build_current_scorecard();

    // Guard: must track at least 20 modules
    assert!(
        scorecard.modules.len() >= 20,
        "Should track at least 20 modules, got {}",
        scorecard.modules.len()
    );
}

#[test]
fn test_ci_guard_all_modules_weighted() {
    let scorecard = build_current_scorecard();

    for module in &scorecard.modules {
        assert!(
            module.usage_weight > 0.0,
            "Module '{}' has zero weight",
            module.name
        );
        assert!(
            module.usage_weight <= 1.0,
            "Module '{}' has weight > 1.0",
            module.name
        );
    }
}

#[test]
fn test_ci_guard_high_usage_modules_complete() {
    let scorecard = build_current_scorecard();

    // High-usage modules (weight >= 0.80) should have high parity (>= 80%)
    for module in &scorecard.modules {
        if module.usage_weight >= 0.80 {
            assert!(
                module.score() >= 0.80,
                "High-usage module '{}' (weight {:.0}%) has low parity score {:.1}%",
                module.name,
                module.usage_weight * 100.0,
                module.score() * 100.0
            );
        }
    }
}

#[test]
fn test_ci_guard_no_empty_descriptions() {
    let scorecard = build_current_scorecard();

    for module in &scorecard.modules {
        assert!(
            !module.description.is_empty(),
            "Module '{}' has empty description",
            module.name
        );

        for feature in &module.features {
            assert!(
                !feature.description.is_empty(),
                "Feature '{}' in module '{}' has empty description",
                feature.name,
                module.name
            );
        }
    }
}

#[test]
fn test_ci_guard_feature_weights_valid() {
    let scorecard = build_current_scorecard();

    for module in &scorecard.modules {
        for feature in &module.features {
            assert!(
                feature.weight >= 0.0 && feature.weight <= 1.0,
                "Feature '{}' in module '{}' has invalid weight {}",
                feature.name,
                module.name,
                feature.weight
            );
        }
    }
}

#[test]
fn test_ci_guard_regression_detection_works() {
    // Ensure regression detection actually works
    let mut baseline = ParityScorecard::new(0.70);
    let mut m = ModuleParity::new("test", "Test", 1.0);
    m.add_feature(Feature::new("f", "F", ParityStatus::Full, 1.0));
    baseline.add_module(m);

    let mut regressed = ParityScorecard::new(0.70);
    let mut m = ModuleParity::new("test", "Test", 1.0);
    m.add_feature(Feature::new("f", "F", ParityStatus::Planned, 1.0)); // Regression!
    regressed.add_module(m);

    let regressions = regressed.has_regressions(&baseline);
    assert!(
        !regressions.is_empty(),
        "Regression detection must find actual regressions"
    );
}
