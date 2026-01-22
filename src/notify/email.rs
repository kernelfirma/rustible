//! Email notification backend.

use std::fmt::Write;
use std::io::{Read, Write as IoWrite};
use std::net::TcpStream;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;
use tracing::{debug, info, warn};

use super::config::EmailConfig;
use super::error::{NotificationError, NotificationResult};
use super::{HostStats, NotificationEvent, Notifier};

/// Email notification backend.
#[derive(Debug)]
pub struct EmailNotifier {
    config: EmailConfig,
    timeout: Duration,
}

impl EmailNotifier {
    /// Creates a new email notifier with the given configuration.
    pub fn new(config: EmailConfig, timeout: Duration) -> NotificationResult<Self> {
        config.validate()?;
        Ok(Self { config, timeout })
    }

    /// Creates an email notifier from environment variables.
    pub fn from_env(timeout: Duration) -> Option<Self> {
        let config = EmailConfig::from_env()?;
        Self::new(config, timeout).ok()
    }

    /// Generates the email subject for an event.
    fn generate_subject(&self, event: &NotificationEvent) -> String {
        let prefix = &self.config.subject_prefix;

        match event {
            NotificationEvent::PlaybookStart { playbook, .. } => {
                format!("{} Playbook '{}' started", prefix, playbook)
            }
            NotificationEvent::PlaybookComplete {
                playbook, success, ..
            } => {
                if *success {
                    format!("{} Playbook '{}' completed successfully", prefix, playbook)
                } else {
                    format!("{} Playbook '{}' FAILED", prefix, playbook)
                }
            }
            NotificationEvent::TaskFailed { playbook, task, .. } => {
                format!("{} Task '{}' failed in '{}'", prefix, task, playbook)
            }
            NotificationEvent::HostUnreachable { host, .. } => {
                format!("{} Host '{}' unreachable", prefix, host)
            }
            NotificationEvent::Custom { name, .. } => {
                format!("{} {}", prefix, name)
            }
        }
    }

    /// Generates the email body for an event.
    fn generate_body(&self, event: &NotificationEvent) -> String {
        let mut body = String::with_capacity(2048);

        writeln!(body, "Rustible Notification").unwrap();
        writeln!(body, "======================").unwrap();
        writeln!(body).unwrap();

        match event {
            NotificationEvent::PlaybookStart {
                playbook,
                hosts,
                timestamp,
            } => {
                writeln!(body, "Event: Playbook Started").unwrap();
                writeln!(body, "Playbook: {}", playbook).unwrap();
                writeln!(body, "Hosts: {} host(s)", hosts.len()).unwrap();
                writeln!(body, "Time: {}", timestamp).unwrap();
                writeln!(body).unwrap();

                if hosts.len() <= 10 {
                    writeln!(body, "Target Hosts:").unwrap();
                    for host in hosts {
                        writeln!(body, "  - {}", host).unwrap();
                    }
                } else {
                    writeln!(body, "Target Hosts (first 10):").unwrap();
                    for host in hosts.iter().take(10) {
                        writeln!(body, "  - {}", host).unwrap();
                    }
                    writeln!(body, "  ... and {} more", hosts.len() - 10).unwrap();
                }
            }

            NotificationEvent::PlaybookComplete {
                playbook,
                success,
                duration_secs,
                host_stats,
                timestamp,
                failures,
            } => {
                let status = if *success { "SUCCESS" } else { "FAILED" };
                writeln!(body, "Event: Playbook Completed").unwrap();
                writeln!(body, "Playbook: {}", playbook).unwrap();
                writeln!(body, "Status: {}", status).unwrap();
                writeln!(body, "Duration: {}", format_duration(*duration_secs)).unwrap();
                writeln!(body, "Time: {}", timestamp).unwrap();
                writeln!(body).unwrap();

                // Host summary
                if !host_stats.is_empty() {
                    writeln!(body, "Host Summary").unwrap();
                    writeln!(body, "------------").unwrap();

                    let mut hosts: Vec<_> = host_stats.keys().collect();
                    hosts.sort();

                    let mut total = HostStats::default();
                    for host in &hosts {
                        if let Some(stats) = host_stats.get(*host) {
                            writeln!(
                                body,
                                "  {}: ok={} changed={} failed={} skipped={}",
                                host, stats.ok, stats.changed, stats.failed, stats.skipped
                            )
                            .unwrap();
                            total.ok += stats.ok;
                            total.changed += stats.changed;
                            total.failed += stats.failed;
                            total.skipped += stats.skipped;
                        }
                    }

                    writeln!(body).unwrap();
                    writeln!(body, "Totals:").unwrap();
                    writeln!(body, "  OK: {}", total.ok).unwrap();
                    writeln!(body, "  Changed: {}", total.changed).unwrap();
                    writeln!(body, "  Failed: {}", total.failed).unwrap();
                    writeln!(body, "  Skipped: {}", total.skipped).unwrap();
                }

                // Failure details
                if let Some(failures) = failures {
                    if !failures.is_empty() {
                        writeln!(body).unwrap();
                        writeln!(body, "Failure Details").unwrap();
                        writeln!(body, "---------------").unwrap();

                        for failure in failures {
                            writeln!(body, "  Host: {}", failure.host).unwrap();
                            writeln!(body, "  Task: {}", failure.task).unwrap();
                            writeln!(body, "  Error: {}", failure.message).unwrap();
                            writeln!(body).unwrap();
                        }
                    }
                }
            }

            NotificationEvent::TaskFailed {
                playbook,
                task,
                host,
                error,
                timestamp,
            } => {
                writeln!(body, "Event: Task Failed").unwrap();
                writeln!(body, "Playbook: {}", playbook).unwrap();
                writeln!(body, "Task: {}", task).unwrap();
                writeln!(body, "Host: {}", host).unwrap();
                writeln!(body, "Time: {}", timestamp).unwrap();
                writeln!(body).unwrap();
                writeln!(body, "Error:").unwrap();
                writeln!(body, "{}", error).unwrap();
            }

            NotificationEvent::HostUnreachable {
                playbook,
                host,
                error,
                timestamp,
            } => {
                writeln!(body, "Event: Host Unreachable").unwrap();
                writeln!(body, "Playbook: {}", playbook).unwrap();
                writeln!(body, "Host: {}", host).unwrap();
                writeln!(body, "Time: {}", timestamp).unwrap();
                writeln!(body).unwrap();
                writeln!(body, "Error:").unwrap();
                writeln!(body, "{}", error).unwrap();
            }

            NotificationEvent::Custom {
                name,
                data,
                timestamp,
            } => {
                writeln!(body, "Event: {}", name).unwrap();
                writeln!(body, "Time: {}", timestamp).unwrap();
                writeln!(body).unwrap();
                writeln!(body, "Data:").unwrap();
                if let Ok(pretty) = serde_json::to_string_pretty(data) {
                    writeln!(body, "{}", pretty).unwrap();
                } else {
                    writeln!(body, "{}", data).unwrap();
                }
            }
        }

        writeln!(body).unwrap();
        writeln!(body, "---").unwrap();
        writeln!(body, "Generated by Rustible").unwrap();

        body
    }

    /// Sends the email via SMTP.
    async fn send_smtp(&self, subject: &str, body: &str) -> NotificationResult<()> {
        let config = self.config.clone();
        let timeout = self.timeout;
        let subject = subject.to_string();
        let body = body.to_string();

        tokio::task::spawn_blocking(move || {
            send_smtp_email(&config, &subject, &body, timeout)
        })
        .await
        .map_err(|e| NotificationError::internal(format!("Failed to spawn email task: {}", e)))?
    }
}

#[async_trait]
impl Notifier for EmailNotifier {
    fn name(&self) -> &str {
        "Email"
    }

    fn is_configured(&self) -> bool {
        !self.config.host.is_empty() && !self.config.from.is_empty() && !self.config.to.is_empty()
    }

    async fn send(&self, event: &NotificationEvent) -> NotificationResult<()> {
        if !self.is_configured() {
            return Err(NotificationError::not_configured("Email"));
        }

        let subject = self.generate_subject(event);
        let body = self.generate_body(event);

        debug!("Sending email notification for event: {}", event.event_type());

        self.send_smtp(&subject, &body).await?;

        info!(
            "Email notification sent successfully to {} recipient(s)",
            self.config.to.len()
        );
        Ok(())
    }
}

/// Sends an email via SMTP (blocking).
fn send_smtp_email(
    config: &EmailConfig,
    subject: &str,
    body: &str,
    timeout: Duration,
) -> NotificationResult<()> {
    let addr = format!("{}:{}", config.host, config.port);
    let stream = TcpStream::connect_timeout(
        &addr.parse().map_err(|e| {
            NotificationError::config(format!("Invalid SMTP address '{}': {}", addr, e))
        })?,
        timeout,
    )
    .map_err(|e| {
        NotificationError::network(format!("Failed to connect to SMTP server '{}': {}", addr, e))
    })?;

    stream.set_read_timeout(Some(timeout)).ok();
    stream.set_write_timeout(Some(timeout)).ok();

    let mut smtp = SmtpConnection::new(stream);

    // Read greeting
    smtp.read_response(220)?;

    // Send EHLO
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "localhost".to_string());
    smtp.send_command(&format!("EHLO {}", hostname))?;
    smtp.read_response(250)?;

    // Handle STARTTLS if configured
    if config.use_starttls {
        smtp.send_command("STARTTLS")?;
        let response = smtp.read_response(220);
        if response.is_err() {
            warn!("STARTTLS not supported by server, continuing without TLS");
        }
        // Note: Actual TLS upgrade would require a TLS library
    }

    // Authenticate if credentials provided
    if let (Some(user), Some(pass)) = (&config.username, &config.password) {
        smtp.send_command("AUTH LOGIN")?;
        smtp.read_response(334)?;

        let user_b64 = base64::engine::general_purpose::STANDARD.encode(user);
        smtp.send_command(&user_b64)?;
        smtp.read_response(334)?;

        let pass_b64 = base64::engine::general_purpose::STANDARD.encode(pass);
        smtp.send_command(&pass_b64)?;
        smtp.read_response(235)?;
    }

    // MAIL FROM
    smtp.send_command(&format!("MAIL FROM:<{}>", config.from))?;
    smtp.read_response(250)?;

    // RCPT TO for each recipient
    for recipient in &config.to {
        smtp.send_command(&format!("RCPT TO:<{}>", recipient))?;
        smtp.read_response(250)?;
    }

    // CC recipients
    for recipient in &config.cc {
        smtp.send_command(&format!("RCPT TO:<{}>", recipient))?;
        smtp.read_response(250)?;
    }

    // BCC recipients
    for recipient in &config.bcc {
        smtp.send_command(&format!("RCPT TO:<{}>", recipient))?;
        smtp.read_response(250)?;
    }

    // DATA
    smtp.send_command("DATA")?;
    smtp.read_response(354)?;

    // Build and send email message
    let message = build_email_message(config, subject, body);
    smtp.send_data(&message)?;
    smtp.read_response(250)?;

    // QUIT
    smtp.send_command("QUIT")?;

    Ok(())
}

/// Builds the email message with headers and body.
fn build_email_message(config: &EmailConfig, subject: &str, body: &str) -> String {
    let mut message = String::with_capacity(body.len() + 512);

    // Headers
    writeln!(message, "From: {}", config.from).unwrap();
    writeln!(message, "To: {}", config.to.join(", ")).unwrap();

    if !config.cc.is_empty() {
        writeln!(message, "Cc: {}", config.cc.join(", ")).unwrap();
    }

    writeln!(message, "Subject: {}", subject).unwrap();
    writeln!(message, "MIME-Version: 1.0").unwrap();
    writeln!(message, "Content-Type: text/plain; charset=utf-8").unwrap();
    writeln!(message, "Content-Transfer-Encoding: 8bit").unwrap();
    writeln!(message, "X-Mailer: Rustible").unwrap();
    writeln!(message, "X-Priority: 3").unwrap();

    // Date header
    let now = chrono::Utc::now();
    writeln!(
        message,
        "Date: {}",
        now.format("%a, %d %b %Y %H:%M:%S +0000")
    )
    .unwrap();

    // Empty line separates headers from body
    writeln!(message).unwrap();

    // Body (escape lone dots per SMTP spec)
    for line in body.lines() {
        if line == "." {
            writeln!(message, "..").unwrap();
        } else {
            writeln!(message, "{}", line).unwrap();
        }
    }

    // End with CRLF.CRLF
    message.push_str("\r\n.\r\n");

    message
}

/// Simple SMTP connection wrapper.
struct SmtpConnection {
    stream: TcpStream,
    buffer: Vec<u8>,
}

impl SmtpConnection {
    fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            buffer: vec![0u8; 1024],
        }
    }

    fn send_command(&mut self, command: &str) -> NotificationResult<()> {
        let cmd = format!("{}\r\n", command);
        self.stream
            .write_all(cmd.as_bytes())
            .map_err(|e| NotificationError::smtp(format!("Failed to send command: {}", e)))?;
        self.stream
            .flush()
            .map_err(|e| NotificationError::smtp(format!("Failed to flush: {}", e)))?;
        Ok(())
    }

    fn send_data(&mut self, data: &str) -> NotificationResult<()> {
        self.stream
            .write_all(data.as_bytes())
            .map_err(|e| NotificationError::smtp(format!("Failed to send data: {}", e)))?;
        self.stream
            .flush()
            .map_err(|e| NotificationError::smtp(format!("Failed to flush: {}", e)))?;
        Ok(())
    }

    fn read_response(&mut self, expected_code: u16) -> NotificationResult<String> {
        let mut response = String::new();

        loop {
            let n = self
                .stream
                .read(&mut self.buffer)
                .map_err(|e| NotificationError::smtp(format!("Failed to read response: {}", e)))?;

            if n == 0 {
                return Err(NotificationError::smtp("Connection closed unexpectedly"));
            }

            response.push_str(&String::from_utf8_lossy(&self.buffer[..n]));

            // Check if we have a complete response
            if response.ends_with("\r\n") {
                let last_line = response.lines().last().unwrap_or("");
                if last_line.len() >= 4 && last_line.chars().nth(3) == Some(' ') {
                    break;
                }
            }
        }

        // Parse response code
        let code: u16 = response
            .chars()
            .take(3)
            .collect::<String>()
            .parse()
            .map_err(|_| NotificationError::smtp(format!("Invalid SMTP response: {}", response)))?;

        if code != expected_code {
            return Err(NotificationError::smtp(format!(
                "Unexpected SMTP response: expected {}, got {}",
                expected_code, response
            )));
        }

        Ok(response)
    }
}

/// Formats a duration in seconds to a human-readable string.
fn format_duration(secs: f64) -> String {
    if secs < 60.0 {
        format!("{:.1}s", secs)
    } else if secs < 3600.0 {
        let mins = (secs / 60.0).floor();
        let remaining = secs % 60.0;
        format!("{:.0}m {:.1}s", mins, remaining)
    } else {
        let hours = (secs / 3600.0).floor();
        let mins = ((secs % 3600.0) / 60.0).floor();
        format!("{:.0}h {:.0}m", hours, mins)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::FailureInfo;
    use std::collections::HashMap;

    fn create_test_config() -> EmailConfig {
        EmailConfig::new("smtp.example.com", "from@example.com")
            .add_recipient("to@example.com")
            .with_port(587)
    }

    #[test]
    fn test_generate_subject_playbook_start() {
        let config = create_test_config();
        let notifier = EmailNotifier {
            config,
            timeout: Duration::from_secs(30),
        };

        let event = NotificationEvent::playbook_start("deploy.yml", vec!["host1".to_string()]);
        let subject = notifier.generate_subject(&event);

        assert!(subject.contains("[Rustible]"));
        assert!(subject.contains("deploy.yml"));
        assert!(subject.contains("started"));
    }

    #[test]
    fn test_generate_subject_playbook_complete_success() {
        let config = create_test_config();
        let notifier = EmailNotifier {
            config,
            timeout: Duration::from_secs(30),
        };

        let event = NotificationEvent::playbook_complete(
            "deploy.yml",
            true,
            Duration::from_secs(45),
            HashMap::new(),
            None,
        );
        let subject = notifier.generate_subject(&event);

        assert!(subject.contains("successfully"));
    }

    #[test]
    fn test_generate_subject_playbook_complete_failure() {
        let config = create_test_config();
        let notifier = EmailNotifier {
            config,
            timeout: Duration::from_secs(30),
        };

        let event = NotificationEvent::playbook_complete(
            "deploy.yml",
            false,
            Duration::from_secs(10),
            HashMap::new(),
            None,
        );
        let subject = notifier.generate_subject(&event);

        assert!(subject.contains("FAILED"));
    }

    #[test]
    fn test_generate_body_playbook_complete() {
        let config = create_test_config();
        let notifier = EmailNotifier {
            config,
            timeout: Duration::from_secs(30),
        };

        let mut host_stats = HashMap::new();
        host_stats.insert("host1".to_string(), HostStats::new(5, 2, 1, 0, 0));

        let failures = vec![FailureInfo::new("host1", "task1", "Connection timeout")];

        let event = NotificationEvent::playbook_complete(
            "deploy.yml",
            false,
            Duration::from_secs(120),
            host_stats,
            Some(failures),
        );

        let body = notifier.generate_body(&event);

        assert!(body.contains("Rustible Notification"));
        assert!(body.contains("deploy.yml"));
        assert!(body.contains("FAILED"));
        assert!(body.contains("host1"));
        assert!(body.contains("Connection timeout"));
        assert!(body.contains("Generated by Rustible"));
    }

    #[test]
    fn test_build_email_message() {
        let config = create_test_config();
        let message = build_email_message(&config, "Test Subject", "Test body\nLine 2");

        assert!(message.contains("From: from@example.com"));
        assert!(message.contains("To: to@example.com"));
        assert!(message.contains("Subject: Test Subject"));
        assert!(message.contains("MIME-Version: 1.0"));
        assert!(message.contains("X-Mailer: Rustible"));
        assert!(message.contains("Test body"));
        assert!(message.ends_with("\r\n.\r\n"));
    }

    #[test]
    fn test_notifier_is_configured() {
        let config = create_test_config();
        let notifier = EmailNotifier {
            config,
            timeout: Duration::from_secs(30),
        };
        assert!(notifier.is_configured());

        let config = EmailConfig::default();
        let notifier = EmailNotifier {
            config,
            timeout: Duration::from_secs(30),
        };
        assert!(!notifier.is_configured());
    }
}
