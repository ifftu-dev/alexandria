use owo_colors::OwoColorize;

/// Print a success message with green checkmark
pub fn success(msg: &str) {
    eprintln!("  {} {}", "✓".green().bold(), msg);
}

/// Print a warning message with yellow exclamation
pub fn warning(msg: &str) {
    eprintln!("  {} {}", "!".yellow().bold(), msg);
}

/// Print an error message with red X
pub fn error(msg: &str) {
    eprintln!("  {} {}", "✗".red().bold(), msg);
}

/// Print an info message with cyan arrow
pub fn info(msg: &str) {
    eprintln!("  {} {}", "→".cyan().bold(), msg);
}

/// Print a header/section title
pub fn header(msg: &str) {
    eprintln!("\n  {}", msg.bold());
}

/// Print a faint/dimmed message
pub fn faint(msg: &str) {
    eprintln!("    {}", msg.dimmed());
}

/// Print a numbered step indicator
pub fn step(n: usize, total: usize, msg: &str) {
    eprintln!("  {} {}", format!("[{}/{}]", n, total).cyan().bold(), msg);
}

/// Print a key-value pair, aligned
pub fn kv(key: &str, value: &str) {
    eprintln!("  {:>16}  {}", key.dimmed(), value);
}

/// Print a blank line to stderr
pub fn blank() {
    eprintln!();
}

/// Print the Alexandria banner
pub fn banner() {
    let version = env!("CARGO_PKG_VERSION");
    eprintln!();
    eprintln!(
        "  {}  {}",
        "⬡ Alexandria".bold(),
        format!("v{}", version).dimmed()
    );
    eprintln!("  {}", "Developer CLI".dimmed());
    eprintln!();
}
