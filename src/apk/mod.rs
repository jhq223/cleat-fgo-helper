//! APK modification pipeline — orchestrates Java/Android toolchain.
//!
//! ## Prerequisites (must be in PATH)
//!
//! - `java`   (JRE/JDK ≥ 11)
//! - `apktool` (Apktool ≥ 2.9)
//! - `apksigner` (Android SDK build-tools)
//! - `keytool` (JDK)
//!
//! ## Phases
//!
//! A) Extract XAPK (ZIP) → build/xapk_out/main.apk + config.arm64_v8a.apk
//! B) `apktool d main.apk` → build/apktool_main/
//! C) Inject .so from config APK + resources/lib/
//! D) Smali string replacements via chaldea mappings
//! E) `apktool b` + `apksigner sign` → dist/fgo-mod.apk

use std::path::{Path, PathBuf};
use std::process::Command;

// ── Paths ──

fn root() -> PathBuf {
    PathBuf::from(".")
}
fn build_dir() -> PathBuf {
    root().join("build")
}
fn dist_dir() -> PathBuf {
    root().join("dist")
}
fn xapk_out() -> PathBuf {
    build_dir().join("xapk_out")
}
fn decompiled() -> PathBuf {
    build_dir().join("apktool_main")
}
fn lib_dir() -> PathBuf {
    root().join("resources").join("lib")
}
fn keystore_path() -> PathBuf {
    root()
        .join("resources")
        .join("keystore")
        .join("fgo_mod.keystore")
}

// ── Command helpers ──

/// Run an external tool. On Windows, wraps .bat/.cmd tools via `cmd /c`
/// because Rust's Command::new does NOT resolve PATHEXT for non-.exe files.
fn run(cmd: &str, args: &[&str]) -> anyhow::Result<()> {
    let output = if cfg!(windows) {
        let mut all_args: Vec<&str> = vec!["/c", cmd];
        all_args.extend_from_slice(args);
        Command::new("cmd")
            .args(&all_args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .map_err(|e| anyhow::anyhow!("cmd.exe: not found ({e})"))?
    } else {
        Command::new(cmd)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .map_err(|e| anyhow::anyhow!("{cmd}: not found in PATH ({e})"))?
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() { stdout } else { stderr };
        let code = output.status.code().unwrap_or(-1);
        anyhow::bail!(
            "{cmd} exited with code {code}\n── stderr ──\n{detail}── stderr ──"
        );
    }

    // Log stdout for debugging (truncated)
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.trim().is_empty() {
        let preview: String = stdout.lines().take(5).collect::<Vec<_>>().join("\n");
        if stdout.lines().count() > 5 {
            log::debug!("  [{cmd}] stdout (first 5 lines):\n{preview}\n  ...");
        } else {
            log::debug!("  [{cmd}] stdout:\n{preview}");
        }
    }

    Ok(())
}

fn run_apktool(args: &[&str]) -> anyhow::Result<()> {
    run("apktool", args)
}

// ── Phase A: Extract XAPK ──

fn phase_extract(xapk_path: &Path) -> anyhow::Result<(PathBuf, Option<PathBuf>)> {
    log::info!("[A] Extracting: {}", xapk_path.display());

    let out = xapk_out();
    if out.exists() {
        std::fs::remove_dir_all(&out)?;
    }
    std::fs::create_dir_all(&out)?;

    let file = std::fs::File::open(xapk_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        let out_path = out.join(&name);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out_file = std::fs::File::create(&out_path)?;
            std::io::copy(&mut entry, &mut out_file)?;
        }
    }

    // Find main APK and config APK
    let mut main_apk = None;
    let mut config_apk = None;

    for entry in std::fs::read_dir(&out)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_lowercase();
        let path = entry.path();

        if name.ends_with(".apk") {
            if name.contains("config") && name.contains("arm64") {
                config_apk = Some(path);
            } else if !name.contains("config") {
                main_apk = Some(path);
            }
        }
    }

    let main = main_apk.ok_or_else(|| anyhow::anyhow!("No main APK found in XAPK"))?;
    log::info!("  main APK: {}", main.display());

    if let Some(ref cfg) = config_apk {
        log::info!("  config APK: {}", cfg.display());
    }

    Ok((main, config_apk))
}

// ── Phase B: apktool decompile ──

fn phase_decompile(main_apk: &Path) -> anyhow::Result<()> {
    let out = decompiled();
    if out.exists() {
        std::fs::remove_dir_all(&out)?;
    }

    log::info!("[B] Decompiling: {}", main_apk.display());

    run_apktool(&[
        "d",
        "-f",
        "-o",
        out.to_str().unwrap(),
        main_apk.to_str().unwrap(),
    ])?;

    log::info!("  output: {}", out.display());

    // Remove split markers
    let manifest = out.join("AndroidManifest.xml");
    if manifest.exists() {
        let content = std::fs::read_to_string(&manifest)?;
        let cleaned = content
            .replace("android:isSplitRequired=\"true\"", "")
            .replace("android:isFeatureSplit=\"true\"", "");
        std::fs::write(&manifest, cleaned)?;
        log::info!("  cleaned split markers from AndroidManifest.xml");
    }

    Ok(())
}

// ── Phase C: Inject .so ──

fn phase_inject_so(config_apk: Option<&Path>) -> anyhow::Result<()> {
    log::info!("[C] Injecting .so files...");

    let target = decompiled().join("lib").join("arm64-v8a");
    std::fs::create_dir_all(&target)?;

    // 1. From config APK
    if let Some(cfg) = config_apk {
        let file = std::fs::File::open(cfg)?;
        let mut archive = zip::ZipArchive::new(file)?;

        let mut count = 0;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_string();
            if name.starts_with("lib/arm64-v8a/") && name.ends_with(".so") {
                let so_name = Path::new(&name).file_name().unwrap();
                let out_path = target.join(so_name);
                let mut out_file = std::fs::File::create(&out_path)?;
                std::io::copy(&mut entry, &mut out_file)?;
                log::info!("  config: {}", so_name.to_string_lossy());
                count += 1;
            }
        }
        log::info!("  {} .so from config APK", count);
    } else {
        log::info!("  (no config APK — using cached .so)");
    }

    // 2. Custom .so
    let custom = lib_dir();
    if custom.exists() {
        for entry in std::fs::read_dir(&custom)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "so") {
                let dst = target.join(path.file_name().unwrap());
                std::fs::copy(&path, &dst)?;
                log::info!(
                    "  custom: {} ({} KB)",
                    path.file_name().unwrap().to_string_lossy(),
                    path.metadata()?.len() / 1024
                );
            }
        }
    }

    Ok(())
}

// ── Phase D: Smali injection ──

fn phase_inject_smali(mappings_dir: Option<&Path>) -> anyhow::Result<()> {
    log::info!("[D] Injecting Smali patches...");

    let smali_dir = decompiled().join("smali");
    if !smali_dir.exists() {
        log::info!("  (no smali directory — skipping)");
        return Ok(());
    }

    let mappings = match mappings_dir {
        Some(dir) if dir.exists() => load_smali_mappings(dir)?,
        _ => {
            log::info!("  (no chaldea mappings — skipping string replacements)");
            return Ok(());
        }
    };

    if mappings.is_empty() {
        return Ok(());
    }

    log::info!("  Loaded {} string replacements", mappings.len());

    let mut replaced = 0u64;
    let mut files_touched = 0u64;

    for entry in walkdir::WalkDir::new(&smali_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "smali"))
    {
        let path = entry.path();
        let content = std::fs::read_to_string(path)?;
        let mut modified = content.clone();

        for (from, to) in &mappings {
            if modified.contains(from) {
                modified = modified.replace(from, to);
                replaced += 1;
            }
        }

        if modified != content {
            std::fs::write(path, &modified)?;
            files_touched += 1;
        }
    }

    log::info!("  {} replacements across {} files", replaced, files_touched);
    Ok(())
}

fn load_smali_mappings(dir: &Path) -> anyhow::Result<Vec<(String, String)>> {
    let mut mappings = Vec::new();

    let categories = [
        "svt_names",
        "ce_names",
        "skill_names",
        "skill_detail",
        "td_names",
        "td_detail",
        "item_names",
        "quest_names",
        "spot_names",
        "event_names",
        "war_names",
        "buff_names",
        "buff_detail",
        "costume_names",
        "mc_names",
    ];

    for cat in &categories {
        let path = dir.join(format!("{cat}.json"));
        if !path.exists() {
            continue;
        }
        let json = std::fs::read_to_string(&path)?;
        let map: serde_json::Value = serde_json::from_str(&json)?;
        if let Some(obj) = map.as_object() {
            for (jp_key, entry) in obj {
                if let Some(cn) = entry.get("CN").and_then(|v| v.as_str()) {
                    if jp_key != cn && !jp_key.is_empty() && !cn.is_empty() {
                        mappings.push((jp_key.clone(), cn.to_string()));
                    }
                }
            }
        }
    }

    Ok(mappings)
}

// ── Phase E: Rebuild + sign ──

fn phase_rebuild(keystore_pass: &str, alias: &str) -> anyhow::Result<()> {
    log::info!("[E] Rebuilding APK...");

    let apk = dist_dir().join("fgo-mod-unsigned.apk");
    std::fs::create_dir_all(dist_dir())?;
    let _ = std::fs::remove_file(&apk);

    run_apktool(&[
        "b",
        "-f",
        "-o",
        apk.to_str().unwrap(),
        decompiled().to_str().unwrap(),
    ])?;

    let size_kb = apk.metadata()?.len() / 1024;
    log::info!("  built: {} ({} KB)", apk.display(), size_kb);

    // Generate keystore if missing
    let ks = keystore_path();
    if !ks.exists() {
        generate_keystore(&ks, keystore_pass, alias)?;
    }

    // Sign
    let signed = dist_dir().join("fgo-mod.apk");
    let _ = std::fs::remove_file(&signed);

    run(
        "apksigner",
        &[
            "sign",
            "--ks",
            ks.to_str().unwrap(),
            "--ks-pass",
            &format!("pass:{keystore_pass}"),
            "--ks-key-alias",
            alias,
            "--out",
            signed.to_str().unwrap(),
            apk.to_str().unwrap(),
        ],
    )?;

    let final_size_mb = signed.metadata()?.len() / 1024 / 1024;
    log::info!("  signed: {} ({} MB)", signed.display(), final_size_mb);
    println!("\n✓ SUCCESS: {} ({} MB)", signed.display(), final_size_mb);

    Ok(())
}

fn generate_keystore(ks: &Path, pass: &str, alias: &str) -> anyhow::Result<()> {
    log::info!("  generating debug keystore...");
    if let Some(parent) = ks.parent() {
        std::fs::create_dir_all(parent)?;
    }

    run(
        "keytool",
        &[
            "-genkey",
            "-v",
            "-keystore",
            ks.to_str().unwrap(),
            "-alias",
            alias,
            "-keyalg",
            "RSA",
            "-keysize",
            "2048",
            "-validity",
            "10000",
            "-storepass",
            pass,
            "-keypass",
            pass,
            "-dname",
            "CN=FGO Mod, OU=Mod, O=Mod, L=Tokyo, S=Tokyo, C=JP",
        ],
    )
}

// ── Public commands ──

pub fn cmd_setup(xapk_path: &Path) -> anyhow::Result<()> {
    if !xapk_path.exists() {
        anyhow::bail!("XAPK not found: {}", xapk_path.display());
    }

    let (main_apk, config_apk) = phase_extract(xapk_path)?;
    phase_decompile(&main_apk)?;
    phase_inject_so(config_apk.as_deref())?;

    let mappings_dir = root().join("data").join("mappings");
    if mappings_dir.exists() {
        phase_inject_smali(Some(&mappings_dir))?;
    } else {
        phase_inject_smali(None)?;
    }

    println!("\n[SETUP DONE] Decompiled: {}", decompiled().display());
    println!("  Next: fgo-helper apk build");
    Ok(())
}

pub fn cmd_build(keystore_pass: &str, alias: &str) -> anyhow::Result<()> {
    if !decompiled().exists() {
        anyhow::bail!(
            "{} not found. Run 'fgo-helper apk setup' first.",
            decompiled().display()
        );
    }

    // Re-inject smali if mappings updated
    let mappings_dir = root().join("data").join("mappings");
    if mappings_dir.exists() {
        phase_inject_smali(Some(&mappings_dir))?;
    }

    phase_rebuild(keystore_pass, alias)?;
    Ok(())
}

pub fn cmd_clean() -> anyhow::Result<()> {
    for d in &[build_dir(), dist_dir()] {
        if d.exists() {
            std::fs::remove_dir_all(d)?;
            println!("Removed {}", d.display());
        }
    }
    println!("Clean complete.");
    Ok(())
}
