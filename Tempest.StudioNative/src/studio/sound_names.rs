//! Sounds in words (Settings ▸ Name sounds by what plays them): why a sound
//! plays, from the conditions `wwise.open` reads in its bank (`when`), and
//! what a sound file holds, from its name (`WWB_VOX_Nyx_Skin02`).

use serde_json::Value as J;

/// Who hears a sound, what variant it is, the surface it is for:
/// `heard by enemies · 2 of 5`. Empty when it always plays.
pub fn when_text(when: &J) -> String {
    when.as_array().into_iter().flatten().filter_map(condition_text).collect::<Vec<_>>().join(" · ")
}

fn condition_text(c: &J) -> Option<String> {
    let name = |v: &J, id: &J| v.as_str().map(str::to_string).unwrap_or_else(|| id.to_string());
    match c["kind"].as_str()? {
        "variant" => Some(format!("{} of {}", c["index"].as_u64()? + 1, c["count"].as_u64()?)),
        "crossfade" => {
            let (from, to) = (c["from"].as_f64()?, c["to"].as_f64()?);
            if c["paramName"].as_str() == Some("IsFriendlyWithLocalPawn") {
                // 1 for the local player and teammates, 0 for enemies.
                return Some(if from >= 0.5 { "heard by you and your team".into() } else if to < 1.0 { "heard by enemies".into() } else { return None });
            }
            Some(format!("{} {from}–{to}", name(&c["paramName"], &c["param"])))
        }
        "switch" | "state" => {
            let states: Vec<String> = c["states"].as_array()?.iter().zip(c["stateNames"].as_array().into_iter().flatten().chain(std::iter::repeat(&J::Null))).map(|(id, n)| name(n, id)).collect();
            let lower: Vec<String> = states.iter().map(|s| s.to_ascii_lowercase()).collect();
            if c["groupName"].as_str() == Some("MaterialType") {
                return Some(format!("on {}", lower.join(" or ")));
            }
            // Friendly / Enemy / Local: whose action it is, from the listener's side.
            if !lower.is_empty() && lower.iter().all(|s| matches!(s.as_str(), "friendly" | "enemy" | "local")) {
                let who: Vec<&str> = lower.iter().map(|s| match s.as_str() { "friendly" => "a teammate", "enemy" => "an enemy", _ => "you" }).collect();
                return Some(format!("by {}", who.join(" or ")));
            }
            Some(format!("{} {}", name(&c["groupName"], &c["group"]), states.join(" or ")))
        }
        _ => None,
    }
}

/// What a sound file's category holds (`VOX` → voice lines).
pub fn category(cat: &str) -> &'static str {
    match cat.to_ascii_uppercase().as_str() {
        "VOX" => "Voice lines",
        "VGS" => "Quick chat",
        "PC" => "Abilities and effects",
        "WEP" => "Weapon",
        "MT" => "Mount",
        "ENV" => "Map",
        "MUS" => "Music",
        "CINE" => "Cinematic",
        "UI" => "Interface",
        "NPC" => "Non-player",
        _ => "Sounds",
    }
}

/// A file of a champion's or item's set: `Voice lines · Scorched`,
/// `Weapon · Skin04`, `Abilities and effects · shared`.
pub fn bank_label(b: &J) -> String {
    let mut out = category(b["category"].as_str().unwrap_or("")).to_string();
    match (b["skin"].as_str(), b["variant"].as_str().filter(|v| !v.is_empty())) {
        (Some(skin), _) => out.push_str(&format!(" · {skin}")),
        (None, Some(v)) => out.push_str(&format!(" · {v}")),
        _ => {}
    }
    if !b["only"].is_null() {
        out.push_str(" · shared");
    }
    out
}

/// A file name from a title: what a file system takes, at most 80 characters.
pub fn file_stem(title: &str) -> String {
    let s: String = title.chars().map(|c| if c.is_alphanumeric() || " -_.()".contains(c) { c } else { '_' }).collect();
    s.trim().chars().take(80).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn roles() {
        let ally = json!([{ "kind": "crossfade", "param": 699271803, "paramName": "IsFriendlyWithLocalPawn", "from": 0.75, "to": 1.0 }]);
        let enemy = json!([{ "kind": "crossfade", "param": 699271803, "paramName": "IsFriendlyWithLocalPawn", "from": 0.0, "to": 0.75 }]);
        assert_eq!(when_text(&ally), "heard by you and your team");
        assert_eq!(when_text(&enemy), "heard by enemies");
        let step = json!([{ "kind": "switch", "group": 1, "groupName": "MaterialType", "states": [2], "stateNames": ["Grass"] }, { "kind": "variant", "index": 1, "count": 5 }]);
        assert_eq!(when_text(&step), "on grass · 2 of 5");
        let unknown = json!([{ "kind": "switch", "group": 973285255, "groupName": null, "states": [2299321487u32], "stateNames": ["Enemy"] }]);
        assert_eq!(when_text(&unknown), "by an enemy");
        assert_eq!(when_text(&json!([])), "");
    }

    #[test]
    fn banks() {
        assert_eq!(bank_label(&json!({ "category": "VOX", "variant": "Skin02", "skin": "Scorched", "only": null })), "Voice lines · Scorched");
        assert_eq!(bank_label(&json!({ "category": "PC", "variant": "", "skin": null, "only": ["x"] })), "Abilities and effects · shared");
        assert_eq!(file_stem("Ultimate · heard by enemies"), "Ultimate _ heard by enemies");
    }
}
