use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    bin_name = "wunder-desktop",
    about = "Run wunder in local desktop mode"
)]
pub struct DesktopArgs {
    /// Bind host for local desktop bridge.
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Bind port for local desktop bridge (0 = random free port).
    #[arg(long, default_value_t = 0)]
    pub port: u16,

    /// Workspace root. Defaults to <app_dir>/WUNDER_WORK.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Runtime temp root. Defaults to <app_dir>/WUNDER_TEMPD.
    #[arg(long)]
    pub temp_root: Option<PathBuf>,

    /// Logical user id in desktop mode.
    #[arg(long)]
    pub user: Option<String>,

    /// Print full desktop token in stdout.
    #[arg(long, default_value_t = false)]
    pub print_token: bool,
}
