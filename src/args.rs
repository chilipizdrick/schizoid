#[derive(Debug, Clone, clap::Parser)]
pub struct Args {
    /// Path to the config file
    #[clap(short, long)]
    pub config_path: String,
}
