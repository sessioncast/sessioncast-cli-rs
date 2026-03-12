use colored::Colorize;

/// Execute a shell command in a tmux session
pub fn exec(
    command: &str,
    session: Option<&str>,
    working_dir: Option<&str>,
    detach: bool,
    new_window: bool,
) -> anyhow::Result<()> {
    let executor = crate::tmux::create_executor();

    // Determine session name
    let session_name = if let Some(s) = session {
        s.to_string()
    } else {
        // Generate random session name
        generate_random_session_name()
    };

    // Check if session exists
    let existing_sessions = executor.list_sessions();
    let session_exists = existing_sessions.contains(&session_name);

    if session_exists {
        if new_window {
            // Create new window in existing session
            let window_name = generate_window_name();
            create_window(&session_name, &window_name)?;
            send_command_to_window(&session_name, &window_name, command, detach);
        } else {
            // Use existing session
            executor.send_keys(&session_name, command);
            executor.send_special_key(&session_name, "Enter");
        }
        println!(
            "{}",
            format!("✓ Command sent to session: {}", session_name).green()
        );
    } else {
        // Create new session with command
        let success = if let Some(cwd) = working_dir {
            create_session_with_command(&session_name, command, Some(cwd), detach)
        } else {
            create_session_with_command(&session_name, command, None, detach)
        };

        if success {
            if detach {
                println!(
                    "{}",
                    format!("✓ Session '{}' created and command running (detached)", session_name).green()
                );
            } else {
                println!(
                    "{}",
                    format!("✓ Session '{}' created with command", session_name).green()
                );
            }
        } else {
            anyhow::bail!("Failed to create session");
        }
    }

    // Show session info
    if !detach && session_exists {
        println!(
            "{}",
            format!("  Run `tmux attach -t {}` to attach", session_name).dimmed()
        );
    }

    Ok(())
}

fn generate_random_session_name() -> String {
    use rand::Rng;
    let suffix: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(6)
        .map(char::from)
        .collect();
    format!("sc-{}", suffix.to_lowercase())
}

fn generate_window_name() -> String {
    use rand::Rng;
    let suffix: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(4)
        .map(char::from)
        .collect();
    format!("cmd-{}", suffix.to_lowercase())
}

fn create_window(session: &str, window_name: &str) -> anyhow::Result<()> {
    let output = std::process::Command::new("tmux")
        .args(["new-window", "-t", session, "-n", window_name])
        .output()?;

    if !output.status.success() {
        anyhow::bail!("Failed to create window: {}", String::from_utf8_lossy(&output.stderr));
    }

    Ok(())
}

fn send_command_to_window(session: &str, window: &str, command: &str, _detach: bool) {
    let target = format!("{}:{}", session, window);

    // Send command
    let _ = std::process::Command::new("tmux")
        .args(["send-keys", "-t", &target, "-l", command])
        .output();

    // Press Enter
    let _ = std::process::Command::new("tmux")
        .args(["send-keys", "-t", &target, "Enter"])
        .output();
}

fn create_session_with_command(
    session_name: &str,
    command: &str,
    working_dir: Option<&str>,
    detach: bool,
) -> bool {
    // Build tmux command
    let mut args = vec!["new-session"];

    if detach {
        args.push("-d");
    }

    args.push("-s");
    args.push(session_name);

    if let Some(cwd) = working_dir {
        args.push("-c");
        args.push(cwd);
    }

    // Create session
    let output = std::process::Command::new("tmux")
        .args(&args)
        .output();

    match output {
        Ok(o) if o.status.success() => {
            // Send the command
            let _ = std::process::Command::new("tmux")
                .args(["send-keys", "-t", session_name, "-l", command])
                .output();

            let _ = std::process::Command::new("tmux")
                .args(["send-keys", "-t", session_name, "Enter"])
                .output();

            true
        }
        _ => false,
    }
}

/// List running commands in sessions
pub fn list_commands(session: Option<&str>) -> anyhow::Result<()> {
    let executor = crate::tmux::create_executor();

    let sessions = if let Some(s) = session {
        vec![s.to_string()]
    } else {
        executor.list_sessions()
    };

    if sessions.is_empty() {
        println!("{}", "No tmux sessions found".yellow());
        return Ok(());
    }

    println!("{}", "Running sessions:".green());
    for s in sessions {
        // Get pane info
        if let Some(panes) = executor.list_panes(&s) {
            for pane in panes {
                let cwd = executor.get_pane_cwd(&s, Some(&pane.id));
                println!(
                    "  {} {} {}",
                    s.cyan(),
                    format!("({})", pane.id).dimmed(),
                    cwd.map(|p| format!("[{}]", p)).unwrap_or_default().dimmed()
                );
            }
        } else {
            println!("  {}", s.cyan());
        }
    }

    Ok(())
}
