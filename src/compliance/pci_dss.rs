//! PCI-DSS (Payment Card Industry Data Security Standard) Scanner
//!
//! Implements security checks based on PCI-DSS requirements for systems
//! that handle payment card data.

use super::checks::*;
use super::{
    CheckInfo, ComplianceContext, ComplianceError, ComplianceFramework, ComplianceResult,
    ComplianceScanner, Finding, Severity,
};
use async_trait::async_trait;

/// PCI-DSS Scanner implementation
pub struct PciDssScanner {
    /// Version of PCI-DSS implemented
    version: String,
    /// Cached check definitions
    checks: Vec<Box<dyn ComplianceCheck>>,
}

impl PciDssScanner {
    /// Create a new PCI-DSS scanner
    pub fn new() -> Self {
        Self {
            version: "4.0".to_string(),
            checks: Self::build_checks(),
        }
    }

    /// Build all PCI-DSS checks
    fn build_checks() -> Vec<Box<dyn ComplianceCheck>> {
        let mut checks: Vec<Box<dyn ComplianceCheck>> = Vec::new();

        // Requirement 2 - Secure configurations
        checks.extend(Self::requirement_2_checks());

        // Requirement 5 - Antivirus
        checks.extend(Self::requirement_5_checks());

        // Requirement 7 - Access control
        checks.extend(Self::requirement_7_checks());

        // Requirement 8 - Authentication
        checks.extend(Self::requirement_8_checks());

        // Requirement 10 - Logging
        checks.extend(Self::requirement_10_checks());

        checks
    }

    fn requirement_2_checks() -> Vec<Box<dyn ComplianceCheck>> {
        vec![
            // PCI-DSS 2.2.1 - Implement only one primary function per server
            Box::new(
                CommandCheck::new(
                    "PCI-2.2.1",
                    "Verify server has single primary function",
                    "systemctl list-units --type=service --state=running | wc -l",
                )
                .with_description(
                    "Servers should implement only one primary function to minimize attack surface.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Services)
                .with_expected_pattern(r"^\d+$")
                .with_remediation(
                    "Review running services and disable unnecessary ones. \
                     Consider separating functions to different servers.",
                )
                .with_tag("pci-req-2".to_string())
                .with_tag("services".to_string()),
            ),
            // PCI-DSS 2.2.2 - Enable only necessary services
            Box::new(
                ServiceCheck::new(
                    "PCI-2.2.2-telnet",
                    "Ensure telnet server is not enabled",
                    "telnet.socket",
                )
                .with_description(
                    "Telnet transmits data in clear text and must not be used.",
                )
                .with_severity(Severity::Critical)
                .should_be_enabled(false)
                .with_remediation("Disable telnet: systemctl stop telnet.socket && systemctl disable telnet.socket")
                .with_tag("pci-req-2".to_string())
                .with_tag("services".to_string()),
            ),
            // PCI-DSS 2.2.3 - Implement security features
            Box::new(
                SysctlCheck::new(
                    "PCI-2.2.3-aslr",
                    "Ensure ASLR is enabled",
                    "kernel.randomize_va_space",
                    "2",
                )
                .with_description(
                    "Address Space Layout Randomization must be enabled.",
                )
                .with_severity(Severity::High)
                .with_remediation(
                    "Set 'kernel.randomize_va_space = 2' in /etc/sysctl.conf",
                )
                .with_tag("pci-req-2".to_string())
                .with_tag("kernel".to_string()),
            ),
            // PCI-DSS 2.2.4 - Configure security parameters
            Box::new(
                SysctlCheck::new(
                    "PCI-2.2.4-syncookies",
                    "Ensure TCP SYN cookies are enabled",
                    "net.ipv4.tcp_syncookies",
                    "1",
                )
                .with_description(
                    "SYN cookies protect against SYN flood denial of service attacks.",
                )
                .with_severity(Severity::High)
                .with_remediation(
                    "Set 'net.ipv4.tcp_syncookies = 1' in /etc/sysctl.conf",
                )
                .with_tag("pci-req-2".to_string())
                .with_tag("network".to_string()),
            ),
        ]
    }

    fn requirement_5_checks() -> Vec<Box<dyn ComplianceCheck>> {
        vec![
            // PCI-DSS 5.2.1 - Anti-malware solutions
            Box::new(
                CommandCheck::new(
                    "PCI-5.2.1",
                    "Verify anti-malware solution is installed",
                    "command -v clamscan || command -v freshclam || systemctl is-active clamav-daemon 2>/dev/null",
                )
                .with_description(
                    "Anti-malware software must be installed on systems commonly affected by malware.",
                )
                .with_severity(Severity::High)
                .with_category(CheckCategory::Services)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Install ClamAV or another anti-malware solution: apt install clamav clamav-daemon",
                )
                .with_tag("pci-req-5".to_string())
                .with_tag("antivirus".to_string()),
            ),
        ]
    }

    fn requirement_7_checks() -> Vec<Box<dyn ComplianceCheck>> {
        vec![
            // PCI-DSS 7.1.1 - Access based on need-to-know
            Box::new(
                CommandCheck::new(
                    "PCI-7.1.1",
                    "Verify root/admin account usage is limited",
                    "grep -E '^[^:]+:x:0:' /etc/passwd | wc -l",
                )
                .with_description("Only necessary accounts should have UID 0 (root privileges).")
                .with_severity(Severity::High)
                .with_category(CheckCategory::AccessControl)
                .with_expected_pattern(r"^1$")
                .with_remediation(
                    "Review accounts with UID 0 and remove unnecessary ones. \
                     Only root should have UID 0.",
                )
                .with_tag("pci-req-7".to_string())
                .with_tag("access-control".to_string()),
            ),
            // PCI-DSS 7.2.1 - Access control systems
            Box::new(
                CommandCheck::new(
                    "PCI-7.2.1",
                    "Verify sudo is installed for privilege escalation",
                    "command -v sudo",
                )
                .with_description("Sudo must be used for controlled privilege escalation.")
                .with_severity(Severity::High)
                .with_category(CheckCategory::AccessControl)
                .with_expected_exit_code(0)
                .with_remediation("Install sudo: apt install sudo or yum install sudo")
                .with_tag("pci-req-7".to_string())
                .with_tag("access-control".to_string()),
            ),
            // PCI-DSS 7.2.2 - Restrict access based on job classification
            Box::new(
                FileCheck::new(
                    "PCI-7.2.2",
                    "Verify sudoers file has proper permissions",
                    "/etc/sudoers",
                )
                .with_description(
                    "The sudoers file must be protected from unauthorized modification.",
                )
                .with_severity(Severity::High)
                .with_category(CheckCategory::AccessControl)
                .with_owner("root")
                .with_group("root")
                .with_mode("440")
                .with_remediation("chown root:root /etc/sudoers && chmod 440 /etc/sudoers")
                .with_tag("pci-req-7".to_string())
                .with_tag("permissions".to_string()),
            ),
        ]
    }

    fn requirement_8_checks() -> Vec<Box<dyn ComplianceCheck>> {
        vec![
            // PCI-DSS 8.2.1 - Unique user IDs
            Box::new(
                CommandCheck::new(
                    "PCI-8.2.1",
                    "Verify no duplicate UIDs exist",
                    "cat /etc/passwd | cut -d: -f3 | sort | uniq -d | wc -l",
                )
                .with_description(
                    "Each user must have a unique ID for accountability.",
                )
                .with_severity(Severity::High)
                .with_category(CheckCategory::UserAccounts)
                .with_expected_pattern(r"^0$")
                .with_remediation(
                    "Review /etc/passwd for duplicate UIDs and assign unique IDs to all users.",
                )
                .with_tag("pci-req-8".to_string())
                .with_tag("user-accounts".to_string()),
            ),
            // PCI-DSS 8.2.3 - Strong authentication
            Box::new(
                CommandCheck::new(
                    "PCI-8.2.3-minlen",
                    "Verify password minimum length is at least 12",
                    "grep -E '^\\s*minlen\\s*=' /etc/security/pwquality.conf | grep -oP '\\d+' | awk '{if($1>=12) exit 0; else exit 1}'",
                )
                .with_description(
                    "Passwords must be at least 12 characters (PCI-DSS 4.0).",
                )
                .with_severity(Severity::High)
                .with_category(CheckCategory::Authentication)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'minlen = 12' in /etc/security/pwquality.conf",
                )
                .with_tag("pci-req-8".to_string())
                .with_tag("password".to_string()),
            ),
            // PCI-DSS 8.2.4 - Password change requirements
            Box::new(
                CommandCheck::new(
                    "PCI-8.2.4",
                    "Verify password expiration is configured",
                    "grep -E '^PASS_MAX_DAYS' /etc/login.defs | awk '{if($2<=90 && $2>0) exit 0; else exit 1}'",
                )
                .with_description(
                    "Passwords must be changed at least every 90 days.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Authentication)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'PASS_MAX_DAYS 90' in /etc/login.defs",
                )
                .with_tag("pci-req-8".to_string())
                .with_tag("password".to_string()),
            ),
            // PCI-DSS 8.2.5 - Password history
            Box::new(
                CommandCheck::new(
                    "PCI-8.2.5",
                    "Verify password history prevents reuse",
                    "grep -E 'remember\\s*=\\s*[0-9]+' /etc/pam.d/common-password /etc/pam.d/system-auth 2>/dev/null | grep -oP '\\d+' | awk '{if($1>=4) exit 0; else exit 1}'",
                )
                .with_description(
                    "Users must not reuse last 4 passwords.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Authentication)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Add 'remember=4' to pam_unix or pam_pwhistory in PAM configuration",
                )
                .with_tag("pci-req-8".to_string())
                .with_tag("password".to_string()),
            ),
            // PCI-DSS 8.3.1 - Multi-factor authentication for admin access
            Box::new(
                CommandCheck::new(
                    "PCI-8.3.1",
                    "Verify MFA capability is available for SSH",
                    "grep -E '^\\s*(ChallengeResponseAuthentication|AuthenticationMethods)' /etc/ssh/sshd_config",
                )
                .with_description(
                    "Multi-factor authentication should be configured for remote administrative access.",
                )
                .with_severity(Severity::High)
                .with_category(CheckCategory::Authentication)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Configure SSH with AuthenticationMethods publickey,keyboard-interactive \
                     and set up Google Authenticator or similar",
                )
                .with_tag("pci-req-8".to_string())
                .with_tag("mfa".to_string()),
            ),
            // PCI-DSS 8.3.4 - Account lockout
            Box::new(
                CommandCheck::new(
                    "PCI-8.3.4",
                    "Verify account lockout is configured",
                    "grep -E 'pam_tally2|pam_faillock' /etc/pam.d/common-auth /etc/pam.d/system-auth 2>/dev/null",
                )
                .with_description(
                    "Accounts must be locked after not more than 10 failed login attempts.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Authentication)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Configure pam_faillock with deny=10 unlock_time=1800 in PAM configuration",
                )
                .with_tag("pci-req-8".to_string())
                .with_tag("account-lockout".to_string()),
            ),
        ]
    }

    fn requirement_10_checks() -> Vec<Box<dyn ComplianceCheck>> {
        vec![
            // PCI-DSS 10.2.1 - Audit logging
            Box::new(
                ServiceCheck::new("PCI-10.2.1", "Verify audit logging is enabled", "auditd")
                    .with_description("All access to cardholder data must be logged.")
                    .with_severity(Severity::High)
                    .should_be_enabled(true)
                    .should_be_running(true)
                    .with_remediation("systemctl enable auditd && systemctl start auditd")
                    .with_tag("pci-req-10".to_string())
                    .with_tag("auditing".to_string()),
            ),
            // PCI-DSS 10.2.2 - Log all actions by root/admin
            Box::new(
                CommandCheck::new(
                    "PCI-10.2.2",
                    "Verify root actions are audited",
                    "auditctl -l 2>/dev/null | grep -E '(uid=0|euid=0)'",
                )
                .with_description("All actions taken by administrators must be logged.")
                .with_severity(Severity::High)
                .with_category(CheckCategory::Auditing)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Add audit rules for root actions: \
                     -a always,exit -F arch=b64 -F euid=0 -S all -k root_actions",
                )
                .with_tag("pci-req-10".to_string())
                .with_tag("auditing".to_string()),
            ),
            // PCI-DSS 10.2.5 - Log authentication events
            Box::new(
                CommandCheck::new(
                    "PCI-10.2.5",
                    "Verify authentication events are logged",
                    "auditctl -l 2>/dev/null | grep -E '(faillog|lastlog|tallylog|pam)'",
                )
                .with_description("All authentication events must be logged.")
                .with_severity(Severity::High)
                .with_category(CheckCategory::Auditing)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Add audit rules for authentication: \
                     -w /var/log/faillog -p wa -k logins \
                     -w /var/log/lastlog -p wa -k logins",
                )
                .with_tag("pci-req-10".to_string())
                .with_tag("auditing".to_string()),
            ),
            // PCI-DSS 10.3 - Time synchronization
            Box::new(
                ServiceCheck::new(
                    "PCI-10.3",
                    "Verify time synchronization is configured",
                    "chronyd",
                )
                .with_description(
                    "Time synchronization must be configured for accurate log timestamps.",
                )
                .with_severity(Severity::High)
                .should_be_enabled(true)
                .should_be_running(true)
                .with_remediation(
                    "Install and enable chrony or ntpd: \
                     systemctl enable chronyd && systemctl start chronyd",
                )
                .with_tag("pci-req-10".to_string())
                .with_tag("time-sync".to_string()),
            ),
            // PCI-DSS 10.5.1 - Limit log access
            Box::new(
                FileCheck::new(
                    "PCI-10.5.1",
                    "Verify audit log permissions are restrictive",
                    "/var/log/audit",
                )
                .with_description("Audit logs must be protected from unauthorized access.")
                .with_severity(Severity::High)
                .with_category(CheckCategory::Auditing)
                .with_owner("root")
                .with_mode("750")
                .with_remediation("chmod 750 /var/log/audit && chown root:root /var/log/audit")
                .with_tag("pci-req-10".to_string())
                .with_tag("permissions".to_string()),
            ),
        ]
    }
}

impl Default for PciDssScanner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ComplianceScanner for PciDssScanner {
    fn framework(&self) -> ComplianceFramework {
        ComplianceFramework::PciDss
    }

    fn name(&self) -> &str {
        "PCI-DSS Scanner"
    }

    fn description(&self) -> &str {
        "Scans for compliance with Payment Card Industry Data Security Standard (PCI-DSS)"
    }

    fn version(&self) -> &str {
        &self.version
    }

    async fn scan(&self, context: &ComplianceContext) -> ComplianceResult<Vec<Finding>> {
        let mut findings = Vec::new();

        for check in &self.checks {
            let check_tags = check.tags();
            if !context.should_include_tag(&check_tags) {
                continue;
            }

            if check.severity() < context.severity_threshold {
                continue;
            }

            let result = check.execute(context).await?;

            let mut finding = Finding::new(check.id(), check.title(), ComplianceFramework::PciDss)
                .with_description(check.description())
                .with_severity(check.severity())
                .with_status(result.status)
                .with_remediation(check.remediation());

            if let Some(observed) = result.observed {
                finding = finding.with_observed(observed);
            }

            for tag in check_tags {
                finding = finding.with_tag(tag);
            }

            findings.push(finding);
        }

        Ok(findings)
    }

    async fn run_check(
        &self,
        check_id: &str,
        context: &ComplianceContext,
    ) -> ComplianceResult<Finding> {
        let check = self
            .checks
            .iter()
            .find(|c| c.id() == check_id)
            .ok_or_else(|| {
                ComplianceError::InvalidConfig(format!("Check {} not found", check_id))
            })?;

        let result = check.execute(context).await?;

        let mut finding = Finding::new(check.id(), check.title(), ComplianceFramework::PciDss)
            .with_description(check.description())
            .with_severity(check.severity())
            .with_status(result.status)
            .with_remediation(check.remediation());

        if let Some(observed) = result.observed {
            finding = finding.with_observed(observed);
        }

        Ok(finding)
    }

    fn list_checks(&self) -> Vec<&str> {
        self.checks.iter().map(|c| c.id()).collect()
    }

    fn get_check_info(&self, check_id: &str) -> Option<CheckInfo> {
        self.checks
            .iter()
            .find(|c| c.id() == check_id)
            .map(|c| CheckInfo {
                id: c.id().to_string(),
                title: c.title().to_string(),
                description: c.description().to_string(),
                severity: c.severity(),
                tags: c.tags(),
                auto_remediable: false,
                remediation_time_minutes: Some(10),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pci_dss_scanner_creation() {
        let scanner = PciDssScanner::new();
        assert_eq!(scanner.framework(), ComplianceFramework::PciDss);
        assert!(!scanner.list_checks().is_empty());
    }

    #[test]
    fn test_list_pci_checks() {
        let scanner = PciDssScanner::new();
        let checks = scanner.list_checks();
        assert!(checks.iter().any(|c| c.starts_with("PCI-")));
    }

    #[test]
    fn test_pci_version() {
        let scanner = PciDssScanner::new();
        assert_eq!(scanner.version(), "4.0");
    }
}
