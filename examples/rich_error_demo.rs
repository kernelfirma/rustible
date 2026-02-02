//! Demo of rich error diagnostics
//!
//! Run with: cargo run --example rich_error_demo

use rustible::diagnostics::{
    connection_error, missing_required_arg_error, module_not_found_error,
    template_syntax_error, undefined_variable_error, yaml_syntax_error, RichDiagnostic, Span,
};

fn main() {
    let playbook_source = r#"---
- name: Configure web servers
  hosts: webservers
  become: true

  vars:
    http_port: 80

  tasks:
    - name: Print debug message
      debug:
        msg: "Port is {{ wrong_port }}"

    - name: Install package
      pacakge:
        name: nginx
        state: present
"#;

    println!("\n{}\n", "=".repeat(70));
    println!("DEMO 1: Undefined Variable Error");
    println!("{}\n", "=".repeat(70));

    let diag = undefined_variable_error(
        "playbook.yml",
        playbook_source,
        21,
        25,
        "wrong_port",
        &["http_port", "ansible_hostname", "inventory_hostname"],
    );
    diag.eprint_with_source(playbook_source);

    println!("\n{}\n", "=".repeat(70));
    println!("DEMO 2: Module Not Found Error");
    println!("{}\n", "=".repeat(70));

    let diag = module_not_found_error(
        "playbook.yml",
        "pacakge",
        Span::from_line_col(playbook_source, 24, 7, 7),
        &["package", "apt", "yum", "dnf", "pip"],
    );
    diag.eprint_with_source(playbook_source);

    println!("\n{}\n", "=".repeat(70));
    println!("DEMO 3: YAML Syntax Error");
    println!("{}\n", "=".repeat(70));

    let bad_yaml = r#"---
- name: Bad playbook
  hosts: all
  tasks:
    - name: Missing colon
      debug
        msg: "hello"
"#;

    let diag = yaml_syntax_error("bad_playbook.yml", bad_yaml, 6, 7, "expected ':' after key");
    diag.eprint_with_source(bad_yaml);

    println!("\n{}\n", "=".repeat(70));
    println!("DEMO 4: Custom Rich Diagnostic with Multiple Labels");
    println!("{}\n", "=".repeat(70));

    let source = r#"---
- name: Example
  hosts: "{{ target_hosts }}"
  vars:
    server_name: "{{ undefined_var }}"
  tasks:
    - name: Test
      debug:
        msg: "{{ another_undefined }}"
"#;

    let diag = RichDiagnostic::error(
        "multiple undefined variables",
        "example.yml",
        Span::from_line_col(source, 5, 21, 13),
    )
    .with_code("E0001")
    .with_label("first undefined variable")
    .with_secondary_label(
        Span::from_line_col(source, 3, 11, 16),
        "also undefined here",
    )
    .with_secondary_label(Span::from_line_col(source, 9, 15, 19), "and here")
    .with_note("Variables must be defined before use")
    .with_help("Define these variables in vars, group_vars, or host_vars");

    diag.eprint_with_source(source);

    // --- New demos for enhanced helper functions ---

    println!("\n{}\n", "=".repeat(70));
    println!("DEMO 5: YAML Tab Detection with Auto-Fix Suggestion");
    println!("{}\n", "=".repeat(70));

    let tab_yaml = "---\n- name: Tabbed playbook\n\thosts: all\n\ttasks:\n\t  - name: Do stuff\n";
    let diag = yaml_syntax_error("tabbed.yml", tab_yaml, 3, 1, "tab character found");
    diag.eprint_with_source(tab_yaml);

    println!("\n{}\n", "=".repeat(70));
    println!("DEMO 6: Template Unclosed Delimiter Detection");
    println!("{}\n", "=".repeat(70));

    let unclosed_tpl = "---\n- name: Bad template\n  debug:\n    msg: \"{{ hostname\"\n";
    let diag = template_syntax_error(
        "template_err.yml",
        unclosed_tpl,
        4,
        10,
        "unclosed expression delimiter",
    );
    diag.eprint_with_source(unclosed_tpl);

    println!("\n{}\n", "=".repeat(70));
    println!("DEMO 7: Connection Error with Pattern-Matched Suggestions");
    println!("{}\n", "=".repeat(70));

    let inv_source = "---\nall:\n  hosts:\n    web01:\n      ansible_host: 192.168.1.10\n";
    let diag = connection_error(
        "inventory.yml",
        "web01",
        "Connection refused",
        Span::from_line_col(inv_source, 4, 5, 5),
    );
    diag.eprint_with_source(inv_source);

    println!("\n{}\n", "=".repeat(70));
    println!("DEMO 8: Missing Required Argument with Suggestion");
    println!("{}\n", "=".repeat(70));

    let copy_source = "---\n- name: Copy files\n  hosts: all\n  tasks:\n    - name: Deploy config\n      copy:\n        dest: /etc/app.conf\n";
    let diag = missing_required_arg_error(
        "copy_task.yml",
        "copy",
        "src",
        Span::from_line_col(copy_source, 6, 7, 4),
    );
    diag.eprint_with_source(copy_source);

    println!();
}
