// src/config.rs
use clap::Parser;
use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Command-line arguments for the application.
#[derive(Parser, Debug, Deserialize, Default)]
#[clap(author, version, about, long_about = None)]
pub struct CliArgs {
    /// Run in listener mode (receive and print events)
    #[clap(short, long, help = "Run in listener mode (receive and print events)")]
    pub listen: bool,

    /// Enable web UI for monitoring changes
    #[clap(short, long, help = "Enable web UI for monitoring changes")]
    pub browse: bool,

    /// UDP multicast address (e.g., "239.0.0.1:9999")
    #[clap(
        short,
        long,
        value_parser,
        help = "UDP multicast address (e.g., \"239.0.0.1:9999\")"
    )]
    pub multicast: Option<String>,

    /// HTTP address for web UI (e.g., "0.0.0.0:8080")
    #[clap(
        short,
        long,
        value_parser,
        help = "HTTP address for web UI (e.g., \"0.0.0.0:8080\")"
    )]
    pub webaddr: Option<String>,

    /// Path to a configuration file (e.g., hirai.toml)
    #[clap(
        short,
        long,
        value_parser,
        help = "Path to a configuration file (e.g., hirai.toml)"
    )]
    pub config: Option<PathBuf>,

    /// One or more directories to monitor
    #[clap(help = "One or more directories to monitor")]
    pub folders: Vec<String>,

    // Removed direct env from clap attribute, will be handled by Figment or default logic
    /// Log level (e.g., trace, debug, info, warn, error)
    #[clap(
        long,
        value_parser,
        help = "Log level (e.g., trace, debug, info, warn, error)"
    )]
    pub log_level: Option<String>,
}

/// Configuration loaded from file, environment, or defaults.
#[derive(Deserialize, Serialize, Debug, Default)]
pub struct FileConfig {
    /// Folders to watch
    pub folders: Option<Vec<String>>,
    /// Multicast address
    pub multicast: Option<String>,
    /// Web address
    pub webaddr: Option<String>,
    /// Listener mode
    pub listen: Option<bool>,
    /// Web UI browsing
    pub browse: Option<bool>,
    /// Log level
    pub log_level: Option<String>,
}

/// Final application configuration after merging all sources.
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Whether to run in listener mode
    pub listen: bool,
    /// Whether to enable web UI
    pub browse: bool,
    /// UDP multicast address
    pub multicast_addr: String,
    /// HTTP address for web UI
    pub web_addr: String,
    /// Directories to monitor
    pub folders_to_watch: Vec<String>,
    /// Log level
    pub log_level: String,
}

impl AppConfig {
    /// Loads the application configuration by merging CLI, file, environment, and defaults.
    pub fn load() -> Result<Self, figment::Error> {
        let cli_args = CliArgs::parse();

        let config_file_path = cli_args
            .config
            .clone()
            .unwrap_or_else(|| PathBuf::from("hirai.toml"));

        // Default log level from environment variable HIRAI_LOG_LEVEL, then "info"
        let default_log_level =
            std::env::var("HIRAI_LOG_LEVEL").unwrap_or_else(|_| "info".to_string());

        let fig = Figment::new()
            .merge(Serialized::defaults(FileConfig {
                // These are the lowest precedence defaults
                multicast: Some("239.0.0.1:9999".to_string()),
                webaddr: Some("0.0.0.0:8080".to_string()),
                listen: Some(false),
                browse: Some(false),
                log_level: Some(default_log_level.clone()), // Use env-aware default
                folders: Some(vec![]),
            }))
            .merge(Toml::file(config_file_path).nested())
            .merge(Env::prefixed("HIRAI_").map(|key| key.as_str().replace("__", ".").into())); // For HIRAI_LOG_LEVEL, HIRAI_MULTICAST etc.

        // Merge CLI args, giving them higher precedence than file or env vars for these specific fields
        let mut final_fig = fig;
        if let Some(mc) = cli_args.multicast.clone() {
            // Clone to avoid moving from cli_args if needed later
            final_fig = final_fig.merge(Serialized::globals(FileConfig {
                multicast: Some(mc),
                ..Default::default()
            }));
        }
        if let Some(wa) = cli_args.webaddr.clone() {
            final_fig = final_fig.merge(Serialized::globals(FileConfig {
                webaddr: Some(wa),
                ..Default::default()
            }));
        }
        // For boolean flags, if they are present in CLI, they override.
        // The || logic later handles this correctly if CLI is true.
        // If CLI is false, it doesn't override a true from config/env unless explicitly set to false.
        // This logic is a bit complex. Let's simplify: CLI always wins for flags if specified.

        // Extract the config after merging defaults, file, and env
        let mut merged_config: FileConfig = final_fig.select("hirai").extract()?;

        // Now, apply CLI overrides explicitly
        if let Some(cli_ll) = cli_args.log_level {
            merged_config.log_level = Some(cli_ll);
        }
        if let Some(cli_mc) = cli_args.multicast {
            merged_config.multicast = Some(cli_mc);
        }
        if let Some(cli_wa) = cli_args.webaddr {
            merged_config.webaddr = Some(cli_wa);
        }
        // For boolean flags, CLI presence means true
        let final_listen = cli_args.listen || merged_config.listen.unwrap_or(false);
        let final_browse = cli_args.browse || merged_config.browse.unwrap_or(false);

        let folders_to_watch = if !cli_args.folders.is_empty() {
            cli_args.folders
        } else {
            merged_config.folders.unwrap_or_default()
        };

        Ok(AppConfig {
            listen: final_listen,
            browse: final_browse,
            multicast_addr: merged_config
                .multicast
                .unwrap_or_else(|| "239.0.0.1:9999".to_string()),
            web_addr: merged_config
                .webaddr
                .unwrap_or_else(|| "0.0.0.0:8080".to_string()),
            folders_to_watch,
            log_level: merged_config.log_level.unwrap_or(default_log_level),
        })
    }
}
