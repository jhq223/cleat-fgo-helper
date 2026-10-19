//! FGO Helper — resource download + crypto + APK mod + script bundling.
//!
//! ```text
//! fgo-helper
//!   apk setup|build|clean
//!   res download|info|list <jp|cn|all>
//!   scripts txt-to-bundle|bundle-to-txt
//!   script export|import
//!   tools compare|deharmonize
//!   mappings download
//! ```

mod apk;
mod bundle;
mod config;
mod crypto;
mod error;
mod mappings;
mod release;
mod res;
mod scripts;
mod storage;
mod unity;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "fgo-helper", version, about = "FGO resource & APK toolchain")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Fetch, validate and package CN translations for the JP client
    Resources {
        #[command(subcommand)]
        action: ResourcesAction,
    },
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
    /// Export/import translatable text from story scripts
    Script {
        #[command(subcommand)]
        action: ScriptAction,
    },
    /// Compare and deharmonize script directories
    Tools {
        #[command(subcommand)]
        action: ToolsAction,
    },
    /// Download Chaldea translation mappings
    Mappings {
        #[command(subcommand)]
        action: MappingsAction,
    },
}

#[derive(Subcommand)]
enum ResourcesAction {
    Build {
        /// Reuse verified local snapshots without contacting game servers
        #[arg(long)]
        offline: bool,
        /// HTTPS download directory; {version} is replaced with the content version
        #[arg(
            long,
            default_value = "https://github.com/jhq223/cleat-fgo/releases/download/resources/"
        )]
        base_url: String,
        #[arg(short, long, default_value = "dist/resources")]
        output: String,
        /// Optional Unity AssetBundle containing FGO-Main-Font-Mod
        #[arg(long)]
        font: Option<String>,
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
        #[arg(long)]
        ks_pass: String,
        /// Key alias
        #[arg(long)]
        ks_alias: String,
        /// Keystore path
        #[arg(long, default_value = "resources/keystore/fgo_mod.keystore")]
        ks: String,
        /// Custom .so directory (default: resources/lib)
        #[arg(long, default_value = "resources/lib")]
        lib_dir: String,
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

// ── Script (translation) subcommands ──

#[derive(Subcommand)]
enum ScriptAction {
    /// Export dialogue and choice text to JSON for translation
    Export {
        /// Input scripts (defaults to the current verified JP snapshot)
        #[arg(short, long)]
        input: Option<String>,
        /// Output directory for .json files
        #[arg(short, long, default_value = "data/export")]
        output: String,
    },
    /// Import translated JSON back into .txt scripts
    Import {
        /// Input directory containing translated .json files
        #[arg(short, long, default_value = "data/export")]
        json_dir: String,
        /// Original script directory (for tag verification)
        #[arg(long)]
        original: Option<String>,
        /// Output directory for modified .txt files
        #[arg(short, long, default_value = "data/output")]
        output: String,
    },
    /// Merge translated JSON (message-only) into original exported JSON
    Merge {
        /// Directory containing translated .json files (message-only format)
        #[arg(short, long)]
        translated: String,
        /// Directory containing original exported .json files (full format with name/original/message)
        #[arg(short = 'r', long, default_value = "data/export")]
        original: String,
        /// Output directory for merged .json files
        #[arg(short, long)]
        output: String,
    },
}

// ── Tools subcommands ──

#[derive(Subcommand)]
enum ToolsAction {
    /// Copy JP-only scripts (missing from CN) to output directory
    Compare {
        /// JP scripts directory
        #[arg(short, long)]
        jp: Option<String>,
        /// CN scripts directory
        #[arg(short, long)]
        cn: Option<String>,
        /// Output directory for JP-only scripts
        #[arg(short, long, default_value = "data/jp_only")]
        output: String,
    },
    /// Apply anti-harmonization replacements to CN scripts
    Deharmonize {
        /// Input directory containing CN .txt scripts
        #[arg(short, long)]
        input: Option<String>,
        /// Output directory for deharmonized scripts
        #[arg(short, long, default_value = "data/cn_deharmonized")]
        output: String,
    },
    /// Generate character name mappings from Chaldea svt_names.json + td_names.json
    ScanNames {
        /// Mappings directory containing svt_names.json
        #[arg(short, long, default_value = "data/mappings")]
        mappings: String,
        /// Output JSON path
        #[arg(short, long, default_value = "names.json")]
        output: String,
    },
    /// Fill CN Chinese translations into JP LocalizationJpn.txt
    Localize {
        /// JP data directory (contains LocalizationJpn.txt)
        #[arg(short, long)]
        jp: Option<String>,
        /// CN data directory (contains LocalizationJpn1.txt)
        #[arg(short, long)]
        cn: Option<String>,
        /// Output path for merged LocalizationJpn.txt
        #[arg(short, long, default_value = "data/output/LocalizationJpn.txt")]
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
        Command::Resources { action } => match action {
            ResourcesAction::Build {
                offline,
                base_url,
                output,
                font,
            } => release::build(offline, &base_url, &output, font.as_deref()).await?,
        },
        Command::Apk { action } => match action {
            ApkAction::Setup { xapk } => apk::cmd_setup(std::path::Path::new(&xapk))?,
            ApkAction::Build {
                ks_pass,
                ks_alias,
                ks,
                lib_dir,
            } => apk::cmd_build(
                &ks_pass,
                &ks_alias,
                std::path::Path::new(&ks),
                std::path::Path::new(&lib_dir),
            )?,
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
        Command::Script { action } => match action {
            ScriptAction::Export { input, output } => {
                scripts::cmd_export(&snapshot_input(input, "jp", true)?, &output)?
            }
            ScriptAction::Import {
                json_dir,
                original,
                output,
            } => scripts::cmd_import(&json_dir, &snapshot_input(original, "jp", true)?, &output)?,
            ScriptAction::Merge {
                translated,
                original,
                output,
            } => scripts::cmd_merge(&translated, &original, &output)?,
        },
        Command::Tools { action } => match action {
            ToolsAction::Compare { jp, cn, output } => scripts::cmd_compare(
                &snapshot_input(jp, "jp", true)?,
                &snapshot_input(cn, "cn", true)?,
                &output,
            )?,
            ToolsAction::Deharmonize { input, output } => {
                scripts::cmd_deharmonize(&snapshot_input(input, "cn", true)?, &output)?
            }
            ToolsAction::ScanNames { mappings, output } => {
                scripts::cmd_scan_names(&mappings, &output)?
            }
            ToolsAction::Localize { jp, cn, output } => scripts::cmd_localize(
                &snapshot_input(jp, "jp", false)?,
                &snapshot_input(cn, "cn", false)?,
                &output,
            )?,
        },
    }

    Ok(())
}

fn snapshot_input(explicit: Option<String>, server: &str, scripts: bool) -> anyhow::Result<String> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    let mut path = res::snapshot::active(server)?;
    if scripts {
        path.push("scripts");
    }
    Ok(path.to_string_lossy().into_owned())
}
