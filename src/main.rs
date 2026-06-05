//! FGO Helper — resource download + crypto + APK mod + script bundling.
//!
//! ```text
//! fgo-helper
//!   apk setup|build|clean
//!   res download|info|list <jp|cn|all>
//!   scripts txt-to-bundle|bundle-to-txt
//!   mappings download
//! ```

mod apk;
mod bundle;
mod config;
mod crypto;
mod error;
mod mappings;
mod res;
mod unity;
mod util;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "fgo-helper", version, about = "FGO resource & APK toolchain")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// APK modification pipeline (requires Java + apktool + apksigner)
    Apk {
        #[command(subcommand)]
        action: ApkAction,
    },
    /// Download & decrypt FGO game resources
    Res {
        #[command(subcommand)]
        action: ResAction,
    },
    /// Convert between .txt scripts and .script bundles
    Scripts {
        #[command(subcommand)]
        action: ScriptsAction,
    },
    /// Download Chaldea translation mappings
    Mappings {
        #[command(subcommand)]
        action: MappingsAction,
    },
}

// ── APK subcommands ──

#[derive(Subcommand)]
enum ApkAction {
    /// Extract XAPK + decompile + inject .so/smali
    Setup {
        /// Path to .xapk file
        #[arg(short, long)]
        xapk: String,
    },
    /// Rebuild APK + sign
    Build {
        /// Keystore password
        #[arg(long, env = "FGO_KEYSTORE_PASS", default_value = "android")]
        ks_pass: String,
        /// Key alias
        #[arg(long, env = "FGO_KEYSTORE_ALIAS", default_value = "fgo_mod")]
        ks_alias: String,
    },
    /// Clean build artifacts
    Clean,
}

// ── Res subcommands ──

#[derive(Subcommand)]
enum ResAction {
    /// Full pipeline: version → AssetStorage → download → decrypt → extract
    Download {
        /// Server: jp or cn
        server: String,
        /// Force re-download (ignore cache)
        #[arg(short, long)]
        force: bool,
        /// Skip script extraction (only download AssetStorage + bundles)
        #[arg(long)]
        no_scripts: bool,
    },
    /// Show version info only
    Info { server: String },
    /// List script assets from cached AssetStorage
    List { server: String },
}

// ── Scripts subcommands ──

#[derive(Subcommand)]
enum ScriptsAction {
    /// Pack .txt files into a .script binary bundle
    TxtToBundle {
        /// Input directory containing .txt files
        #[arg(short, long, default_value = ".")]
        input: String,
        /// Output .script file path
        #[arg(short, long, default_value = "bundle.script")]
        output: String,
    },
    /// Unpack a .script binary bundle to .txt files
    BundleToTxt {
        /// Input .script file path
        #[arg(short, long)]
        input: String,
        /// Output directory for .txt files
        #[arg(short, long, default_value = "scripts")]
        output: String,
    },
}

// ── Mappings subcommands ──

#[derive(Subcommand)]
enum MappingsAction {
    /// Download all 35 Chaldea translation JSON files
    Download {
        /// Output directory
        #[arg(short, long, default_value = "data/mappings")]
        output: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Apk { action } => match action {
            ApkAction::Setup { xapk } => apk::cmd_setup(std::path::Path::new(&xapk))?,
            ApkAction::Build { ks_pass, ks_alias } => apk::cmd_build(&ks_pass, &ks_alias)?,
            ApkAction::Clean => apk::cmd_clean()?,
        },
        Command::Res { action } => match action {
            ResAction::Download {
                server,
                force,
                no_scripts,
            } => res::cmd_download(&server, force, no_scripts).await?,
            ResAction::Info { server } => res::cmd_info(&server).await?,
            ResAction::List { server } => res::cmd_list(&server).await?,
        },
        Command::Scripts { action } => match action {
            ScriptsAction::TxtToBundle { input, output } => {
                bundle::cmd_txt_to_bundle(&input, &output)?
            }
            ScriptsAction::BundleToTxt { input, output } => {
                bundle::cmd_bundle_to_txt(&input, &output)?
            }
        },
        Command::Mappings { action } => match action {
            MappingsAction::Download { output } => mappings::cmd_download(&output).await?,
        },
    }

    Ok(())
}
