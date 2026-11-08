//! What a mod's details must be, as the launcher's cooker, the CLI and the
//! repository (mods.lowrezstudio.com) check them (a port of the Electron
//! Studio's `modmeta.ts`): the id, the version (semver without build
//! metadata), dependency entries (`id` + an optional version requirement),
//! the icon (a real PNG up to 1 MB) and the summary's length.

/// The longest summary the repository lists.
pub const MAX_SUMMARY: usize = 140;

/// The largest icon a package may carry.
pub const MAX_ICON_BYTES: usize = 1024 * 1024;

/// 2 to 64 of lowercase letters, digits, dots, dashes and underscores, starting with a letter or digit.
pub fn id_ok(id: &str) -> bool {
    let b = id.as_bytes();
    (2..=64).contains(&b.len()) && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit()) && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-'))
}

/// `0`, or digits without a leading zero.
fn numeric(p: &str) -> bool {
    !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()) && (p == "0" || !p.starts_with('0'))
}

/// A pre-release identifier: numeric (no leading zero), or alphanumerics and dashes with a non-digit.
fn pre_ident(p: &str) -> bool {
    !p.is_empty() && p.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') && (numeric(p) || p.bytes().any(|c| !c.is_ascii_digit()))
}

/// A package version: 1.2.3 or 1.2.3-beta.1 (no build metadata: nothing orders it).
pub fn is_semver(v: &str) -> bool {
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (v, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| numeric(p)) && pre.is_none_or(|p| p.split('.').all(pre_ident))
}

/// The semver nearest to what was typed (`v1.2` → `1.2.0`, `1.0 beta_2` →
/// `1.0.0-beta.2`), or `None` when nothing in it reads as a version.
pub fn to_semver(text: &str) -> Option<String> {
    let text = text.split('+').next().unwrap_or_default().trim_start();
    let lower = text.to_ascii_lowercase();
    let rest = if lower.starts_with("version") {
        &text[7..]
    } else if lower.starts_with('v') {
        &text[1..]
    } else {
        text
    }
    .trim_start();
    // Up to three numbers separated by . , or _ (more are dropped), then a pre-release.
    let mut nums: Vec<String> = Vec::new();
    let mut i = 0;
    let b = rest.as_bytes();
    loop {
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if start == i {
            break;
        }
        nums.push(rest[start..i].to_string());
        if i < b.len() && matches!(b[i], b'.' | b',' | b'_') && b.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
            i += 1;
        } else {
            break;
        }
    }
    if nums.is_empty() {
        return None;
    }
    let strip = |n: &str| {
        let t = n.trim_start_matches('0');
        if t.is_empty() { "0".to_string() } else { t.to_string() }
    };
    let core: Vec<String> = (0..3).map(|k| nums.get(k).map(|n| strip(n)).unwrap_or_else(|| "0".into())).collect();
    let pre: Vec<String> = rest[i..].split(|c: char| !c.is_ascii_alphanumeric()).filter(|p| !p.is_empty()).map(|p| if p.bytes().all(|c| c.is_ascii_digit()) { strip(p) } else { p.to_string() }).collect();
    let v = if pre.is_empty() { core.join(".") } else { format!("{}-{}", core.join("."), pre.join(".")) };
    is_semver(&v).then_some(v)
}

/// One comparator of a requirement: `>=1.2`, `^1`, `~1.2.3`, `1.2.x`, a bare version…
fn comparator(t: &str) -> bool {
    let t = ["==", ">=", "<=", ">", "<", "=", "^", "~"].iter().find_map(|op| t.strip_prefix(op)).unwrap_or(t).trim_start();
    let (core, pre) = match t.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (t, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    if parts.is_empty() || parts.len() > 3 || !parts[0].bytes().all(|c| c.is_ascii_digit()) || parts[0].is_empty() {
        return false;
    }
    if !parts[1..].iter().all(|p| *p == "x" || *p == "*" || (!p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))) {
        return false;
    }
    match pre {
        // A pre-release needs the full version (1.2.3-beta, not 1.2-beta).
        Some(p) => parts.len() == 3 && parts[1..].iter().all(|x| x.bytes().all(|c| c.is_ascii_digit())) && !p.is_empty() && p.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'-'),
        None => true,
    }
}

fn requirement_ok(r: &str) -> bool {
    let r = r.trim();
    if r.is_empty() || r == "*" {
        return true;
    }
    r.split("||").all(|part| {
        let tokens: Vec<&str> = part.split([' ', ',', '\t']).filter(|t| !t.is_empty()).collect();
        !tokens.is_empty() && tokens.iter().all(|t| comparator(t))
    })
}

/// Why a dependency line is not one the repository accepts: a mod id (or
/// `tempest`, the launcher) then an optional requirement such as `>=1.0.0`,
/// `^2`, `~1.2` or `>=1.0 <2.0`.
pub fn dependency_problem(line: &str) -> Option<String> {
    let line = line.trim();
    let end = line.find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))).unwrap_or(line.len());
    if end == 0 {
        return Some(format!("\"{line}\" does not start with a mod id"));
    }
    let id = line[..end].to_ascii_lowercase();
    if id != "tempest" && !id_ok(&id) {
        return Some(format!("\"{}\" is not a mod id", &line[..end]));
    }
    let req = line[end..].trim();
    if !req.is_empty() && (!req.starts_with(['<', '>', '=', '^', '~', '*', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9']) || !requirement_ok(req)) {
        return Some(format!("\"{req}\" is not a version requirement (for example >=1.0.0, ^2 or ~1.2)"));
    }
    None
}

/// Why a file cannot be a package's icon: packages carry it as `icon.png`, so it must really be one, and small.
pub fn icon_problem(data: &[u8]) -> Option<String> {
    if !data.starts_with(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some("The icon must be a PNG image".into());
    }
    if data.len() > MAX_ICON_BYTES {
        return Some(format!("The icon is {:.2} MB; it may be 1 MB at most", data.len() as f64 / (1024.0 * 1024.0)));
    }
    None
}

/// True when the repository refused the credentials (log in again, not a generic error).
pub fn is_auth_error(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    ["no api key", "unauthorized", "401", "403"].iter().any(|k| m.contains(k))
}

/// What `tempest mod publish --json` reports.
#[derive(Clone, Debug, Default)]
pub struct Published {
    pub state: String,
    pub mod_id: Option<String>,
    pub version: Option<String>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// The outcome printed by `mod publish --json`, or `None` when stdout holds none.
pub fn parse_publish(stdout: &str) -> Option<Published> {
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
    let state = v["state"].as_str()?.to_string();
    let r = &v["result"];
    let list = |k: &str| r[k].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect();
    Some(Published { state, mod_id: r["modId"].as_str().map(str::to_string), version: r["version"].as_str().map(str::to_string), errors: list("errors"), warnings: list("warnings") })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver() {
        for ok in ["1.0.0", "0.1.0", "1.2.3-beta.1", "1.2.3-rc-1", "10.20.30"] {
            assert!(is_semver(ok), "{ok}");
        }
        for bad in ["1.0", "01.0.0", "1.0.0+b", "1.0.0-", "1.0.0-01", "v1.0.0", "1.0.0-beta..1"] {
            assert!(!is_semver(bad), "{bad}");
        }
    }

    #[test]
    fn nearest() {
        assert_eq!(to_semver("v1.2").as_deref(), Some("1.2.0"));
        assert_eq!(to_semver("1.0 beta_2").as_deref(), Some("1.0.0-beta.2"));
        assert_eq!(to_semver("Version 2").as_deref(), Some("2.0.0"));
        assert_eq!(to_semver("1.2.3.4").as_deref(), Some("1.2.3"));
        assert_eq!(to_semver("007").as_deref(), Some("7.0.0"));
        assert_eq!(to_semver("beta"), None);
    }

    #[test]
    fn dependencies() {
        for ok in ["tempest-scripting>=1.0.0", "tempest", "my.mod ^2", "a1 >=1.0 <2.0", "abc ~1.2", "abc 1.2.x", "abc *", "abc >=1.2.3-beta"] {
            assert_eq!(dependency_problem(ok), None, "{ok}");
        }
        for bad in ["", ">=1.0", "A", "abc foo", "abc >=1.2-beta", "abc >="] {
            assert!(dependency_problem(bad).is_some(), "{bad}");
        }
    }

    #[test]
    fn ids_and_icons() {
        assert!(id_ok("my-mod") && id_ok("a1") && !id_ok("a") && !id_ok("-a") && !id_ok("My"));
        assert!(icon_problem(b"\x89PNG\r\n\x1a\nrest").is_none());
        assert!(icon_problem(b"\xff\xd8\xff").is_some());
    }

    #[test]
    fn publish_output() {
        let p = parse_publish(r#"{"state":"done","result":{"modId":"x","version":"1.0.0","errors":[],"warnings":["w"]}}"#).unwrap();
        assert_eq!((p.state.as_str(), p.mod_id.as_deref(), p.warnings.len()), ("done", Some("x"), 1));
        assert!(parse_publish("Publish failed: 401").is_none());
        assert!(is_auth_error("Unauthorized") && !is_auth_error("bad version"));
    }
}
