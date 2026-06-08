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
//! D) Smali injection: loadLibrary → UnityPlayerActivity
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
        let detail = if stderr.trim().is_empty() {
            stdout
        } else {
            stderr
        };
        let code = output.status.code().unwrap_or(-1);
        anyhow::bail!("{cmd} exited with code {code}\n── stderr ──\n{detail}── stderr ──");
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

        let re_split_types =
            regex::Regex::new(r#"\s*android:(?:requiredSplitTypes|splitTypes)="[^"]*""#)?;
        let re_splits_required = regex::Regex::new(
            r#"<meta-data\s+android:name="com\.android\.vending\.splits\.required"\s+android:value="true"\s*/>"#,
        )?;
        let re_splits_res = regex::Regex::new(
            r#"<meta-data\s+android:name="com\.android\.vending\.splits"\s+android:resource="@xml/splits\d+"\s*/>"#,
        )?;
        let re_derived_apk = regex::Regex::new(
            r#"<meta-data\s+android:name="com\.android\.vending\.derived\.apk\.id"\s+android:value="\d+"\s*/>"#,
        )?;

        let cleaned = re_split_types.replace_all(&content, "");
        let cleaned = re_splits_required.replace_all(&cleaned,
            r#"<meta-data android:name="com.android.vending.splits.required" android:value="false"/>"#);
        let cleaned = re_splits_res.replace_all(&cleaned, "");
        let cleaned = re_derived_apk.replace_all(&cleaned, "");

        if cleaned != content {
            std::fs::write(&manifest, cleaned.as_bytes())?;
            log::info!("  cleaned split markers from AndroidManifest.xml");
        }
    }

    Ok(())
}

// ── Phase C: Inject .so ──

fn phase_inject_so(config_apk: Option<&Path>, custom_lib_dir: Option<&Path>) -> anyhow::Result<()> {
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
    if let Some(ref custom) = custom_lib_dir
        && custom.exists()
    {
        for entry in std::fs::read_dir(custom)? {
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

fn phase_inject_smali() -> anyhow::Result<()> {
    log::info!("[D] Injecting Smali patches...");

    // Find UnityPlayerActivity.smali (may be under smali/ or smali_classesN/)
    let target = walkdir::WalkDir::new(decompiled())
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().is_some_and(|ext| ext == "smali")
                && e.path()
                    .to_string_lossy()
                    .replace('\\', "/")
                    .contains("com/unity3d/player/UnityPlayerActivity")
        })
        .map(|e| e.path().to_path_buf())
        .next()
        .ok_or_else(|| anyhow::anyhow!("UnityPlayerActivity.smali not found"))?;

    log::info!("  target: {}", target.display());

    let text = std::fs::read_to_string(&target)?;

    // Skip if already injected
    if text.contains(r#"const-string v0, "cleat_fgo""#) {
        log::info!("  (already injected — skipping)");
        return Ok(());
    }

    // Inject loadLibrary right after super.onCreate()
    let needle = "invoke-super {p0, p1}, Landroid/app/Activity;->onCreate(Landroid/os/Bundle;)V";
    if let Some(pos) = text.find(needle) {
        let end = pos + needle.len();
        let patch = concat!(
            "\n\n",
            "    const-string v0, \"cleat_fgo\"\n",
            "    invoke-static {v0}, Ljava/lang/System;->loadLibrary(Ljava/lang/String;)V\n",
        );
        let modified = format!("{}{}{}", &text[..end], patch, &text[end..]);
        std::fs::write(&target, &modified)?;
        log::info!("  injected loadLibrary");
    } else {
        log::warn!("  super.onCreate() not found in UnityPlayerActivity");
    }

    Ok(())
}

// ── Phase E: Rebuild + sign ──

fn phase_rebuild(ks: &Path, keystore_pass: &str, alias: &str) -> anyhow::Result<()> {
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

    // Keystore must already exist — user creates it manually
    if !ks.exists() {
        anyhow::bail!("Keystore not found: {}", ks.display(),);
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
    log::info!("✓ SUCCESS: {} ({} MB)", signed.display(), final_size_mb);

    Ok(())
}

// ── Public commands ──

pub fn cmd_setup(xapk_path: &Path) -> anyhow::Result<()> {
    if !xapk_path.exists() {
        anyhow::bail!("XAPK not found: {}", xapk_path.display());
    }

    let (main_apk, config_apk) = phase_extract(xapk_path)?;
    phase_decompile(&main_apk)?;
    phase_inject_so(config_apk.as_deref(), None)?;
    phase_inject_smali()?;

    log::info!("[SETUP DONE] Decompiled: {}", decompiled().display());
    log::info!("  Next: fgo-helper apk build");
    Ok(())
}

pub fn cmd_build(
    keystore_pass: &str,
    alias: &str,
    ks: &Path,
    lib_dir: &Path,
) -> anyhow::Result<()> {
    if !decompiled().exists() {
        anyhow::bail!(
            "{} not found. Run 'fgo-helper apk setup' first.",
            decompiled().display()
        );
    }

    // Inject custom .so (config .so already injected by setup)
    phase_inject_so(None, Some(lib_dir))?;

    // Re-inject smali (idempotent: skips if already injected)
    phase_inject_smali()?;

    phase_rebuild(ks, keystore_pass, alias)?;
    Ok(())
}

pub fn cmd_clean() -> anyhow::Result<()> {
    for d in &[build_dir(), dist_dir()] {
        if d.exists() {
            std::fs::remove_dir_all(d)?;
            log::info!("Removed {}", d.display());
        }
    }
    log::info!("Clean complete.");
    Ok(())
}
