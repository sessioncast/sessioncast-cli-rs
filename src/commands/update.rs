use colored::Colorize;
use self_update::cargo_crate_version;

/// Check for updates (without installing)
pub fn check_update() -> anyhow::Result<()> {
    println!("{}", "Checking for updates...\n".cyan());

    let current_version = env!("CARGO_PKG_VERSION");
    println!("Current version: {}", current_version.dimmed());

    // Check GitHub for latest release
    let releases = self_update::backends::github::ReleaseList::configure()
        .repo_owner("sessioncast")
        .repo_name("sessioncast-cli-rs")
        .build()?;

    let releases = releases.fetch()?;
    let latest = releases.first();

    match latest {
        Some(release) => {
            let latest_version = release.version.trim_start_matches('v');
            println!("Latest version:  {}", latest_version.dimmed());

            if latest_version == current_version {
                println!("\n{}", "You're already on the latest version!".green());
            } else {
                println!(
                    "\n{}",
                    format!("Update available: {} → {}", current_version, latest_version).yellow()
                );
                println!(
                    "Run {} to update.",
                    "sessioncast update".cyan()
                );
            }
        }
        None => {
            println!("{}", "Could not check for updates.".yellow());
            println!("Make sure you have internet access.");
        }
    }

    Ok(())
}

/// Update to the latest version
pub fn update() -> anyhow::Result<()> {
    println!("{}", "Updating SessionCast CLI...\n".cyan());

    let status = self_update::backends::github::Update::configure()
        .repo_owner("sessioncast")
        .repo_name("sessioncast-cli-rs")
        .bin_name("sessioncast")
        .show_download_progress(true)
        .current_version(cargo_crate_version!())
        .build()?
        .update()?;

    match status {
        self_update::Status::UpToDate(_) => {
            println!("\n{}", "You're already on the latest version!".green());
        }
        self_update::Status::Updated(_) => {
            println!("\n{}", "✓ SessionCast updated successfully!".green());
            println!("Run {} to see the new version.", "sessioncast --version".dimmed());
        }
    }

    Ok(())
}
