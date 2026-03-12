use colored::Colorize;
use std::env;

/// Dependency name
pub const DEP_TMUX: &str = "tmux";
pub const DEP_ITMUX: &str = "itmux";

/// Check if a command exists
pub fn command_exists(cmd: &str) -> bool {
    which::which(cmd).is_ok()
}

/// Check all dependencies
pub fn check_deps() -> anyhow::Result<()> {
    println!("{}", "Checking SessionCast dependencies...\n".bold());

    let platform = Platform::detect();
    println!("Platform: {}\n", format!("{:?}", platform).cyan());

    let deps = get_required_deps();

    let mut all_ok = true;
    for dep in &deps {
        let installed = command_exists(dep.binary);
        let status = if installed {
            "✓ installed".green()
        } else {
            all_ok = false;
            "✗ not found".red()
        };
        println!("  {} {}", dep.name.bold(), status);
        if let Some(desc) = &dep.description {
            println!("    {}", desc.dimmed());
        }
    }

    println!();

    if all_ok {
        println!("{}", "All dependencies are installed!".green());
    } else {
        println!(
            "{}",
            "Run `sessioncast deps install` to install missing dependencies.".yellow()
        );
    }

    Ok(())
}

/// Install dependencies
pub fn install() -> anyhow::Result<()> {
    let platform = Platform::detect();
    let deps = get_required_deps();

    let missing: Vec<_> = deps.iter().filter(|d| !command_exists(d.binary)).collect();

    if missing.is_empty() {
        println!("{}", "All dependencies are already installed!".green());
        return Ok(());
    }

    println!("{}", "Installing missing dependencies...\n".bold());

    for dep in &missing {
        println!("Installing {}...", dep.name.cyan());
        install_dep(dep, &platform)?;
    }

    println!("\n{}", "Dependencies installed successfully!".green());

    Ok(())
}

fn install_dep(dep: &Dependency, platform: &Platform) -> anyhow::Result<()> {
    match platform {
        Platform::MacOS => {
            if dep.binary == DEP_TMUX {
                install_with_brew("tmux")?;
            }
        }
        Platform::Linux => {
            if dep.binary == DEP_TMUX {
                install_tmux_linux()?;
            }
        }
        Platform::Windows => {
            if dep.binary == DEP_ITMUX {
                install_itmux_windows()?;
            }
        }
        Platform::Unknown => {
            anyhow::bail!("Unknown platform. Please install dependencies manually.");
        }
    }

    Ok(())
}

// ============================================================================
// macOS Installation
// ============================================================================

fn install_with_brew(package: &str) -> anyhow::Result<()> {
    // Check if brew is installed
    if !command_exists("brew") {
        anyhow::bail!(
            "Homebrew is not installed. Please install it from https://brew.sh or install {} manually.",
            package
        );
    }

    println!("  Running: {}", format!("brew install {}", package).dimmed());

    let status = std::process::Command::new("brew")
        .args(["install", package])
        .status()?;

    if !status.success() {
        anyhow::bail!("Failed to install {} with Homebrew", package);
    }

    Ok(())
}

// ============================================================================
// Linux Installation
// ============================================================================

fn install_tmux_linux() -> anyhow::Result<()> {
    // Detect package manager
    let (pm, install_cmd) = if command_exists("apt-get") {
        ("apt", vec!["apt-get", "install", "-y", "tmux"])
    } else if command_exists("dnf") {
        ("dnf", vec!["install", "-y", "tmux"])
    } else if command_exists("yum") {
        ("yum", vec!["install", "-y", "tmux"])
    } else if command_exists("pacman") {
        ("pacman", vec!["-S", "--noconfirm", "tmux"])
    } else if command_exists("apk") {
        ("apk", vec!["add", "tmux"])
    } else {
        anyhow::bail!(
            "Could not detect package manager. Please install tmux manually:\n  \
             Debian/Ubuntu: sudo apt install tmux\n  \
             Fedora: sudo dnf install tmux\n  \
             Arch: sudo pacman -S tmux\n  \
             Alpine: apk add tmux"
        );
    };

    println!(
        "  Running: {}",
        format!("sudo {} install tmux", pm).dimmed()
    );

    // Try with sudo first
    let result = std::process::Command::new("sudo")
        .args(&install_cmd)
        .status();

    match result {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => anyhow::bail!("Failed to install tmux"),
        Err(_) => {
            // Try without sudo (might work in some containers)
            println!("  {} trying without sudo...", "sudo failed,".yellow());
            let status = std::process::Command::new(install_cmd[0])
                .args(&install_cmd[1..])
                .status()?;
            if status.success() {
                Ok(())
            } else {
                anyhow::bail!("Failed to install tmux")
            }
        }
    }
}

// ============================================================================
// Windows Installation (conditional compile)
// ============================================================================

#[cfg(windows)]
fn install_itmux_windows() -> anyhow::Result<()> {
    use std::fs;
    use std::io::{Read, Write};

    println!("\n  {}", "itmux Installation Guide".bold());
    println!(
        "  {}\n",
        "Windows requires itmux (Cygwin + tmux bundle) for tmux support.".dimmed()
    );

    // Default installation directory
    let install_dir = std::env::var("USERPROFILE")
        .map(|p| format!("{}\\itmux", p))
        .unwrap_or_else(|_| "C:\\itmux".to_string());

    // Check if already exists
    if std::path::Path::new(&install_dir).exists() {
        println!(
            "  {} Directory already exists: {}",
            "!".yellow(),
            install_dir
        );
        set_itmux_env(&install_dir);
        return Ok(());
    }

    // Try to download from GitHub
    println!("  {}", "Fetching latest release from GitHub...".cyan());

    let api_url = "https://api.github.com/repos/phayte/itmux/releases/latest";

    let response = ureq::AgentBuilder::new()
        .user_agent("sessioncast-cli")
        .build()
        .get(api_url)
        .call()
        .map_err(|e| anyhow::anyhow!("Failed to fetch release info: {}", e))?;

    if response.status() != 200 {
        show_manual_install_guide(&install_dir);
        anyhow::bail!("Manual installation required");
    }

    let body = response
        .into_string()
        .map_err(|e| anyhow::anyhow!("Failed to read response: {}", e))?;

    // Parse JSON to find download URL
    let zip_url = body
        .lines()
        .find(|line| line.contains("browser_download_url") && line.contains(".zip"))
        .and_then(|line| {
            let start = line.find("https://")?;
            let end = line.rfind('"')?;
            Some(line[start..end].to_string())
        });

    let Some(zip_url) = zip_url else {
        show_manual_install_guide(&install_dir);
        anyhow::bail!("Could not find download URL");
    };

    println!("  {} {}", "↓".cyan(), zip_url);

    // Download zip file
    let temp_dir = std::env::temp_dir();
    let zip_path = temp_dir.join("itmux.zip");

    println!("  {}", "Downloading...".cyan());

    let response = ureq::get(&zip_url)
        .call()
        .map_err(|e| anyhow::anyhow!("Failed to download: {}", e))?;

    let mut file = fs::File::create(&zip_path)
        .map_err(|e| anyhow::anyhow!("Failed to create temp file: {}", e))?;

    let mut buffer = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut buffer)
        .map_err(|e| anyhow::anyhow!("Failed to read download: {}", e))?;
    file.write_all(&buffer)
        .map_err(|e| anyhow::anyhow!("Failed to write temp file: {}", e))?;

    println!("  {}", "Extracting...".cyan());

    // Extract zip file
    extract_zip(&zip_path, &std::path::PathBuf::from(&install_dir))?;

    // Cleanup
    let _ = fs::remove_file(&zip_path);

    // Set environment variable
    set_itmux_env(&install_dir);

    println!(
        "\n  {} {}",
        "✓".green(),
        format!("itmux installed to: {}", install_dir).green()
    );

    Ok(())
}

#[cfg(windows)]
fn set_itmux_env(install_dir: &str) {
    let _ = std::process::Command::new("powershell")
        .args([
            "-Command",
            &format!(
                "[Environment]::SetEnvironmentVariable('ITMUX_HOME', '{}', 'User')",
                install_dir
            ),
        ])
        .status();
}

#[cfg(windows)]
fn show_manual_install_guide(install_dir: &str) {
    println!("\n  {} Could not auto-download itmux.", "!".yellow());
    println!("  Please install manually:\n");
    println!("    1. Visit: https://github.com/phayte/itmux/releases");
    println!("    2. Download the latest itmux-x.x.x.zip");
    println!("    3. Extract to: {}", install_dir);
    println!("    4. Set environment variable:");
    println!(
        "       [Environment]::SetEnvironmentVariable('ITMUX_HOME', '{}', 'User')\n",
        install_dir
    );
}

#[cfg(windows)]
fn extract_zip(zip_path: &std::path::Path, dest: &std::path::Path) -> anyhow::Result<()> {
    use std::fs;
    use std::io;

    fs::create_dir_all(dest)?;

    let file = fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| anyhow::anyhow!("Failed to open zip: {}", e))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| anyhow::anyhow!("Failed to read zip entry: {}", e))?;
        let outpath = match file.enclosed_name() {
            Some(path) => dest.join(path),
            None => continue,
        };

        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(p)?;
                }
            }
            let mut outfile = fs::File::create(&outpath)?;
            io::copy(&mut file, &mut outfile)?;
        }
    }

    Ok(())
}

// Non-windows stub
#[cfg(not(windows))]
fn install_itmux_windows() -> anyhow::Result<()> {
    anyhow::bail!("itmux is only needed on Windows");
}

// ============================================================================
// Platform Detection
// ============================================================================

#[derive(Debug, Clone)]
enum Platform {
    MacOS,
    Linux,
    Windows,
    Unknown,
}

impl Platform {
    fn detect() -> Self {
        match env::consts::OS {
            "macos" => Platform::MacOS,
            "linux" => Platform::Linux,
            "windows" => Platform::Windows,
            _ => Platform::Unknown,
        }
    }
}

#[derive(Debug, Clone)]
struct Dependency {
    name: &'static str,
    binary: &'static str,
    description: Option<&'static str>,
}

fn get_required_deps() -> Vec<Dependency> {
    match Platform::detect() {
        Platform::MacOS | Platform::Linux => vec![Dependency {
            name: "tmux",
            binary: DEP_TMUX,
            description: Some("Terminal multiplexer for session management"),
        }],
        Platform::Windows => vec![Dependency {
            name: "itmux",
            binary: DEP_ITMUX,
            description: Some("tmux-like terminal multiplexer for Windows"),
        }],
        Platform::Unknown => vec![],
    }
}
