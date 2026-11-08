//! Self-update from GitHub releases. The native Studio ships under its own
//! tags (`studio-native-v*`, `.github/workflows/studio-native-release.yml`),
//! so neither the launcher nor the Electron Studio (`studio-v*`) sees its
//! releases. Each release carries one zip per platform
//! (`Tempest-Studio-Native-<version>-<os>-<arch>.zip`, made by
//! `scripts/bundle.sh`) with its `<zip>.sha256`. An update downloads the zip next to
//! the install, checks its hash, unpacks it there, moves the running copy
//! aside (a running program can be renamed on every platform, not
//! overwritten on Windows) and puts the new one in its place; a small shell
//! waits for this process to exit, deletes the old copy and starts the new.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

const REPO: &str = "LowRezStudio/Tempest";
pub const PREFIX: &str = "studio-native-v";
/// In the folder of a bundled build on Windows and Linux (the macOS build is a `.app`).
pub const MARKER: &str = ".tempest-studio-bundle";

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub page: String,
    /// The zip for this platform (name, URL) and its `.sha256`, when the release has them.
    pub asset: Option<(String, String)>,
    pub sums: Option<String>,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// `macos-arm64`, `windows-x64`, `linux-x64`: the end of the zip's name.
pub fn platform() -> String {
    let os = if cfg!(target_os = "macos") { "macos" } else if cfg!(windows) { "windows" } else { "linux" };
    let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" };
    format!("{os}-{arch}")
}

/// Whether `a` is a newer `x.y.z[-pre]` than `b`; a pre-release sorts below its release.
pub fn is_newer(a: &str, b: &str) -> bool {
    match (semver::Version::parse(a), semver::Version::parse(b)) {
        (Ok(a), Ok(b)) => a > b,
        _ => false,
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(15))).user_agent(concat!("tempest-studio-native/", env!("CARGO_PKG_VERSION"))).build().into()
}

/// The newest published native Studio release, if newer than this build.
pub fn check() -> Result<Option<Release>, String> {
    // A stand-in for GitHub (testing): a URL answering the same JSON.
    let url = std::env::var("TEMPEST_STUDIO_RELEASES_URL").unwrap_or_else(|_| format!("https://api.github.com/repos/{REPO}/releases?per_page=40"));
    let mut resp = agent()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("could not reach GitHub: {e}"))?;
    let releases: Vec<GhRelease> = resp.body_mut().read_json().map_err(|e| format!("GitHub answered something unexpected: {e}"))?;
    let best = releases
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| Some((r.tag_name.strip_prefix(PREFIX)?.to_string(), r)))
        .filter(|(v, _)| semver::Version::parse(v).is_ok())
        .max_by(|(a, _), (b, _)| semver::Version::parse(a).unwrap().cmp(&semver::Version::parse(b).unwrap()));
    let Some((version, r)) = best else { return Ok(None) };
    if !is_newer(&version, current()) {
        return Ok(None);
    }
    let suffix = format!("-{}.zip", platform());
    let asset = r.assets.iter().find(|a| a.name.ends_with(&suffix)).map(|a| (a.name.clone(), a.browser_download_url.clone()));
    let sums = asset.as_ref().and_then(|(name, _)| r.assets.iter().find(|a| a.name == format!("{name}.sha256"))).map(|a| a.browser_download_url.clone());
    Ok(Some(Release { version, notes: r.body.unwrap_or_default().trim().to_string(), page: r.html_url, asset, sums }))
}

/// What an update replaces: the `.app` on macOS, else the files of the bundle's folder.
#[derive(Clone, Debug)]
pub enum Install {
    App(PathBuf),
    Folder(PathBuf),
}

impl Install {
    /// The folder the download and the new copy go to (same volume, so they can be renamed in).
    fn parent(&self) -> &Path {
        match self {
            Install::App(app) => app.parent().unwrap_or(Path::new("/")),
            Install::Folder(dir) => dir,
        }
    }
}

/// Where this build is installed, if it is a bundle the updater can replace:
/// a development build (`target/…`) is not one.
pub fn install() -> Option<Install> {
    static INSTALL: std::sync::OnceLock<Option<Install>> = std::sync::OnceLock::new();
    INSTALL.get_or_init(find_install).clone()
}

/// Whether this copy can replace itself: a bundle in a folder this user may write.
pub fn can_update() -> bool {
    static CAN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CAN.get_or_init(|| install().is_some_and(|i| writable(&i)))
}

fn find_install() -> Option<Install> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let dir = exe.parent()?;
    if cfg!(target_os = "macos") {
        let app = dir.parent()?.parent()?;
        (app.extension()? == "app").then(|| Install::App(app.to_path_buf()))
    } else {
        dir.join(MARKER).exists().then(|| Install::Folder(dir.to_path_buf()))
    }
}

/// Whether this process may write where the update goes.
fn writable(install: &Install) -> bool {
    let probe = install.parent().join(format!(".tempest-studio-write-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// A downloaded and unpacked update, ready to swap in.
#[derive(Debug)]
pub struct Staged {
    install: Install,
    /// The unpacked bundle: the new `.app`, or the folder holding the new files.
    root: PathBuf,
    /// What to delete once this process has exited.
    staging: PathBuf,
}

/// Download, check and unpack `release` beside the install; `progress` gets
/// the fraction downloaded.
pub fn download(release: &Release, install: &Install, progress: impl Fn(f32)) -> Result<Staged, String> {
    let (name, url) = release.asset.clone().ok_or("this release has no download for this platform")?;
    let expected = match &release.sums {
        Some(sums) => {
            let text = agent().get(sums).call().map_err(|e| format!("could not download the checksums: {e}"))?.body_mut().read_to_string().map_err(|e| e.to_string())?;
            Some(sha256_of(&text, &name).ok_or_else(|| format!("{name} is not in its checksum file"))?)
        }
        None => None,
    };
    let staging = install.parent().join(format!(".tempest-studio-update-{}", release.version));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| format!("could not write next to the Studio: {e}"))?;
    let zip = staging.join(&name);
    let mut resp = agent().get(&url).config().timeout_global(None).build().call().map_err(|e| format!("could not download {name}: {e}"))?;
    let total = resp.body().content_length().unwrap_or(0);
    let mut reader = resp.body_mut().as_reader();
    let mut file = std::fs::File::create(&zip).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let (mut buf, mut done) = (vec![0u8; 1 << 16], 0u64);
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("the download stopped: {e}"))?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if total > 0 {
            progress(done as f32 / total as f32);
        }
    }
    drop(file);
    let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if let Some(expected) = expected {
        if got != expected {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("{name} is damaged (its checksum does not match)"));
        }
    }
    let out = staging.join("new");
    unpack(&zip, &out)?;
    let _ = std::fs::remove_file(&zip);
    let root = match install {
        Install::App(app) => {
            let new = out.join(app.file_name().unwrap());
            if new.is_dir() {
                new
            } else {
                // The app was renamed by the user: take the one `.app` of the zip.
                std::fs::read_dir(&out).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "app")).ok_or("the download holds no app")?
            }
        }
        Install::Folder(_) => {
            // The zip holds one folder with the files.
            let mut dirs = std::fs::read_dir(&out).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).filter(|p| p.is_dir());
            let dir = dirs.next().ok_or("the download holds no files")?;
            if !dir.join(MARKER).exists() {
                return Err("the download is not a Studio bundle".into());
            }
            dir
        }
    };
    Ok(Staged { install: install.clone(), root, staging })
}

/// The hash a `sha256sum` line gives `name` (`<hash>  <name>`, or a bare hash).
fn sha256_of(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let hash = it.next()?;
        let file = it.next().map(|f| f.trim_start_matches('*'));
        (hash.len() == 64 && file.is_none_or(|f| f == name)).then(|| hash.to_ascii_lowercase())
    })
}

fn unpack(zip: &Path, out: &Path) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        // ditto keeps what an app bundle needs (permissions, signatures, extended attributes).
        let st = std::process::Command::new("/usr/bin/ditto").arg("-x").arg("-k").arg(zip).arg(out).status().map_err(|e| e.to_string())?;
        return if st.success() { Ok(()) } else { Err("could not unpack the download".into()) };
    }
    let file = std::fs::File::open(zip).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("the download is not a zip: {e}"))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else { continue };
        let path = out.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let mut f = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode & 0o777));
        }
    }
    Ok(())
}

/// Put the new copy in place of this one and start a helper that, once this
/// process has exited, deletes the old copy and starts the new one. The caller quits next.
pub fn apply(staged: Staged) -> Result<(), String> {
    let old = staged.staging.join("old");
    std::fs::create_dir_all(&old).map_err(|e| e.to_string())?;
    let exe = match &staged.install {
        Install::App(app) => {
            let aside = old.join(app.file_name().unwrap());
            std::fs::rename(app, &aside).map_err(|e| format!("could not move the Studio aside: {e}"))?;
            if let Err(e) = std::fs::rename(&staged.root, app) {
                let _ = std::fs::rename(&aside, app);
                return Err(format!("could not put the new Studio in place: {e}"));
            }
            app.clone()
        }
        Install::Folder(dir) => {
            // File by file: the folder itself holds the running program.
            let mut moved: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
            let result = (|| -> Result<(), String> {
                for entry in std::fs::read_dir(&staged.root).map_err(|e| e.to_string())?.flatten() {
                    let name = entry.file_name();
                    let target = dir.join(&name);
                    let aside = old.join(&name);
                    let had = target.exists();
                    if had {
                        std::fs::rename(&target, &aside).map_err(|e| format!("could not move {} aside: {e}", name.to_string_lossy()))?;
                    }
                    moved.push((target.clone(), had.then_some(aside)));
                    std::fs::rename(entry.path(), &target).map_err(|e| format!("could not put {} in place: {e}", name.to_string_lossy()))?;
                }
                Ok(())
            })();
            if let Err(e) = result {
                for (target, aside) in moved.into_iter().rev() {
                    let _ = std::fs::remove_file(&target).or_else(|_| std::fs::remove_dir_all(&target));
                    if let Some(a) = aside {
                        let _ = std::fs::rename(a, &target);
                    }
                }
                return Err(e);
            }
            dir.join(if cfg!(windows) { "tempest-studio.exe" } else { "tempest-studio" })
        }
    };
    relaunch(&exe, &staged.staging)
}

/// The helper: wait for this process, delete `cleanup`, start `target`.
fn relaunch(target: &Path, cleanup: &Path) -> Result<(), String> {
    let pid = std::process::id();
    let mut cmd = if cfg!(windows) {
        let script = format!(
            "Wait-Process -Id {pid} -ErrorAction SilentlyContinue; Remove-Item -LiteralPath '{}' -Recurse -Force -ErrorAction SilentlyContinue; Start-Process -FilePath '{}'",
            cleanup.display().to_string().replace('\'', "''"),
            target.display().to_string().replace('\'', "''")
        );
        let mut c = std::process::Command::new("powershell");
        c.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script]);
        c
    } else {
        let start = if cfg!(target_os = "macos") { "open \"$2\"" } else { "nohup \"$2\" >/dev/null 2>&1 &" };
        let mut c = std::process::Command::new("/bin/sh");
        c.arg("-c").arg(format!("while kill -0 {pid} 2>/dev/null; do sleep 0.2; done; rm -rf \"$1\"; {start}")).arg("sh").arg(cleanup).arg(target);
        c
    };
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    cmd.spawn().map(|_| ()).map_err(|e| format!("could not start the new Studio: {e}"))
}

/// A failed or abandoned update leaves its staging folder: removed at start.
pub fn clean_leftovers() {
    let Some(install) = install() else { return };
    let Ok(entries) = std::fs::read_dir(install.parent()) else { return };
    for e in entries.flatten() {
        if e.file_name().to_string_lossy().starts_with(".tempest-studio-update-") {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("0.0.2", "0.0.1"));
        assert!(is_newer("0.1.0", "0.0.9"));
        assert!(is_newer("1.0.0", "1.0.0-beta.2"));
        assert!(!is_newer("1.0.0-beta.2", "1.0.0"));
        assert!(!is_newer("0.0.1", "0.0.1"));
        assert!(!is_newer("junk", "0.0.1"));
    }

    /// Reaches GitHub: `cargo test update::tests::live -- --ignored`.
    #[test]
    #[ignore]
    fn live() {
        let r = check().expect("GitHub answers");
        eprintln!("newest: {r:?}");
    }

    #[test]
    fn checksum_lines() {
        let h = "ab".repeat(32);
        assert_eq!(sha256_of(&format!("{h}  a.zip\n"), "a.zip"), Some(h.clone()));
        assert_eq!(sha256_of(&format!("{h} *a.zip"), "a.zip"), Some(h.clone()));
        assert_eq!(sha256_of(&h, "a.zip"), Some(h.clone()));
        assert_eq!(sha256_of(&format!("{h}  b.zip"), "a.zip"), None);
    }

    #[test]
    fn unpacks_with_permissions() {
        let dir = std::env::temp_dir().join(format!("tsn-unpack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("a.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
            w.start_file("bundle/tempest-studio", opts).unwrap();
            w.write_all(b"#!/bin/sh\n").unwrap();
            w.start_file("bundle/.tempest-studio-bundle", zip::write::SimpleFileOptions::default()).unwrap();
            w.finish().unwrap();
        }
        // ditto on macOS, the zip crate elsewhere: both must give the same tree.
        unpack(&zip_path, &dir.join("out")).unwrap();
        let exe = dir.join("out/bundle/tempest-studio");
        assert_eq!(std::fs::read(&exe).unwrap(), b"#!/bin/sh\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&exe).unwrap().permissions().mode() & 0o111, 0o111);
        }
        assert!(dir.join("out/bundle/.tempest-studio-bundle").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
