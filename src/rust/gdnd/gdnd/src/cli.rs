//! CLI argument parsing for GDND

use std::path::PathBuf;

use clap::Parser;

/// GPU Dead Node Detector - Kubernetes GPU health monitoring and fault isolation
#[derive(Debug, Parser)]
#[command(name = "gdnd")]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Path to configuration file
    #[arg(short, long, default_value = "/etc/gdnd/config.yaml")]
    pub config: PathBuf,

    /// Node name (overrides config and NODE_NAME env)
    #[arg(long, env = "NODE_NAME")]
    pub node_name: Option<String>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info", env = "GDND_LOG_LEVEL")]
    pub log_level: String,

    /// Output logs in JSON format
    #[arg(long, default_value = "false", env = "GDND_LOG_JSON")]
    pub log_json: bool,

    /// Dry run mode - log actions but don't execute
    #[arg(long, default_value = "false")]
    pub dry_run: bool,

    /// Run a single detection pass and exit
    #[arg(long)]
    pub once: bool,

    /// Enable debug endpoints
    #[arg(long, default_value = "false")]
    pub debug: bool,
}

impl Cli {
    /// Parse CLI arguments
    pub fn parse_args() -> Self {
        Self::parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_defaults() {
        let cli = Cli::try_parse_from(["gdnd"]).unwrap();
        assert_eq!(cli.config.to_str().unwrap(), "/etc/gdnd/config.yaml");
        assert_eq!(cli.log_level, "info");
        assert!(!cli.log_json);
        assert!(!cli.dry_run);
        assert!(!cli.once);
    }

    #[test]
    fn test_cli_custom_config() {
        let cli = Cli::try_parse_from(["gdnd", "-c", "/custom/config.yaml"]).unwrap();
        assert_eq!(cli.config.to_str().unwrap(), "/custom/config.yaml");
    }

    #[test]
    fn test_cli_dry_run() {
        let cli = Cli::try_parse_from(["gdnd", "--dry-run"]).unwrap();
        assert!(cli.dry_run);
    }
}
