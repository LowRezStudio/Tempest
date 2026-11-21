//! Names of the game parameters, switch groups and states Paladins' banks
//! decide with. Banks keep only their FNV hashes ([`super::fnv::name_id`]);
//! these names were matched against those hashes from the strings of the game
//! (its executable, every package's name table, the assembly) and pairs of
//! their words, the way wwiser's `words.py` reverses names; `upk wwise
//! names` lists the ids still unknown. Media (`.wem`) have no name to find:
//! Wwise numbers them, and only the unshipped `SoundbanksInfo.xml` maps them.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use super::bnk::Condition;
use super::fnv::name_id;

const KNOWN: &[&str] = &[
    // Game parameters.
    "IsFriendlyWithLocalPawn",
    "AmmoAmountPercentage",
    "ChargeAmount",
    "DistanceDefault",
    "PlayerVelocity",
    "SpinSpeed",
    "ThreatLevel",
    // Switch groups.
    "MaterialType",
    "DamageTaken",
    "IsPosition3D",
    "DarklordInhandFireModeIndex",
    "DemonPortalTimer",
    "GuardianChainAttackIndex",
    // States.
    "Friendly",
    "Enemy",
    "Local",
    "Dirt",
    "Grass",
    "Heavywood",
    "Lightwood",
    "Snow",
    "Stone",
    "Water",
    "Fire",
    "Ice",
    "Low",
    "Medium",
    "High",
    "Active",
    "Inactive",
    "AlmostDone",
    "Attack_01",
    "Attack_02",
    "Attack_03",
    "Chain0",
    "Chain1",
    "Chain2",
];

/// The name hashing to `id`, if known.
pub fn name(id: u32) -> Option<&'static str> {
    static BY_ID: OnceLock<HashMap<u32, &'static str>> = OnceLock::new();
    BY_ID
        .get_or_init(|| KNOWN.iter().map(|n| (name_id(n), *n)).collect())
        .get(&id)
        .copied()
}

fn name_or_id(id: u32) -> String {
    name(id)
        .map(str::to_string)
        .unwrap_or_else(|| id.to_string())
}

/// A condition in words, with names where known: `IsFriendlyWithLocalPawn
/// 0.75–1`, `variant 2 of 5`, `Surface = Grass`.
pub fn describe(c: &Condition) -> String {
    match c {
        Condition::Variant { index, count } => format!("variant {} of {count}", index + 1),
        Condition::Crossfade { param, from, to } => format!("{} {from}–{to}", name_or_id(*param)),
        Condition::Switch { group, states, .. } => format!(
            "{} = {}",
            name_or_id(*group),
            states
                .iter()
                .map(|s| name_or_id(*s))
                .collect::<Vec<_>>()
                .join(" or ")
        ),
    }
}

/// Words event titles are split into: built-in ones plus those of the game's
/// texts (FX and ability names), since event names run words together
/// (`nyx_sp_abilityultimate_play`).
#[derive(Clone, Debug)]
pub struct Words(HashSet<String>);

const BUILTIN_WORDS: &str = "ability ultimate weapon fire shoot shot reload charge charged activate active deactivate end start stop loop impact hit hits miss explode explosion projectile proj \
    jump land step footstep footsteps death die dead kill kills killed killing streak assist first blood victory defeat match intro select taunt joke laugh emote mount dismount idle \
    heal healed healing health low me my you your team teammate enemy enemies ally allies friendly behind us attack attacking defend defending capture capturing captured point payload \
    push pushing stuck use using on off cool down cooldown attempting to of the a an in at by for with card armor shield buff debuff stun stunned frozen freeze flying fall falling \
    respawn spawn chatter directed after before taking damage sniper top play unlock unlocking champion class specific item reminder grunt pain voice line lines overhead uppercut \
    swing slash dash leap blink stealth cloak uncloak dome barricade turret grenade potion rift poison trap totem whiz by buzz throw thrown catch beam laser bounce bolt orb";

impl Words {
    pub fn builtin() -> Words {
        let mut w = Words(HashSet::new());
        w.add_text(BUILTIN_WORDS);
        w
    }

    /// The words of a text: split at anything not a letter, and at case changes (`PoisonArmor`).
    pub fn add_text(&mut self, text: &str) {
        let mut cur = String::new();
        let mut prev_lower = false;
        for c in text.chars().chain(std::iter::once(' ')) {
            let boundary = !c.is_ascii_alphabetic() || (c.is_ascii_uppercase() && prev_lower);
            if boundary && cur.len() >= 2 {
                self.0.insert(std::mem::take(&mut cur));
            } else if boundary {
                cur.clear();
            }
            if c.is_ascii_alphabetic() {
                cur.push(c.to_ascii_lowercase());
            }
            prev_lower = c.is_ascii_lowercase();
        }
    }

    /// `word` as known words (fewest pieces), or as it is.
    fn split(&self, word: &str) -> Vec<String> {
        let n = word.len();
        if n < 6 || self.0.contains(word) || !word.is_ascii() {
            return vec![word.to_string()];
        }
        // best[i]: the fewest words covering word[..i].
        let mut best: Vec<Option<(usize, usize)>> = vec![None; n + 1];
        best[0] = Some((0, 0));
        for end in 1..=n {
            for start in 0..end {
                if let Some((count, _)) = best[start] {
                    if end - start >= 2
                        && self.0.contains(&word[start..end])
                        && best[end].is_none_or(|(c, _)| count + 1 < c)
                    {
                        best[end] = Some((count + 1, start));
                    }
                }
            }
        }
        if best[n].is_none() {
            return vec![word.to_string()];
        }
        let mut out = Vec::new();
        let mut end = n;
        while end > 0 {
            let (_, start) = best[end].unwrap();
            out.push(word[start..end].to_string());
            end = start;
        }
        out.reverse();
        out
    }
}

/// An event name in words: `nyx_sp_abilityultimate_play` → `Ultimate`,
/// `nyx_sp_rift_charge_1p_stop` → `Rift charge (first person, stop)`. The
/// leading owner tokens (`owner`: the bank's, such as `nyx`, `skin02`) and
/// codes (`sp`, `tr`, `vox`…) are left out.
pub fn event_title(event: &str, owner: &[String], words: &Words) -> String {
    const CODES: &[&str] = &[
        "sp", "nv", "tr", "mvt", "vo", "vox", "vgs", "pc", "wep", "aud", "sfx", "ww",
    ];
    let lower = event.to_ascii_lowercase();
    let mut tokens: Vec<&str> = lower.split('_').filter(|t| !t.is_empty()).collect();
    let mut notes: Vec<&str> = Vec::new();
    match tokens.last() {
        Some(&"play") => {
            tokens.pop();
        }
        Some(&"stop") => {
            tokens.pop();
            notes.push("stop");
        }
        _ => {}
    }
    while tokens.len() > 1
        && (CODES.contains(&tokens[0]) || owner.iter().any(|o| o.eq_ignore_ascii_case(tokens[0])))
    {
        // `nv`: non-verbal, a grunt rather than a line (`nyx_nv_death` beside `nyx_sp_death`).
        if tokens[0] == "nv" && !tokens[1..].iter().any(|t| t.starts_with("grunt")) {
            notes.insert(0, "grunt");
        }
        tokens.remove(0);
    }
    let mut out: Vec<String> = Vec::new();
    for t in tokens {
        match t {
            "1p" => notes.insert(0, "first person"),
            "3p" => notes.insert(0, "third person"),
            "lp" => out.push("loop".into()),
            "proj" => out.push("projectile".into()),
            "explo" => out.push("explosion".into()),
            _ => {
                // Letters then digits (`ability1`, `skin02`) are two words.
                let cut = t
                    .find(|c: char| c.is_ascii_digit())
                    .filter(|&i| i > 0 && t[i..].chars().all(|c| c.is_ascii_digit()));
                let (word, digits) = match cut {
                    Some(i) => (&t[..i], Some(&t[i..])),
                    None => (t, None),
                };
                out.extend(words.split(word));
                if let Some(d) = digits {
                    let d = d.trim_start_matches('0');
                    out.push(if d.is_empty() {
                        "0".into()
                    } else {
                        d.to_string()
                    });
                }
            }
        }
    }
    // `ability ultimate` is the ultimate.
    if out.len() >= 2 && out[0] == "ability" && out[1] == "ultimate" {
        out.remove(0);
    }
    let mut title = out.join(" ");
    if let Some(first) = title.get(0..1) {
        title = first.to_ascii_uppercase() + &title[1..];
    }
    if !notes.is_empty() {
        title = format!("{title} ({})", notes.join(", "));
    }
    title
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_ids() {
        // As Paladins' voice banks crossfade ally and enemy lines.
        assert_eq!(name(699271803), Some("IsFriendlyWithLocalPawn"));
        assert_eq!(name(2469620754), Some("MaterialType"));
        assert_eq!(
            describe(&Condition::Crossfade {
                param: 699271803,
                from: 0.75,
                to: 1.0
            }),
            "IsFriendlyWithLocalPawn 0.75–1"
        );
        assert_eq!(
            describe(&Condition::Variant { index: 1, count: 5 }),
            "variant 2 of 5"
        );
    }

    #[test]
    fn event_titles() {
        let mut w = Words::builtin();
        w.add_text("Nyx Sounds Poison Armor Activate");
        let nyx = ["nyx".to_string()];
        assert_eq!(
            event_title("nyx_sp_abilityultimate_play", &nyx, &w),
            "Ultimate"
        );
        assert_eq!(event_title("nyx_sp_ability1_play", &nyx, &w), "Ability 1");
        assert_eq!(event_title("nyx_nv_death_play", &nyx, &w), "Death (grunt)");
        assert_eq!(event_title("nyx_nv_gruntjump_play", &nyx, &w), "Grunt jump");
        assert_eq!(
            event_title("nyx_sp_poisonarmor_lp_1p_stop", &nyx, &w),
            "Poison armor loop (first person, stop)"
        );
        assert_eq!(
            event_title("nyx_tr_attemptingtouseabilityoncooldown_play", &nyx, &w),
            "Attempting to use ability on cooldown"
        );
        let skin = ["nyx".to_string(), "skin02".to_string()];
        assert_eq!(
            event_title("nyx_skin02_sp_rift_shoot_play", &skin, &w),
            "Rift shoot"
        );
        // Unknown run-together words stay whole.
        assert_eq!(event_title("nyx_sp_zzqxwv_play", &nyx, &w), "Zzqxwv");
    }
}
