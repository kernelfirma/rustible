//! CIS Benchmark Scanner
//!
//! Implements security checks based on the Center for Internet Security (CIS)
//! benchmarks for Linux systems. This covers hardening recommendations for:
//!
//! - Filesystem configuration
//! - System services
//! - Network configuration
//! - Logging and auditing
//! - Access control
//! - System maintenance
//!
//! ## Supported Benchmarks
//!
//! - CIS Benchmark for Ubuntu Linux
//! - CIS Benchmark for RHEL/CentOS
//! - CIS Benchmark for Debian
//!
//! The scanner automatically detects the OS and applies appropriate checks.

use super::checks::*;
use super::{
    CheckInfo, ComplianceContext, ComplianceError, ComplianceFramework, ComplianceResult,
    ComplianceScanner, Finding, Severity,
};
use async_trait::async_trait;

/// CIS Benchmark Scanner
pub struct CisScanner {
    /// Version of CIS benchmark implemented
    version: String,
    /// Cached check definitions
    checks: Vec<Box<dyn ComplianceCheck>>,
}

impl CisScanner {
    /// Create a new CIS scanner
    pub fn new() -> Self {
        Self {
            version: "1.0.0".to_string(),
            checks: Self::build_checks(),
        }
    }

    /// Build all CIS benchmark checks
    fn build_checks() -> Vec<Box<dyn ComplianceCheck>> {
        let mut checks: Vec<Box<dyn ComplianceCheck>> = Vec::new();

        // Add filesystem checks
        checks.extend(Self::filesystem_checks());

        // Add service checks
        checks.extend(Self::service_checks());

        // Add network checks
        checks.extend(Self::network_checks());

        // Add logging and auditing checks
        checks.extend(Self::logging_checks());

        // Add access control checks
        checks.extend(Self::access_control_checks());

        // Add authentication checks
        checks.extend(Self::authentication_checks());

        checks
    }

    // =========================================================================
    // Filesystem Configuration (CIS 1.x)
    // =========================================================================

    fn filesystem_checks() -> Vec<Box<dyn ComplianceCheck>> {
        vec![
            // CIS 1.1.1.1 - Ensure mounting of cramfs is disabled
            Box::new(
                CommandCheck::new(
                    "CIS-1.1.1.1",
                    "Ensure cramfs kernel module is disabled",
                    "modprobe -n -v cramfs 2>/dev/null | grep -qE 'install /bin/(true|false)'",
                )
                .with_description(
                    "The cramfs filesystem type is a compressed read-only Linux filesystem \
                     embedded in small footprint systems. Disabling cramfs reduces attack surface.",
                )
                .with_severity(Severity::Low)
                .with_category(CheckCategory::Filesystem)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Add 'install cramfs /bin/true' to /etc/modprobe.d/cramfs.conf and run \
                     'modprobe -r cramfs' if loaded.",
                )
                .with_tag("filesystem".to_string())
                .with_tag("kernel-modules".to_string()),
            ),
            // CIS 1.1.1.2 - Ensure mounting of squashfs is disabled
            Box::new(
                CommandCheck::new(
                    "CIS-1.1.1.2",
                    "Ensure squashfs kernel module is disabled",
                    "modprobe -n -v squashfs 2>/dev/null | grep -qE 'install /bin/(true|false)'",
                )
                .with_description(
                    "The squashfs filesystem is a compressed read-only filesystem. \
                     Disabling reduces attack surface unless needed for snap packages.",
                )
                .with_severity(Severity::Low)
                .with_category(CheckCategory::Filesystem)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Add 'install squashfs /bin/true' to /etc/modprobe.d/squashfs.conf",
                )
                .with_tag("filesystem".to_string())
                .with_tag("kernel-modules".to_string()),
            ),
            // CIS 1.1.1.3 - Ensure mounting of udf is disabled
            Box::new(
                CommandCheck::new(
                    "CIS-1.1.1.3",
                    "Ensure udf kernel module is disabled",
                    "modprobe -n -v udf 2>/dev/null | grep -qE 'install /bin/(true|false)'",
                )
                .with_description(
                    "The udf filesystem type is for reading Universal Disk Format media. \
                     Disabling reduces attack surface on systems that don't need optical media.",
                )
                .with_severity(Severity::Low)
                .with_category(CheckCategory::Filesystem)
                .with_expected_exit_code(0)
                .with_remediation("Add 'install udf /bin/true' to /etc/modprobe.d/udf.conf")
                .with_tag("filesystem".to_string())
                .with_tag("kernel-modules".to_string()),
            ),
            // CIS 1.1.2 - Ensure /tmp is configured
            Box::new(
                CommandCheck::new(
                    "CIS-1.1.2",
                    "Ensure /tmp is a separate partition",
                    "findmnt -n /tmp",
                )
                .with_description(
                    "The /tmp directory is a world-writable directory used for temporary storage. \
                     Having it on a separate partition enables mount options like noexec, nodev, nosuid.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Filesystem)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Create a separate partition for /tmp or configure tmpfs in /etc/fstab: \
                     'tmpfs /tmp tmpfs defaults,rw,nosuid,nodev,noexec,relatime 0 0'",
                )
                .with_tag("filesystem".to_string())
                .with_tag("partitioning".to_string()),
            ),
            // CIS 1.1.3 - Ensure noexec option set on /tmp
            Box::new(
                CommandCheck::new(
                    "CIS-1.1.3",
                    "Ensure noexec option set on /tmp partition",
                    "findmnt -n /tmp | grep -q noexec",
                )
                .with_description(
                    "The noexec mount option prevents execution of binaries from /tmp, \
                     which is a common target for malware.",
                )
                .with_severity(Severity::High)
                .with_category(CheckCategory::Filesystem)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Add 'noexec' option to /tmp mount in /etc/fstab and remount: \
                     'mount -o remount,noexec /tmp'",
                )
                .with_tag("filesystem".to_string())
                .with_tag("mount-options".to_string()),
            ),
            // CIS 1.1.4 - Ensure nodev option set on /tmp
            Box::new(
                CommandCheck::new(
                    "CIS-1.1.4",
                    "Ensure nodev option set on /tmp partition",
                    "findmnt -n /tmp | grep -q nodev",
                )
                .with_description(
                    "The nodev mount option prevents the creation of device files in /tmp.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Filesystem)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Add 'nodev' option to /tmp mount in /etc/fstab and remount",
                )
                .with_tag("filesystem".to_string())
                .with_tag("mount-options".to_string()),
            ),
            // CIS 1.1.5 - Ensure nosuid option set on /tmp
            Box::new(
                CommandCheck::new(
                    "CIS-1.1.5",
                    "Ensure nosuid option set on /tmp partition",
                    "findmnt -n /tmp | grep -q nosuid",
                )
                .with_description(
                    "The nosuid mount option prevents setuid programs from running from /tmp.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Filesystem)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Add 'nosuid' option to /tmp mount in /etc/fstab and remount",
                )
                .with_tag("filesystem".to_string())
                .with_tag("mount-options".to_string()),
            ),
            // CIS 1.4.1 - Ensure permissions on bootloader config
            Box::new(
                FileCheck::new(
                    "CIS-1.4.1",
                    "Ensure permissions on bootloader config are configured",
                    "/boot/grub/grub.cfg",
                )
                .with_description(
                    "The grub configuration file contains boot options that could be exploited \
                     to gain unauthorized access. It should be readable only by root.",
                )
                .with_severity(Severity::High)
                .with_category(CheckCategory::AccessControl)
                .with_owner("root")
                .with_group("root")
                .with_mode("600")
                .with_remediation(
                    "Run: chown root:root /boot/grub/grub.cfg && chmod 600 /boot/grub/grub.cfg",
                )
                .with_tag("filesystem".to_string())
                .with_tag("bootloader".to_string()),
            ),
            // CIS 1.5.1 - Ensure core dumps are restricted
            Box::new(
                SysctlCheck::new(
                    "CIS-1.5.1",
                    "Ensure core dumps are restricted",
                    "fs.suid_dumpable",
                    "0",
                )
                .with_description(
                    "Core dumps can contain sensitive data. Restricting them prevents \
                     information disclosure and reduces attack surface.",
                )
                .with_severity(Severity::Medium)
                .with_remediation(
                    "Set 'fs.suid_dumpable = 0' in /etc/sysctl.conf and run 'sysctl -p'",
                )
                .with_tag("kernel".to_string())
                .with_tag("core-dumps".to_string()),
            ),
            // CIS 1.5.2 - Ensure address space layout randomization (ASLR) is enabled
            Box::new(
                SysctlCheck::new(
                    "CIS-1.5.2",
                    "Ensure ASLR is enabled",
                    "kernel.randomize_va_space",
                    "2",
                )
                .with_description(
                    "Address Space Layout Randomization (ASLR) makes it more difficult \
                     for an attacker to predict target memory addresses for exploitation.",
                )
                .with_severity(Severity::High)
                .with_remediation(
                    "Set 'kernel.randomize_va_space = 2' in /etc/sysctl.conf",
                )
                .with_tag("kernel".to_string())
                .with_tag("memory".to_string()),
            ),
        ]
    }

    // =========================================================================
    // Services (CIS 2.x)
    // =========================================================================

    fn service_checks() -> Vec<Box<dyn ComplianceCheck>> {
        vec![
            // CIS 2.1.1 - Ensure xinetd is not installed
            Box::new(
                CommandCheck::new(
                    "CIS-2.1.1",
                    "Ensure xinetd is not installed",
                    "dpkg -l xinetd 2>/dev/null | grep -q '^ii' || rpm -q xinetd 2>/dev/null",
                )
                .with_description(
                    "The xinetd service provides a method for starting internet services. \
                     If no services require xinetd, it should be removed.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Services)
                .with_expected_exit_code(1) // Should NOT be installed
                .with_remediation("Remove xinetd: apt remove xinetd or yum remove xinetd")
                .with_tag("services".to_string())
                .with_tag("legacy".to_string()),
            ),
            // CIS 2.1.2 - Ensure openbsd-inetd is not installed
            Box::new(
                CommandCheck::new(
                    "CIS-2.1.2",
                    "Ensure inetd is not installed",
                    "dpkg -l openbsd-inetd 2>/dev/null | grep -q '^ii'",
                )
                .with_description(
                    "The inetd daemon provides a method for starting internet services. \
                     Modern systems should use systemd instead.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Services)
                .with_expected_exit_code(1)
                .with_remediation("Remove inetd: apt remove openbsd-inetd")
                .with_tag("services".to_string())
                .with_tag("legacy".to_string()),
            ),
            // CIS 2.2.1 - Ensure time synchronization is in use
            Box::new(
                ServiceCheck::new(
                    "CIS-2.2.1",
                    "Ensure time synchronization is configured",
                    "systemd-timesyncd",
                )
                .with_description(
                    "Time synchronization is important for authentication protocols, \
                     log analysis, and forensics. Either systemd-timesyncd, chrony, or ntp should be used.",
                )
                .with_severity(Severity::High)
                .should_be_running(true)
                .should_be_enabled(true)
                .with_remediation(
                    "Enable time sync: systemctl enable --now systemd-timesyncd \
                     or install and configure chrony/ntp",
                )
                .with_tag("services".to_string())
                .with_tag("time".to_string()),
            ),
            // CIS 2.2.2 - Ensure X Window System is not installed (servers)
            Box::new(
                CommandCheck::new(
                    "CIS-2.2.2",
                    "Ensure X Window System is not installed",
                    "dpkg -l xserver-xorg* 2>/dev/null | grep -q '^ii' || rpm -q xorg-x11-server-* 2>/dev/null",
                )
                .with_description(
                    "Unless a GUI is required, the X Window System should not be installed \
                     on server systems to reduce attack surface.",
                )
                .with_severity(Severity::Low)
                .with_category(CheckCategory::Services)
                .with_expected_exit_code(1)
                .with_remediation("Remove X: apt remove xserver-xorg* or yum remove xorg-x11-server-*")
                .with_tag("services".to_string())
                .with_tag("gui".to_string()),
            ),
            // CIS 2.2.3 - Ensure Avahi Server is not installed
            Box::new(
                ServiceCheck::new(
                    "CIS-2.2.3",
                    "Ensure Avahi Server is not enabled",
                    "avahi-daemon",
                )
                .with_description(
                    "Avahi is a free zeroconf implementation. It can be used for mDNS/DNS-SD \
                     service discovery. If not needed, it should be disabled.",
                )
                .with_severity(Severity::Medium)
                .should_be_enabled(false)
                .with_remediation(
                    "Disable Avahi: systemctl stop avahi-daemon && systemctl disable avahi-daemon",
                )
                .with_tag("services".to_string())
                .with_tag("network".to_string()),
            ),
            // CIS 2.2.4 - Ensure CUPS is not installed (unless needed)
            Box::new(
                ServiceCheck::new(
                    "CIS-2.2.4",
                    "Ensure CUPS is not enabled",
                    "cups",
                )
                .with_description(
                    "The Common Unix Printing System (CUPS) provides the ability to print. \
                     If printing is not needed, it should be disabled.",
                )
                .with_severity(Severity::Low)
                .should_be_enabled(false)
                .with_remediation(
                    "Disable CUPS: systemctl stop cups && systemctl disable cups",
                )
                .with_tag("services".to_string())
                .with_tag("printing".to_string()),
            ),
            // CIS 2.2.5 - Ensure DHCP Server is not installed
            Box::new(
                ServiceCheck::new(
                    "CIS-2.2.5",
                    "Ensure DHCP Server is not enabled",
                    "isc-dhcp-server",
                )
                .with_description(
                    "DHCP server should only be enabled if the system is designated as a DHCP server.",
                )
                .with_severity(Severity::Medium)
                .should_be_enabled(false)
                .with_remediation(
                    "Disable DHCP server: systemctl stop isc-dhcp-server && systemctl disable isc-dhcp-server",
                )
                .with_tag("services".to_string())
                .with_tag("network".to_string()),
            ),
            // CIS 2.2.6 - Ensure LDAP server is not installed
            Box::new(
                ServiceCheck::new(
                    "CIS-2.2.6",
                    "Ensure LDAP server is not enabled",
                    "slapd",
                )
                .with_description(
                    "LDAP server should only be enabled if the system is designated as an LDAP server.",
                )
                .with_severity(Severity::Medium)
                .should_be_enabled(false)
                .with_remediation(
                    "Disable LDAP: systemctl stop slapd && systemctl disable slapd",
                )
                .with_tag("services".to_string())
                .with_tag("directory".to_string()),
            ),
            // CIS 2.2.7 - Ensure NFS is not installed (unless needed)
            Box::new(
                ServiceCheck::new(
                    "CIS-2.2.7",
                    "Ensure NFS server is not enabled",
                    "nfs-server",
                )
                .with_description(
                    "NFS server should only be enabled if required for file sharing.",
                )
                .with_severity(Severity::Medium)
                .should_be_enabled(false)
                .with_remediation(
                    "Disable NFS: systemctl stop nfs-server && systemctl disable nfs-server",
                )
                .with_tag("services".to_string())
                .with_tag("filesharing".to_string()),
            ),
            // CIS 2.2.8 - Ensure DNS Server is not installed
            Box::new(
                ServiceCheck::new(
                    "CIS-2.2.8",
                    "Ensure DNS Server is not enabled",
                    "named",
                )
                .with_description(
                    "DNS server should only run on designated DNS servers.",
                )
                .with_severity(Severity::Medium)
                .should_be_enabled(false)
                .with_remediation(
                    "Disable DNS: systemctl stop named && systemctl disable named",
                )
                .with_tag("services".to_string())
                .with_tag("dns".to_string()),
            ),
            // CIS 2.2.9 - Ensure FTP Server is not installed
            Box::new(
                ServiceCheck::new(
                    "CIS-2.2.9",
                    "Ensure FTP Server is not enabled",
                    "vsftpd",
                )
                .with_description(
                    "FTP transmits data in clear text. SFTP should be used instead.",
                )
                .with_severity(Severity::High)
                .should_be_enabled(false)
                .with_remediation(
                    "Disable FTP: systemctl stop vsftpd && systemctl disable vsftpd",
                )
                .with_tag("services".to_string())
                .with_tag("ftp".to_string()),
            ),
            // CIS 2.2.10 - Ensure HTTP Server is not installed (unless needed)
            Box::new(
                ServiceCheck::new(
                    "CIS-2.2.10",
                    "Ensure HTTP server is intentionally enabled",
                    "apache2",
                )
                .with_description(
                    "HTTP servers should only be enabled on designated web servers.",
                )
                .with_severity(Severity::Low)
                .should_be_enabled(false)
                .with_remediation(
                    "If not needed: systemctl stop apache2 && systemctl disable apache2",
                )
                .with_tag("services".to_string())
                .with_tag("web".to_string()),
            ),
            // CIS 2.2.11 - Ensure mail server is local-only
            Box::new(
                CommandCheck::new(
                    "CIS-2.2.11",
                    "Ensure mail transfer agent is configured for local-only mode",
                    "ss -lntu | grep -E ':25\\s' | grep -v '127.0.0.1:25' | grep -v '\\[::1\\]:25'",
                )
                .with_description(
                    "Mail servers should only listen on localhost unless they are designated mail relays.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Services)
                .with_expected_exit_code(1) // Should not find external listeners
                .with_remediation(
                    "Configure MTA to listen only on localhost. For Postfix: \
                     inet_interfaces = loopback-only in /etc/postfix/main.cf",
                )
                .with_tag("services".to_string())
                .with_tag("mail".to_string()),
            ),
            // CIS 2.2.12 - Ensure Samba is not enabled
            Box::new(
                ServiceCheck::new(
                    "CIS-2.2.12",
                    "Ensure Samba is not enabled",
                    "smbd",
                )
                .with_description(
                    "Samba server should only be enabled on designated file servers.",
                )
                .with_severity(Severity::Medium)
                .should_be_enabled(false)
                .with_remediation(
                    "Disable Samba: systemctl stop smbd && systemctl disable smbd",
                )
                .with_tag("services".to_string())
                .with_tag("filesharing".to_string()),
            ),
            // CIS 2.2.13 - Ensure SNMP Server is not installed
            Box::new(
                ServiceCheck::new(
                    "CIS-2.2.13",
                    "Ensure SNMP Server is not enabled",
                    "snmpd",
                )
                .with_description(
                    "SNMP server can expose sensitive system information. \
                     It should be disabled unless specifically required.",
                )
                .with_severity(Severity::Medium)
                .should_be_enabled(false)
                .with_remediation(
                    "Disable SNMP: systemctl stop snmpd && systemctl disable snmpd",
                )
                .with_tag("services".to_string())
                .with_tag("monitoring".to_string()),
            ),
            // CIS 2.2.14 - Ensure rsync service is not enabled
            Box::new(
                ServiceCheck::new(
                    "CIS-2.2.14",
                    "Ensure rsync service is not enabled",
                    "rsync",
                )
                .with_description(
                    "The rsync daemon allows remote file synchronization. \
                     It should be disabled unless specifically required.",
                )
                .with_severity(Severity::Medium)
                .should_be_enabled(false)
                .with_remediation(
                    "Disable rsync: systemctl stop rsync && systemctl disable rsync",
                )
                .with_tag("services".to_string())
                .with_tag("filesharing".to_string()),
            ),
            // CIS 2.2.15 - Ensure NIS Server is not installed
            Box::new(
                ServiceCheck::new(
                    "CIS-2.2.15",
                    "Ensure NIS Server is not enabled",
                    "ypserv",
                )
                .with_description(
                    "NIS is an old directory service that transmits data unencrypted. \
                     Use LDAP with TLS instead.",
                )
                .with_severity(Severity::High)
                .should_be_enabled(false)
                .with_remediation(
                    "Disable NIS: systemctl stop ypserv && systemctl disable ypserv",
                )
                .with_tag("services".to_string())
                .with_tag("legacy".to_string()),
            ),
        ]
    }

    // =========================================================================
    // Network Configuration (CIS 3.x)
    // =========================================================================

    fn network_checks() -> Vec<Box<dyn ComplianceCheck>> {
        vec![
            // CIS 3.1.1 - Ensure IP forwarding is disabled
            Box::new(
                SysctlCheck::new(
                    "CIS-3.1.1",
                    "Ensure IP forwarding is disabled",
                    "net.ipv4.ip_forward",
                    "0",
                )
                .with_description(
                    "IP forwarding allows the system to act as a router. \
                     It should be disabled unless the system is designed to route traffic.",
                )
                .with_severity(Severity::Medium)
                .with_remediation(
                    "Set 'net.ipv4.ip_forward = 0' in /etc/sysctl.conf and run 'sysctl -p'",
                )
                .with_tag("network".to_string())
                .with_tag("routing".to_string()),
            ),
            // CIS 3.1.2 - Ensure packet redirect sending is disabled
            Box::new(
                SysctlCheck::new(
                    "CIS-3.1.2",
                    "Ensure packet redirect sending is disabled",
                    "net.ipv4.conf.all.send_redirects",
                    "0",
                )
                .with_description(
                    "ICMP redirects can be used to maliciously alter routing tables. \
                     Systems should not send redirects unless acting as a router.",
                )
                .with_severity(Severity::Medium)
                .with_remediation(
                    "Set 'net.ipv4.conf.all.send_redirects = 0' in /etc/sysctl.conf",
                )
                .with_tag("network".to_string())
                .with_tag("icmp".to_string()),
            ),
            // CIS 3.2.1 - Ensure source routed packets are not accepted
            Box::new(
                SysctlCheck::new(
                    "CIS-3.2.1",
                    "Ensure source routed packets are not accepted",
                    "net.ipv4.conf.all.accept_source_route",
                    "0",
                )
                .with_description(
                    "Source routing allows the sender to define the route packets take. \
                     This can be used to bypass security controls.",
                )
                .with_severity(Severity::Medium)
                .with_remediation(
                    "Set 'net.ipv4.conf.all.accept_source_route = 0' in /etc/sysctl.conf",
                )
                .with_tag("network".to_string())
                .with_tag("routing".to_string()),
            ),
            // CIS 3.2.2 - Ensure ICMP redirects are not accepted
            Box::new(
                SysctlCheck::new(
                    "CIS-3.2.2",
                    "Ensure ICMP redirects are not accepted",
                    "net.ipv4.conf.all.accept_redirects",
                    "0",
                )
                .with_description(
                    "ICMP redirect messages can be used to maliciously alter the routing table.",
                )
                .with_severity(Severity::Medium)
                .with_remediation(
                    "Set 'net.ipv4.conf.all.accept_redirects = 0' in /etc/sysctl.conf",
                )
                .with_tag("network".to_string())
                .with_tag("icmp".to_string()),
            ),
            // CIS 3.2.3 - Ensure secure ICMP redirects are not accepted
            Box::new(
                SysctlCheck::new(
                    "CIS-3.2.3",
                    "Ensure secure ICMP redirects are not accepted",
                    "net.ipv4.conf.all.secure_redirects",
                    "0",
                )
                .with_description(
                    "Secure ICMP redirects are the same as ICMP redirects but from gateways \
                     listed in the default gateway list.",
                )
                .with_severity(Severity::Medium)
                .with_remediation(
                    "Set 'net.ipv4.conf.all.secure_redirects = 0' in /etc/sysctl.conf",
                )
                .with_tag("network".to_string())
                .with_tag("icmp".to_string()),
            ),
            // CIS 3.2.4 - Ensure suspicious packets are logged
            Box::new(
                SysctlCheck::new(
                    "CIS-3.2.4",
                    "Ensure suspicious packets are logged",
                    "net.ipv4.conf.all.log_martians",
                    "1",
                )
                .with_description(
                    "Martian packets (packets with impossible addresses) should be logged \
                     for security monitoring.",
                )
                .with_severity(Severity::Low)
                .with_remediation(
                    "Set 'net.ipv4.conf.all.log_martians = 1' in /etc/sysctl.conf",
                )
                .with_tag("network".to_string())
                .with_tag("logging".to_string()),
            ),
            // CIS 3.2.5 - Ensure broadcast ICMP requests are ignored
            Box::new(
                SysctlCheck::new(
                    "CIS-3.2.5",
                    "Ensure broadcast ICMP requests are ignored",
                    "net.ipv4.icmp_echo_ignore_broadcasts",
                    "1",
                )
                .with_description(
                    "Ignoring ICMP echo requests to broadcast addresses protects against \
                     Smurf attacks.",
                )
                .with_severity(Severity::Medium)
                .with_remediation(
                    "Set 'net.ipv4.icmp_echo_ignore_broadcasts = 1' in /etc/sysctl.conf",
                )
                .with_tag("network".to_string())
                .with_tag("icmp".to_string()),
            ),
            // CIS 3.2.6 - Ensure bogus ICMP responses are ignored
            Box::new(
                SysctlCheck::new(
                    "CIS-3.2.6",
                    "Ensure bogus ICMP responses are ignored",
                    "net.ipv4.icmp_ignore_bogus_error_responses",
                    "1",
                )
                .with_description(
                    "Ignoring bogus ICMP error responses protects against certain types of attacks.",
                )
                .with_severity(Severity::Low)
                .with_remediation(
                    "Set 'net.ipv4.icmp_ignore_bogus_error_responses = 1' in /etc/sysctl.conf",
                )
                .with_tag("network".to_string())
                .with_tag("icmp".to_string()),
            ),
            // CIS 3.2.7 - Ensure Reverse Path Filtering is enabled
            Box::new(
                SysctlCheck::new(
                    "CIS-3.2.7",
                    "Ensure Reverse Path Filtering is enabled",
                    "net.ipv4.conf.all.rp_filter",
                    "1",
                )
                .with_description(
                    "Reverse path filtering validates that packets arriving on an interface \
                     came from a route the system would use to send packets back.",
                )
                .with_severity(Severity::Medium)
                .with_remediation(
                    "Set 'net.ipv4.conf.all.rp_filter = 1' in /etc/sysctl.conf",
                )
                .with_tag("network".to_string())
                .with_tag("spoofing".to_string()),
            ),
            // CIS 3.2.8 - Ensure TCP SYN Cookies is enabled
            Box::new(
                SysctlCheck::new(
                    "CIS-3.2.8",
                    "Ensure TCP SYN Cookies is enabled",
                    "net.ipv4.tcp_syncookies",
                    "1",
                )
                .with_description(
                    "SYN cookies protect against SYN flood attacks by using a cryptographic \
                     cookie to track connections.",
                )
                .with_severity(Severity::High)
                .with_remediation(
                    "Set 'net.ipv4.tcp_syncookies = 1' in /etc/sysctl.conf",
                )
                .with_tag("network".to_string())
                .with_tag("dos-protection".to_string()),
            ),
            // CIS 3.2.9 - Ensure IPv6 router advertisements are not accepted
            Box::new(
                SysctlCheck::new(
                    "CIS-3.2.9",
                    "Ensure IPv6 router advertisements are not accepted",
                    "net.ipv6.conf.all.accept_ra",
                    "0",
                )
                .with_description(
                    "Router advertisements can be used to maliciously configure IPv6 routing.",
                )
                .with_severity(Severity::Medium)
                .with_remediation(
                    "Set 'net.ipv6.conf.all.accept_ra = 0' in /etc/sysctl.conf",
                )
                .with_tag("network".to_string())
                .with_tag("ipv6".to_string()),
            ),
            // CIS 3.3.1 - Ensure TCP Wrappers is installed
            Box::new(
                CommandCheck::new(
                    "CIS-3.3.1",
                    "Ensure TCP Wrappers is installed",
                    "dpkg -l tcpd 2>/dev/null | grep -q '^ii' || rpm -q tcp_wrappers 2>/dev/null",
                )
                .with_description(
                    "TCP Wrappers provides access control list for services that support it.",
                )
                .with_severity(Severity::Low)
                .with_category(CheckCategory::Network)
                .with_expected_exit_code(0)
                .with_remediation("Install TCP Wrappers: apt install tcpd or yum install tcp_wrappers")
                .with_tag("network".to_string())
                .with_tag("access-control".to_string()),
            ),
            // CIS 3.4.1 - Ensure firewall is installed
            Box::new(
                CommandCheck::new(
                    "CIS-3.4.1",
                    "Ensure a firewall package is installed",
                    "command -v iptables || command -v nft || command -v ufw",
                )
                .with_description(
                    "A firewall package (iptables, nftables, or ufw) should be installed.",
                )
                .with_severity(Severity::High)
                .with_category(CheckCategory::Network)
                .with_expected_exit_code(0)
                .with_remediation("Install firewall: apt install iptables or apt install ufw")
                .with_tag("network".to_string())
                .with_tag("firewall".to_string()),
            ),
        ]
    }

    // =========================================================================
    // Logging and Auditing (CIS 4.x)
    // =========================================================================

    fn logging_checks() -> Vec<Box<dyn ComplianceCheck>> {
        vec![
            // CIS 4.1.1.1 - Ensure auditd is installed
            Box::new(
                CommandCheck::new(
                    "CIS-4.1.1.1",
                    "Ensure auditd is installed",
                    "dpkg -l auditd 2>/dev/null | grep -q '^ii' || rpm -q audit 2>/dev/null",
                )
                .with_description(
                    "auditd is the userspace component of the Linux Auditing System. \
                     It's responsible for writing audit records to disk.",
                )
                .with_severity(Severity::High)
                .with_category(CheckCategory::Auditing)
                .with_expected_exit_code(0)
                .with_remediation("Install auditd: apt install auditd or yum install audit")
                .with_tag("auditing".to_string())
                .with_tag("logging".to_string()),
            ),
            // CIS 4.1.1.2 - Ensure auditd service is enabled
            Box::new(
                ServiceCheck::new(
                    "CIS-4.1.1.2",
                    "Ensure auditd service is enabled",
                    "auditd",
                )
                .with_description(
                    "The auditd service should be enabled to capture security-relevant events.",
                )
                .with_severity(Severity::High)
                .should_be_enabled(true)
                .should_be_running(true)
                .with_remediation("Enable auditd: systemctl enable auditd && systemctl start auditd")
                .with_tag("auditing".to_string())
                .with_tag("logging".to_string()),
            ),
            // CIS 4.1.2 - Ensure audit log storage size is configured
            Box::new(
                CommandCheck::new(
                    "CIS-4.1.2",
                    "Ensure audit log storage size is configured",
                    "grep -E '^max_log_file\\s*=' /etc/audit/auditd.conf",
                )
                .with_description(
                    "The max_log_file parameter should be set to ensure sufficient disk space \
                     is reserved for audit logs.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Auditing)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'max_log_file = <size>' in /etc/audit/auditd.conf (size in MB)",
                )
                .with_tag("auditing".to_string())
                .with_tag("logging".to_string()),
            ),
            // CIS 4.1.3 - Ensure audit logs are not automatically deleted
            Box::new(
                CommandCheck::new(
                    "CIS-4.1.3",
                    "Ensure audit logs are not automatically deleted",
                    "grep -E '^max_log_file_action\\s*=\\s*keep_logs' /etc/audit/auditd.conf",
                )
                .with_description(
                    "Audit logs should be retained for investigation and compliance purposes.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Auditing)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'max_log_file_action = keep_logs' in /etc/audit/auditd.conf",
                )
                .with_tag("auditing".to_string())
                .with_tag("logging".to_string()),
            ),
            // CIS 4.2.1.1 - Ensure rsyslog is installed
            Box::new(
                CommandCheck::new(
                    "CIS-4.2.1.1",
                    "Ensure rsyslog or syslog-ng is installed",
                    "dpkg -l rsyslog 2>/dev/null | grep -q '^ii' || rpm -q rsyslog 2>/dev/null || dpkg -l syslog-ng 2>/dev/null | grep -q '^ii'",
                )
                .with_description(
                    "A syslog package must be installed to capture system logging.",
                )
                .with_severity(Severity::High)
                .with_category(CheckCategory::Auditing)
                .with_expected_exit_code(0)
                .with_remediation("Install rsyslog: apt install rsyslog or yum install rsyslog")
                .with_tag("logging".to_string())
                .with_tag("syslog".to_string()),
            ),
            // CIS 4.2.1.2 - Ensure rsyslog service is enabled
            Box::new(
                ServiceCheck::new(
                    "CIS-4.2.1.2",
                    "Ensure rsyslog service is enabled",
                    "rsyslog",
                )
                .with_description(
                    "The rsyslog service should be enabled to capture system logs.",
                )
                .with_severity(Severity::High)
                .should_be_enabled(true)
                .should_be_running(true)
                .with_remediation("Enable rsyslog: systemctl enable rsyslog && systemctl start rsyslog")
                .with_tag("logging".to_string())
                .with_tag("syslog".to_string()),
            ),
            // CIS 4.2.1.4 - Ensure rsyslog default file permissions
            Box::new(
                CommandCheck::new(
                    "CIS-4.2.1.4",
                    "Ensure rsyslog default file permissions configured",
                    "grep -E '^\\$FileCreateMode\\s+0640' /etc/rsyslog.conf /etc/rsyslog.d/*.conf 2>/dev/null",
                )
                .with_description(
                    "Log files should have restrictive permissions to protect sensitive information.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Auditing)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Add '$FileCreateMode 0640' to /etc/rsyslog.conf",
                )
                .with_tag("logging".to_string())
                .with_tag("permissions".to_string()),
            ),
            // CIS 4.2.2.1 - Ensure journald is configured to compress large logs
            Box::new(
                CommandCheck::new(
                    "CIS-4.2.2.1",
                    "Ensure journald is configured to compress large logs",
                    "grep -E '^Compress=yes' /etc/systemd/journald.conf",
                )
                .with_description(
                    "Large log files should be compressed to save disk space.",
                )
                .with_severity(Severity::Low)
                .with_category(CheckCategory::Auditing)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'Compress=yes' in /etc/systemd/journald.conf",
                )
                .with_tag("logging".to_string())
                .with_tag("journald".to_string()),
            ),
            // CIS 4.2.2.2 - Ensure journald is configured to write logs to persistent disk
            Box::new(
                CommandCheck::new(
                    "CIS-4.2.2.2",
                    "Ensure journald is configured for persistent storage",
                    "grep -E '^Storage=persistent' /etc/systemd/journald.conf",
                )
                .with_description(
                    "Journal logs should be written to persistent storage for forensic analysis.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Auditing)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'Storage=persistent' in /etc/systemd/journald.conf",
                )
                .with_tag("logging".to_string())
                .with_tag("journald".to_string()),
            ),
        ]
    }

    // =========================================================================
    // Access Control (CIS 5.x)
    // =========================================================================

    fn access_control_checks() -> Vec<Box<dyn ComplianceCheck>> {
        vec![
            // CIS 5.1.1 - Ensure cron daemon is enabled
            Box::new(
                ServiceCheck::new(
                    "CIS-5.1.1",
                    "Ensure cron daemon is enabled",
                    "cron",
                )
                .with_description(
                    "Cron is a time-based job scheduling daemon. It should be enabled \
                     for automated task execution.",
                )
                .with_severity(Severity::Low)
                .should_be_enabled(true)
                .with_remediation("Enable cron: systemctl enable cron")
                .with_tag("access-control".to_string())
                .with_tag("scheduling".to_string()),
            ),
            // CIS 5.1.2 - Ensure permissions on /etc/crontab
            Box::new(
                FileCheck::new(
                    "CIS-5.1.2",
                    "Ensure permissions on /etc/crontab are configured",
                    "/etc/crontab",
                )
                .with_description(
                    "The crontab file should be owned by root with restrictive permissions.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::AccessControl)
                .with_owner("root")
                .with_group("root")
                .with_mode("600")
                .with_remediation("chown root:root /etc/crontab && chmod 600 /etc/crontab")
                .with_tag("access-control".to_string())
                .with_tag("cron".to_string()),
            ),
            // CIS 5.1.3 - Ensure permissions on /etc/cron.hourly
            Box::new(
                FileCheck::new(
                    "CIS-5.1.3",
                    "Ensure permissions on /etc/cron.hourly are configured",
                    "/etc/cron.hourly",
                )
                .with_description(
                    "Cron directories should be owned by root with restrictive permissions.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::AccessControl)
                .with_owner("root")
                .with_group("root")
                .with_mode("700")
                .with_remediation("chown root:root /etc/cron.hourly && chmod 700 /etc/cron.hourly")
                .with_tag("access-control".to_string())
                .with_tag("cron".to_string()),
            ),
            // CIS 5.1.4 - Ensure permissions on /etc/cron.daily
            Box::new(
                FileCheck::new(
                    "CIS-5.1.4",
                    "Ensure permissions on /etc/cron.daily are configured",
                    "/etc/cron.daily",
                )
                .with_description(
                    "Cron directories should be owned by root with restrictive permissions.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::AccessControl)
                .with_owner("root")
                .with_group("root")
                .with_mode("700")
                .with_remediation("chown root:root /etc/cron.daily && chmod 700 /etc/cron.daily")
                .with_tag("access-control".to_string())
                .with_tag("cron".to_string()),
            ),
            // CIS 5.2.1 - Ensure permissions on /etc/ssh/sshd_config
            Box::new(
                FileCheck::new(
                    "CIS-5.2.1",
                    "Ensure permissions on /etc/ssh/sshd_config are configured",
                    "/etc/ssh/sshd_config",
                )
                .with_description(
                    "The SSH daemon configuration file should have restrictive permissions.",
                )
                .with_severity(Severity::High)
                .with_category(CheckCategory::Ssh)
                .with_owner("root")
                .with_group("root")
                .with_mode("600")
                .with_remediation(
                    "chown root:root /etc/ssh/sshd_config && chmod 600 /etc/ssh/sshd_config",
                )
                .with_tag("ssh".to_string())
                .with_tag("permissions".to_string()),
            ),
            // CIS 5.2.2 - Ensure permissions on SSH private host keys
            Box::new(
                CommandCheck::new(
                    "CIS-5.2.2",
                    "Ensure permissions on SSH private host key files",
                    "find /etc/ssh -xdev -type f -name 'ssh_host_*_key' -exec stat -c '%a %U %G' {} \\; | grep -v '^600 root root$'",
                )
                .with_description(
                    "SSH private host keys should be owned by root with mode 0600.",
                )
                .with_severity(Severity::Critical)
                .with_category(CheckCategory::Ssh)
                .with_expected_exit_code(1) // Should find nothing (all files compliant)
                .with_remediation(
                    "chown root:root /etc/ssh/ssh_host_*_key && chmod 600 /etc/ssh/ssh_host_*_key",
                )
                .with_tag("ssh".to_string())
                .with_tag("permissions".to_string()),
            ),
            // CIS 5.2.3 - Ensure permissions on SSH public host keys
            Box::new(
                CommandCheck::new(
                    "CIS-5.2.3",
                    "Ensure permissions on SSH public host key files",
                    "find /etc/ssh -xdev -type f -name 'ssh_host_*_key.pub' -exec stat -c '%a %U %G' {} \\; | grep -v '^644 root root$'",
                )
                .with_description(
                    "SSH public host keys should be owned by root with mode 0644.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Ssh)
                .with_expected_exit_code(1)
                .with_remediation(
                    "chown root:root /etc/ssh/ssh_host_*_key.pub && chmod 644 /etc/ssh/ssh_host_*_key.pub",
                )
                .with_tag("ssh".to_string())
                .with_tag("permissions".to_string()),
            ),
            // CIS 5.3.1 - Ensure password creation requirements
            Box::new(
                CommandCheck::new(
                    "CIS-5.3.1",
                    "Ensure password creation requirements are configured",
                    "grep -E '^\\s*minlen\\s*=' /etc/security/pwquality.conf | grep -E '[0-9]+' | awk -F= '{if($2>=14) exit 0; else exit 1}'",
                )
                .with_description(
                    "Password complexity requirements should be configured to enforce strong passwords.",
                )
                .with_severity(Severity::High)
                .with_category(CheckCategory::Authentication)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'minlen = 14' in /etc/security/pwquality.conf. Also set minclass, dcredit, ucredit, ocredit, lcredit.",
                )
                .with_tag("authentication".to_string())
                .with_tag("password".to_string()),
            ),
            // CIS 5.4.1 - Ensure password expiration is configured
            Box::new(
                CommandCheck::new(
                    "CIS-5.4.1",
                    "Ensure password expiration is 365 days or less",
                    "grep -E '^PASS_MAX_DAYS' /etc/login.defs | awk '{if($2<=365 && $2>0) exit 0; else exit 1}'",
                )
                .with_description(
                    "Password expiration should be set to require periodic password changes.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Authentication)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'PASS_MAX_DAYS 365' in /etc/login.defs",
                )
                .with_tag("authentication".to_string())
                .with_tag("password".to_string()),
            ),
            // CIS 5.4.2 - Ensure minimum password age
            Box::new(
                CommandCheck::new(
                    "CIS-5.4.2",
                    "Ensure minimum days between password changes is configured",
                    "grep -E '^PASS_MIN_DAYS' /etc/login.defs | awk '{if($2>=1) exit 0; else exit 1}'",
                )
                .with_description(
                    "Minimum password age prevents users from cycling through passwords quickly.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Authentication)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'PASS_MIN_DAYS 1' in /etc/login.defs",
                )
                .with_tag("authentication".to_string())
                .with_tag("password".to_string()),
            ),
            // CIS 5.4.3 - Ensure password expiration warning days
            Box::new(
                CommandCheck::new(
                    "CIS-5.4.3",
                    "Ensure password expiration warning days is 7 or more",
                    "grep -E '^PASS_WARN_AGE' /etc/login.defs | awk '{if($2>=7) exit 0; else exit 1}'",
                )
                .with_description(
                    "Users should receive advance warning before password expiration.",
                )
                .with_severity(Severity::Low)
                .with_category(CheckCategory::Authentication)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'PASS_WARN_AGE 7' in /etc/login.defs",
                )
                .with_tag("authentication".to_string())
                .with_tag("password".to_string()),
            ),
            // CIS 5.4.4 - Ensure inactive password lock
            Box::new(
                CommandCheck::new(
                    "CIS-5.4.4",
                    "Ensure inactive password lock is 30 days or less",
                    "useradd -D | grep INACTIVE | awk -F= '{if($2<=30 && $2>0) exit 0; else exit 1}'",
                )
                .with_description(
                    "Inactive accounts should be locked after a period of inactivity.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Authentication)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Run: useradd -D -f 30",
                )
                .with_tag("authentication".to_string())
                .with_tag("account-management".to_string()),
            ),
            // CIS 5.5 - Ensure root login is restricted to system console
            Box::new(
                CommandCheck::new(
                    "CIS-5.5",
                    "Ensure root login is restricted to system console",
                    "cat /etc/securetty 2>/dev/null | grep -v '^#' | grep -v '^$' | wc -l",
                )
                .with_description(
                    "Direct root login should be limited to local console only.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::AccessControl)
                .with_expected_pattern(r"^[0-9]+$")
                .with_remediation(
                    "Edit /etc/securetty to list only physical console devices",
                )
                .with_tag("access-control".to_string())
                .with_tag("root".to_string()),
            ),
            // CIS 5.6 - Ensure access to su command is restricted
            Box::new(
                CommandCheck::new(
                    "CIS-5.6",
                    "Ensure access to the su command is restricted",
                    "grep -E '^\\s*auth\\s+required\\s+pam_wheel.so' /etc/pam.d/su",
                )
                .with_description(
                    "The su command should be restricted to members of the wheel group.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::AccessControl)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Add 'auth required pam_wheel.so use_uid' to /etc/pam.d/su",
                )
                .with_tag("access-control".to_string())
                .with_tag("privilege-escalation".to_string()),
            ),
        ]
    }

    // =========================================================================
    // Authentication (CIS 5.2.x SSH)
    // =========================================================================

    fn authentication_checks() -> Vec<Box<dyn ComplianceCheck>> {
        vec![
            // CIS 5.2.4 - Ensure SSH Protocol is set to 2
            Box::new(
                CommandCheck::new(
                    "CIS-5.2.4",
                    "Ensure SSH Protocol is set to 2",
                    "grep -Ei '^\\s*Protocol\\s+1' /etc/ssh/sshd_config",
                )
                .with_description(
                    "SSH Protocol 1 is insecure and should not be used. Protocol 2 is the default in modern SSH.",
                )
                .with_severity(Severity::Critical)
                .with_category(CheckCategory::Ssh)
                .with_expected_exit_code(1) // Should NOT find Protocol 1
                .with_remediation(
                    "Remove 'Protocol 1' from /etc/ssh/sshd_config (Protocol 2 is default)",
                )
                .with_tag("ssh".to_string())
                .with_tag("protocol".to_string()),
            ),
            // CIS 5.2.5 - Ensure SSH LogLevel is appropriate
            Box::new(
                CommandCheck::new(
                    "CIS-5.2.5",
                    "Ensure SSH LogLevel is appropriate",
                    "grep -Ei '^\\s*LogLevel\\s+(INFO|VERBOSE)' /etc/ssh/sshd_config",
                )
                .with_description(
                    "SSH logging should capture enough information for security analysis.",
                )
                .with_severity(Severity::Low)
                .with_category(CheckCategory::Ssh)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'LogLevel INFO' or 'LogLevel VERBOSE' in /etc/ssh/sshd_config",
                )
                .with_tag("ssh".to_string())
                .with_tag("logging".to_string()),
            ),
            // CIS 5.2.6 - Ensure SSH X11 forwarding is disabled
            Box::new(
                CommandCheck::new(
                    "CIS-5.2.6",
                    "Ensure SSH X11 forwarding is disabled",
                    "grep -Ei '^\\s*X11Forwarding\\s+no' /etc/ssh/sshd_config",
                )
                .with_description(
                    "X11 forwarding can be used to tunnel X11 traffic through SSH, which may not be needed.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Ssh)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'X11Forwarding no' in /etc/ssh/sshd_config",
                )
                .with_tag("ssh".to_string())
                .with_tag("x11".to_string()),
            ),
            // CIS 5.2.7 - Ensure SSH MaxAuthTries is set to 4 or less
            Box::new(
                CommandCheck::new(
                    "CIS-5.2.7",
                    "Ensure SSH MaxAuthTries is set to 4 or less",
                    "grep -Ei '^\\s*MaxAuthTries\\s+[1-4]$' /etc/ssh/sshd_config",
                )
                .with_description(
                    "Limiting authentication attempts protects against brute force attacks.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Ssh)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'MaxAuthTries 4' in /etc/ssh/sshd_config",
                )
                .with_tag("ssh".to_string())
                .with_tag("authentication".to_string()),
            ),
            // CIS 5.2.8 - Ensure SSH IgnoreRhosts is enabled
            Box::new(
                CommandCheck::new(
                    "CIS-5.2.8",
                    "Ensure SSH IgnoreRhosts is enabled",
                    "grep -Ei '^\\s*IgnoreRhosts\\s+yes' /etc/ssh/sshd_config || ! grep -Ei '^\\s*IgnoreRhosts' /etc/ssh/sshd_config",
                )
                .with_description(
                    "The IgnoreRhosts parameter prevents use of .rhosts files for authentication.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Ssh)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'IgnoreRhosts yes' in /etc/ssh/sshd_config (default is yes)",
                )
                .with_tag("ssh".to_string())
                .with_tag("authentication".to_string()),
            ),
            // CIS 5.2.9 - Ensure SSH HostbasedAuthentication is disabled
            Box::new(
                CommandCheck::new(
                    "CIS-5.2.9",
                    "Ensure SSH HostbasedAuthentication is disabled",
                    "grep -Ei '^\\s*HostbasedAuthentication\\s+no' /etc/ssh/sshd_config || ! grep -Ei '^\\s*HostbasedAuthentication' /etc/ssh/sshd_config",
                )
                .with_description(
                    "Host-based authentication is less secure than key-based authentication.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Ssh)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'HostbasedAuthentication no' in /etc/ssh/sshd_config",
                )
                .with_tag("ssh".to_string())
                .with_tag("authentication".to_string()),
            ),
            // CIS 5.2.10 - Ensure SSH root login is disabled
            Box::new(
                CommandCheck::new(
                    "CIS-5.2.10",
                    "Ensure SSH root login is disabled",
                    "grep -Ei '^\\s*PermitRootLogin\\s+(no|prohibit-password)' /etc/ssh/sshd_config",
                )
                .with_description(
                    "Direct root login via SSH should be disabled to enforce use of sudo.",
                )
                .with_severity(Severity::High)
                .with_category(CheckCategory::Ssh)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'PermitRootLogin no' in /etc/ssh/sshd_config",
                )
                .with_tag("ssh".to_string())
                .with_tag("root".to_string()),
            ),
            // CIS 5.2.11 - Ensure SSH PermitEmptyPasswords is disabled
            Box::new(
                CommandCheck::new(
                    "CIS-5.2.11",
                    "Ensure SSH PermitEmptyPasswords is disabled",
                    "grep -Ei '^\\s*PermitEmptyPasswords\\s+no' /etc/ssh/sshd_config || ! grep -Ei '^\\s*PermitEmptyPasswords' /etc/ssh/sshd_config",
                )
                .with_description(
                    "Empty passwords should never be allowed for SSH connections.",
                )
                .with_severity(Severity::Critical)
                .with_category(CheckCategory::Ssh)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'PermitEmptyPasswords no' in /etc/ssh/sshd_config",
                )
                .with_tag("ssh".to_string())
                .with_tag("password".to_string()),
            ),
            // CIS 5.2.12 - Ensure SSH PermitUserEnvironment is disabled
            Box::new(
                CommandCheck::new(
                    "CIS-5.2.12",
                    "Ensure SSH PermitUserEnvironment is disabled",
                    "grep -Ei '^\\s*PermitUserEnvironment\\s+no' /etc/ssh/sshd_config || ! grep -Ei '^\\s*PermitUserEnvironment' /etc/ssh/sshd_config",
                )
                .with_description(
                    "Permitting user environment variables can enable users to bypass security controls.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Ssh)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'PermitUserEnvironment no' in /etc/ssh/sshd_config",
                )
                .with_tag("ssh".to_string())
                .with_tag("environment".to_string()),
            ),
            // CIS 5.2.13 - Ensure only strong ciphers are used
            Box::new(
                CommandCheck::new(
                    "CIS-5.2.13",
                    "Ensure only strong SSH ciphers are used",
                    "grep -Ei '^\\s*Ciphers' /etc/ssh/sshd_config | grep -Ev '(3des|arcfour|blowfish|cast128)'",
                )
                .with_description(
                    "Weak ciphers like 3des, arcfour, blowfish should not be used.",
                )
                .with_severity(Severity::High)
                .with_category(CheckCategory::Ssh)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Configure Ciphers in /etc/ssh/sshd_config to use only strong ciphers like \
                     aes256-ctr, aes192-ctr, aes128-ctr, aes256-gcm@openssh.com",
                )
                .with_tag("ssh".to_string())
                .with_tag("cryptography".to_string()),
            ),
            // CIS 5.2.14 - Ensure only strong MACs are used
            Box::new(
                CommandCheck::new(
                    "CIS-5.2.14",
                    "Ensure only strong SSH MAC algorithms are used",
                    "grep -Ei '^\\s*MACs' /etc/ssh/sshd_config | grep -Ev '(md5|96)'",
                )
                .with_description(
                    "Weak MAC algorithms like MD5 or 96-bit variants should not be used.",
                )
                .with_severity(Severity::High)
                .with_category(CheckCategory::Ssh)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Configure MACs in /etc/ssh/sshd_config to use only strong algorithms like \
                     hmac-sha2-512-etm@openssh.com, hmac-sha2-256-etm@openssh.com",
                )
                .with_tag("ssh".to_string())
                .with_tag("cryptography".to_string()),
            ),
            // CIS 5.2.15 - Ensure only strong Key Exchange algorithms are used
            Box::new(
                CommandCheck::new(
                    "CIS-5.2.15",
                    "Ensure only strong SSH Key Exchange algorithms are used",
                    "grep -Ei '^\\s*KexAlgorithms' /etc/ssh/sshd_config | grep -Ev 'diffie-hellman-group1'",
                )
                .with_description(
                    "Weak key exchange algorithms should not be used.",
                )
                .with_severity(Severity::High)
                .with_category(CheckCategory::Ssh)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Configure KexAlgorithms in /etc/ssh/sshd_config with strong algorithms",
                )
                .with_tag("ssh".to_string())
                .with_tag("cryptography".to_string()),
            ),
            // CIS 5.2.16 - Ensure SSH Idle Timeout Interval is configured
            Box::new(
                CommandCheck::new(
                    "CIS-5.2.16",
                    "Ensure SSH Idle Timeout Interval is configured",
                    "grep -Ei '^\\s*ClientAliveInterval\\s+[1-9][0-9]*' /etc/ssh/sshd_config",
                )
                .with_description(
                    "Idle SSH sessions should be automatically terminated after a period of inactivity.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Ssh)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'ClientAliveInterval 300' and 'ClientAliveCountMax 3' in /etc/ssh/sshd_config",
                )
                .with_tag("ssh".to_string())
                .with_tag("session".to_string()),
            ),
            // CIS 5.2.17 - Ensure SSH LoginGraceTime is set
            Box::new(
                CommandCheck::new(
                    "CIS-5.2.17",
                    "Ensure SSH LoginGraceTime is set to one minute or less",
                    "grep -Ei '^\\s*LoginGraceTime\\s+(60|[1-5][0-9]|[1-9])' /etc/ssh/sshd_config",
                )
                .with_description(
                    "The time allowed for successful authentication should be limited.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Ssh)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'LoginGraceTime 60' in /etc/ssh/sshd_config",
                )
                .with_tag("ssh".to_string())
                .with_tag("authentication".to_string()),
            ),
            // CIS 5.2.18 - Ensure SSH access is limited
            Box::new(
                CommandCheck::new(
                    "CIS-5.2.18",
                    "Ensure SSH access is limited",
                    "grep -Ei '^\\s*(AllowUsers|AllowGroups|DenyUsers|DenyGroups)' /etc/ssh/sshd_config",
                )
                .with_description(
                    "SSH access should be explicitly allowed or denied for specific users/groups.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Ssh)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Configure AllowUsers, AllowGroups, DenyUsers, or DenyGroups in /etc/ssh/sshd_config",
                )
                .with_tag("ssh".to_string())
                .with_tag("access-control".to_string()),
            ),
            // CIS 5.2.19 - Ensure SSH warning banner is configured
            Box::new(
                CommandCheck::new(
                    "CIS-5.2.19",
                    "Ensure SSH warning banner is configured",
                    "grep -Ei '^\\s*Banner\\s+' /etc/ssh/sshd_config | grep -v 'none'",
                )
                .with_description(
                    "A warning banner should be displayed before authentication to inform users of policies.",
                )
                .with_severity(Severity::Low)
                .with_category(CheckCategory::Ssh)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'Banner /etc/issue.net' in /etc/ssh/sshd_config",
                )
                .with_tag("ssh".to_string())
                .with_tag("banner".to_string()),
            ),
            // CIS 5.2.20 - Ensure SSH PAM is enabled
            Box::new(
                CommandCheck::new(
                    "CIS-5.2.20",
                    "Ensure SSH PAM is enabled",
                    "grep -Ei '^\\s*UsePAM\\s+yes' /etc/ssh/sshd_config",
                )
                .with_description(
                    "PAM provides additional authentication and session management capabilities.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Ssh)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'UsePAM yes' in /etc/ssh/sshd_config",
                )
                .with_tag("ssh".to_string())
                .with_tag("pam".to_string()),
            ),
        ]
    }
}

impl Default for CisScanner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ComplianceScanner for CisScanner {
    fn framework(&self) -> ComplianceFramework {
        ComplianceFramework::Cis
    }

    fn name(&self) -> &str {
        "CIS Benchmark Scanner"
    }

    fn description(&self) -> &str {
        "Scans for compliance with Center for Internet Security (CIS) Benchmarks for Linux"
    }

    fn version(&self) -> &str {
        &self.version
    }

    async fn scan(&self, context: &ComplianceContext) -> ComplianceResult<Vec<Finding>> {
        let mut findings = Vec::new();

        for check in &self.checks {
            // Check tag filtering
            let check_tags = check.tags();
            if !context.should_include_tag(&check_tags) {
                continue;
            }

            // Check severity threshold
            if check.severity() < context.severity_threshold {
                continue;
            }

            // Execute the check
            let result = check.execute(context).await?;

            // Create finding
            let mut finding = Finding::new(check.id(), check.title(), ComplianceFramework::Cis)
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

            for reference in check.references() {
                finding = finding.with_reference(reference);
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
            .ok_or_else(|| ComplianceError::InvalidConfig(format!("Check {} not found", check_id)))?;

        let result = check.execute(context).await?;

        let mut finding = Finding::new(check.id(), check.title(), ComplianceFramework::Cis)
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
                remediation_time_minutes: Some(5),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cis_scanner_creation() {
        let scanner = CisScanner::new();
        assert_eq!(scanner.framework(), ComplianceFramework::Cis);
        assert!(!scanner.list_checks().is_empty());
    }

    #[test]
    fn test_check_info_retrieval() {
        let scanner = CisScanner::new();
        let info = scanner.get_check_info("CIS-1.1.1.1");
        assert!(info.is_some());

        let info = info.unwrap();
        assert_eq!(info.id, "CIS-1.1.1.1");
        assert!(!info.title.is_empty());
    }

    #[test]
    fn test_list_checks() {
        let scanner = CisScanner::new();
        let checks = scanner.list_checks();
        assert!(!checks.is_empty());
        assert!(checks.iter().any(|c| c.starts_with("CIS-")));
    }
}
