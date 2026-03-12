// Utility functions
use colored::Colorize;

pub fn show_welcome() {
    println!();
    println!("{}", "✓ SessionCast CLI installed".green().bold());
    println!();

    let has_tmux = crate::tmux::is_available();

    if !has_tmux {
        println!("{}", "⚠ tmux not found".yellow());
        #[cfg(target_os = "macos")]
        println!("{}", "  Install: brew install tmux".dimmed());
        #[cfg(not(any(target_os = "macos", windows)))]
        println!("{}", "  Install: sudo apt install tmux".dimmed());
        println!();
    }

    println!("{}", "Quick Start:".bold());
    println!("  {}   {}", "sessioncast login".cyan(), "# Login via browser".dimmed());
    println!("  {}   {}", "sessioncast agent".cyan(), "# Start streaming".dimmed());
    println!();
    println!("{}", "Web Console: https://app.sessioncast.io".dimmed());
    println!();
}
