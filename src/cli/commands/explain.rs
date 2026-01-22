//! Explain command - show detailed information about error codes
//!
//! Provides documentation for rustible error codes, similar to `rustc --explain`.
//!
//! # Usage
//!
//! ```bash
//! rustible explain E0001
//! rustible explain --list
//! ```

use anyhow::Result;
use colored::Colorize;
use rustible::diagnostics::ErrorCodeRegistry;

/// Run the explain command
pub fn run(code: Option<&str>, list: bool) -> Result<i32> {
    let registry = ErrorCodeRegistry::new();

    if list {
        print_all_codes(&registry);
        return Ok(0);
    }

    let Some(code) = code else {
        eprintln!("{}: no error code provided", "error".red().bold());
        eprintln!();
        eprintln!("Usage: rustible explain <ERROR_CODE>");
        eprintln!("       rustible explain --list");
        eprintln!();
        eprintln!("Example: rustible explain E0001");
        return Ok(1);
    };

    // Normalize error code (add E prefix if missing)
    let normalized_code = if code.starts_with('E') || code.starts_with('e') {
        code.to_uppercase()
    } else {
        format!("E{}", code.trim_start_matches('0'))
    };

    if let Some(info) = registry.get(&normalized_code) {
        print_error_explanation(info);
        Ok(0)
    } else {
        eprintln!("{}: unknown error code '{}'", "error".red().bold(), code);
        eprintln!();
        eprintln!("Use 'rustible explain --list' to see all error codes.");
        Ok(1)
    }
}

fn print_error_explanation(info: &rustible::diagnostics::ErrorCodeInfo) {
    println!();
    println!("{}: {}", info.code.cyan().bold(), info.title.bold());
    println!();
    println!("{}", info.explanation);
    println!();

    if !info.causes.is_empty() {
        println!("{}", "Common causes:".yellow().bold());
        for cause in &info.causes {
            println!("  {} {}", "•".dimmed(), cause);
        }
        println!();
    }

    if !info.fixes.is_empty() {
        println!("{}", "How to fix:".green().bold());
        for fix in &info.fixes {
            println!("  {} {}", "→".dimmed(), fix);
        }
        println!();
    }
}

fn print_all_codes(registry: &ErrorCodeRegistry) {
    println!();
    println!("{}", "Rustible Error Codes".bold().underline());
    println!();

    // Get all codes sorted
    let mut codes: Vec<_> = [
        "E0001", "E0002", "E0003", "E0004", "E0010", "E0020", "E0030",
    ]
    .into_iter()
    .filter_map(|code| registry.get(code).map(|info| (code, info)))
    .collect();

    codes.sort_by_key(|(code, _)| *code);

    println!("{:8} {}", "Code".cyan().bold(), "Description".bold());
    println!("{}", "─".repeat(60).dimmed());

    for (code, info) in codes {
        println!("{:8} {}", code.cyan(), info.title);
    }

    println!();
    println!(
        "Use '{}' for detailed explanation.",
        "rustible explain <CODE>".green()
    );
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_explain_known_code() {
        let result = run(Some("E0001"), false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn test_explain_unknown_code() {
        let result = run(Some("E9999"), false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1);
    }

    #[test]
    fn test_explain_no_code() {
        let result = run(None, false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1);
    }

    #[test]
    fn test_explain_list() {
        let result = run(None, true);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }
}
