use clap::{Parser, Subcommand};
use colored::Colorize;
use sessioncast::{commands, config::AppConfig, tmux, utils};

/// SessionCast CLI - Control your agents from anywhere
#[derive(Parser)]
#[command(name = "sessioncast")]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Login to SessionCast (opens browser if no API key provided)
    Login {
        /// API key or agent token
        api_key: Option<String>,

        /// Custom API URL
        #[arg(short, long)]
        url: Option<String>,

        /// Custom Auth URL
        #[arg(short, long)]
        auth: Option<String>,
    },

    /// Clear stored credentials
    Logout,

    /// Check login status
    Status,

    /// List your agents
    Agents,

    /// List tmux sessions on agents
    List {
        /// Agent name
        agent: Option<String>,
    },

    /// Send keys to a tmux session
    Send {
        /// Target (agent:session or agent:session:window)
        target: String,

        /// Keys to send
        keys: String,

        /// Do not press Enter after keys
        #[arg(long)]
        no_enter: bool,
    },

    /// Execute a shell command in a tmux session
    Cmd {
        /// Command to execute
        command: String,

        /// Target tmux session (creates new if not specified)
        #[arg(short, long)]
        session: Option<String>,

        /// Working directory for new session
        #[arg(short = 'C', long)]
        cwd: Option<String>,

        /// Run in detached mode (don't attach to session)
        #[arg(short = 'd', long)]
        detach: bool,

        /// Create new window in existing session
        #[arg(short = 'w', long)]
        new_window: bool,
    },

    /// Start the SessionCast agent
    Agent {
        /// Path to config file
        #[arg(short, long)]
        config: Option<String>,

        /// Enable debug logging
        #[arg(short, long)]
        debug: bool,
    },

    /// Stream a local web service via headless Chrome
    Tunnel {
        /// Port number or URL
        target: String,

        /// Path to config file
        #[arg(short, long)]
        config: Option<String>,

        /// Enable debug logging
        #[arg(short, long)]
        debug: bool,

        /// Viewport width
        #[arg(short = 'W', long, default_value = "1280")]
        width: u32,

        /// Viewport height
        #[arg(short = 'H', long, default_value = "720")]
        height: u32,

        /// Path to Chrome binary
        #[arg(long)]
        chrome_path: Option<String>,

        /// Chrome DevTools Protocol port
        #[arg(long, default_value = "9222")]
        cdp_port: u16,
    },

    /// Check and install dependencies (tmux/itmux)
    Deps {
        #[command(subcommand)]
        action: Option<DepsAction>,
    },

    /// Update SessionCast CLI to the latest version
    Update {
        /// Only check for updates, don't install
        #[arg(short, long)]
        check: bool,
    },
}

#[derive(Subcommand)]
enum DepsAction {
    /// Check dependency status (default)
    Check,

    /// Install missing dependencies
    Install,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();
    let config = AppConfig::load().ok();

    // Handle no command (show welcome or help)
    let Some(command) = cli.command else {
        if !config.as_ref().map(|c| c.has_seen_welcome()).unwrap_or(false) {
            utils::show_welcome();
            if let Ok(mut c) = AppConfig::load() {
                c.set_seen_welcome();
                c.save()?;
            }
            return Ok(());
        }

        println!("{}", "\n  SessionCast CLI\n".bold());
        println!("  Control your agents from anywhere.\n");
        println!("  Run `sessioncast --help` for usage.\n");
        return Ok(());
    };

    // Handle commands
    match command {
        Commands::Login { api_key, url, auth } => {
            commands::login(api_key.as_deref(), url.as_deref(), auth.as_deref()).await?;
        }

        Commands::Logout => {
            commands::logout()?;
        }

        Commands::Status => {
            commands::status()?;
        }

        Commands::Agents => {
            println!("{}", "Listing agents...".dimmed());
            // TODO: Implement agents listing
        }

        Commands::List { agent } => {
            let sessions = tmux::list_sessions();
            if sessions.is_empty() {
                println!("{}", "No tmux sessions found".yellow());
            } else {
                println!("{}", "Tmux sessions:".green());
                for session in sessions {
                    let name = if let Some(a) = &agent {
                        format!("{}:{}", a, session.name)
                    } else {
                        session.name
                    };
                    println!("  {}", name);
                }
            }
        }

        Commands::Send {
            target,
            keys,
            no_enter,
        } => {
            let enter = !no_enter;
            if tmux::send_keys(&target, &keys, enter) {
                println!("{}", "Keys sent".green());
            } else {
                println!("{}", "Failed to send keys".red());
            }
        }

        Commands::Cmd {
            command,
            session,
            cwd,
            detach,
            new_window,
        } => {
            commands::exec(&command, session.as_deref(), cwd.as_deref(), detach, new_window)?;
        }

        Commands::Agent { config, debug } => {
            commands::run_agent(config.as_deref(), debug).await?;
        }

        Commands::Tunnel {
            target,
            config: _,
            debug: _,
            width: _,
            height: _,
            chrome_path: _,
            cdp_port: _,
        } => {
            let url = if target.parse::<u16>().is_ok() {
                format!("http://localhost:{}", target)
            } else {
                target
            };
            println!("{}", "Tunnel command not yet implemented".yellow());
            println!("URL: {}", url);
        }

        Commands::Deps { action } => {
            match action {
                None | Some(DepsAction::Check) => {
                    commands::check_deps()?;
                }
                Some(DepsAction::Install) => {
                    commands::install()?;
                }
            }
        }

        Commands::Update { check } => {
            if check {
                commands::check_update()?;
            } else {
                commands::update()?;
            }
        }
    }

    Ok(())
}
