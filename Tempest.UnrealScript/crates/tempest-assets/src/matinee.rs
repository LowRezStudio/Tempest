//! Matinee (UE3's cutscene/mover timelines) as cooked maps store it: a
//! `SeqAct_Interp` Kismet action whose "Data" variable is an `InterpData`
//! (`InterpLength`, `InterpGroups`); each `InterpGroup` (`GroupName`) holds
//! `InterpTracks`, and a track (`InterpTrackMove` with `PosTrack` and
//! `EulerTrack`, float/vector/colour property tracks with `FloatTrack` /
//! `VectorTrack`, `InterpTrackEvent` `EventTrack`, `InterpTrackDirector`
//! `CutTrack`, `InterpTrackToggle` `ToggleTrack`, Hi-Rez's
//! `InterpTrackAkEvent`…) keeps its keys either as an `InterpCurve*`
//! (`Points[] {InVal, OutVal, ArriveTangent, LeaveTangent, InterpMode}`) or
//! as an array of keys with a `Time`. A move track may split its axes into
//! `SubTracks` (`InterpTrackMoveAxis`, one float curve each). The actors a
//! group drives are the `SeqVar_Object`s linked to the action's variable
//! connector named like the group.
//!
//! Everything is ordinary properties, so the curves are read and written
//! generically by property path (`PosTrack.Points[2].OutVal.X`).

use serde_json::{json, Value as J};
use tempest_upk::io::{Error, PackageIndex, Result};
use tempest_upk::Package;

use crate::edit::{Editor, ListOp};
use crate::map::prop;
use crate::object::Object;
use crate::value::{StructValue, Value};

fn member<'a>(v: &'a Value, pkg: &Package, name: &str) -> Option<&'a Value> {
    match v {
        Value::Struct(StructValue::Tagged(ps)) => ps.iter().find(|p| p.array_index == 0 && pkg.fname_to_string(p.name).eq_ignore_ascii_case(name)).map(|p| &p.value),
        Value::Struct(StructValue::Binary(ms)) => ms.iter().find(|m| m.name.eq_ignore_ascii_case(name)).and_then(|m| m.elems.first()),
        _ => None,
    }
}

fn num(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Float(f) => Some(*f as f64),
        Value::Int(i) => Some(*i as f64),
        Value::Byte(b) => Some(*b as f64),
        _ => None,
    }
}

fn refs(v: Option<&Value>) -> Vec<PackageIndex> {
    match v {
        Some(Value::Array(a)) => a.iter().filter_map(|x| if let Value::Object(i) = x { Some(*i) } else { None }).collect(),
        Some(Value::Object(i)) if !i.is_null() => vec![*i],
        _ => Vec::new(),
    }
}

/// A value as JSON: numbers, vectors/colours as arrays, names and enums as text.
fn value_json(v: &Value, pkg: &Package) -> J {
    match v {
        Value::Float(f) => json!(f),
        Value::Int(i) => json!(i),
        Value::Byte(b) => json!(b),
        Value::Bool(b) => json!(b),
        Value::Name(n) | Value::Enum(n) => json!(pkg.fname_to_string(*n)),
        Value::Str(s) => json!(s),
        Value::Object(i) => if i.is_null() { J::Null } else { json!(pkg.object_name(*i)) },
        Value::Struct(_) => {
            for names in [["X", "Y", "Z", ""], ["R", "G", "B", "A"]] {
                let vals: Vec<f64> = names.iter().filter(|n| !n.is_empty()).filter_map(|n| num(member(v, pkg, n))).collect();
                if !vals.is_empty() {
                    return json!(vals);
                }
            }
            J::Null
        }
        _ => J::Null,
    }
}

/// The key lists of a track: curves (`<prop>.Points`) and timed key arrays.
fn track_curves(o: &Object, pkg: &Package) -> Vec<J> {
    let mut out = Vec::new();
    for p in &o.properties {
        let name = pkg.fname_to_string(p.name);
        // InterpCurve: a struct with Points.
        if let Some(Value::Array(points)) = member(&p.value, pkg, "Points") {
            let keys: Vec<J> = points
                .iter()
                .map(|k| {
                    json!({
                        "time": num(member(k, pkg, "InVal")).unwrap_or(0.0),
                        "value": member(k, pkg, "OutVal").map(|v| value_json(v, pkg)).unwrap_or(J::Null),
                        "mode": member(k, pkg, "InterpMode").map(|v| value_json(v, pkg)).unwrap_or(J::Null),
                    })
                })
                .collect();
            out.push(json!({ "name": name, "kind": "curve", "path": format!("{name}.Points"), "keys": keys }));
            continue;
        }
        // An array of keys with a Time.
        if let Value::Array(items) = &p.value {
            if items.first().is_some_and(|k| member(k, pkg, "Time").is_some()) {
                let keys: Vec<J> = items
                    .iter()
                    .map(|k| {
                        let label = match k {
                            Value::Struct(StructValue::Tagged(ps)) => ps
                                .iter()
                                .find(|f| !pkg.fname_to_string(f.name).eq_ignore_ascii_case("Time") && matches!(f.value, Value::Name(_) | Value::Enum(_) | Value::Str(_) | Value::Object(_)))
                                .map(|f| value_json(&f.value, pkg)),
                            _ => None,
                        };
                        json!({ "time": num(member(k, pkg, "Time")).unwrap_or(0.0), "value": label.unwrap_or(J::Null) })
                    })
                    .collect();
                out.push(json!({ "name": name, "kind": "keys", "path": name, "keys": keys }));
            }
        }
    }
    out
}

fn track_json(ed: &mut Editor, t: usize, parent: Option<usize>) -> Result<Vec<J>> {
    let (mut me, subs) = ed.with_object(t, |pkg, o| {
        let class = pkg.class_name_of(PackageIndex::export(t));
        let title = match prop(o, pkg, "TrackTitle") {
            Some(Value::Str(s)) => s.clone(),
            _ => class.trim_start_matches("interptrack").to_string(),
        };
        (
            json!({
                "export": t,
                "class": class,
                "title": title,
                "parent": parent,
                "moveFrame": prop(o, pkg, "MoveFrame").map(|v| value_json(v, pkg)).unwrap_or(json!("imf_world")),
                "moveAxis": prop(o, pkg, "MoveAxis").map(|v| value_json(v, pkg)),
                "property": prop(o, pkg, "PropertyName").map(|v| value_json(v, pkg)),
                "disabled": matches!(prop(o, pkg, "bDisableTrack"), Some(Value::Bool(true))),
                "curves": track_curves(o, pkg),
            }),
            refs(prop(o, pkg, "SubTracks")),
        )
    })?;
    let mut out = Vec::new();
    me["subTracks"] = json!(subs.iter().filter_map(|s| s.export_index()).collect::<Vec<_>>());
    out.push(me);
    for s in subs.iter().filter_map(|s| s.export_index()) {
        out.extend(track_json(ed, s, Some(t))?);
    }
    Ok(out)
}

/// A variable a connector links: itself, or for a `SeqVar_Named` the
/// variable whose `VarName` is its `FindVarName` (resolved at run time by
/// the engine, anywhere in the level).
fn resolve_var(ed: &mut Editor, v: usize) -> Option<usize> {
    if !ed.pkg.class_name_of(PackageIndex::export(v)).eq_ignore_ascii_case("SeqVar_Named") {
        return Some(v);
    }
    let wanted = ed
        .with_object(v, |pkg, o| match prop(o, pkg, "FindVarName") {
            Some(Value::Name(n)) => Some(pkg.fname_to_string(*n)),
            _ => None,
        })
        .ok()??;
    (0..ed.pkg.exports.len()).find(|&j| {
        j != v
            && ed
                .with_object(j, |pkg, o| matches!(prop(o, pkg, "VarName"), Some(Value::Name(n)) if pkg.fname_to_string(*n).eq_ignore_ascii_case(&wanted)))
                .unwrap_or(false)
    })
}

/// `matinee.get`: the timeline of a `SeqAct_Interp` (or of an `InterpData`).
pub fn get(ed: &mut Editor, index: usize) -> Result<J> {
    let class = ed.pkg.class_name_of(PackageIndex::export(index));
    let (action, data) = if class.eq_ignore_ascii_case("InterpData") {
        (None, index)
    } else {
        // The "Data" connector of the action holds the InterpData (or a
        // named variable standing for it).
        let linked = ed.with_object(index, |pkg, o| match prop(o, pkg, "VariableLinks") {
            Some(Value::Array(links)) => links
                .iter()
                .find(|l| refs(member(l, pkg, "ExpectedType")).first().is_some_and(|t| pkg.object_name(*t).eq_ignore_ascii_case("InterpData")))
                .map(|l| refs(member(l, pkg, "LinkedVariables")).iter().filter_map(|v| v.export_index()).collect::<Vec<_>>())
                .unwrap_or_default(),
            _ => Vec::new(),
        })?;
        let data = linked.into_iter().find_map(|v| resolve_var(ed, v)).filter(|&d| ed.pkg.class_name_of(PackageIndex::export(d)).eq_ignore_ascii_case("InterpData"));
        (Some(index), data.ok_or_else(|| Error::Format("this Matinee action has no InterpData linked".into()))?)
    };
    // Actors per group: the action's variable connectors named after the groups.
    let mut group_actors: Vec<(String, Vec<J>)> = Vec::new();
    if let Some(a) = action {
        let links: Vec<(String, Vec<PackageIndex>)> = ed.with_object(a, |pkg, o| match prop(o, pkg, "VariableLinks") {
            Some(Value::Array(links)) => links
                .iter()
                .map(|l| {
                    let desc = match member(l, pkg, "LinkDesc") {
                        Some(Value::Str(s)) => s.clone(),
                        _ => String::new(),
                    };
                    (desc, refs(member(l, pkg, "LinkedVariables")))
                })
                .collect(),
            _ => Vec::new(),
        })?;
        for (desc, vars) in links {
            let mut actors = Vec::new();
            let resolved: Vec<usize> = vars.iter().filter_map(|v| v.export_index()).filter_map(|v| resolve_var(ed, v)).collect();
            for v in resolved {
                let obj = ed.with_object(v, |pkg, o| refs(prop(o, pkg, "ObjValue")).first().copied());
                if let Ok(Some(target)) = obj {
                    actors.push(json!({ "export": target.export_index(), "name": ed.pkg.object_name(target), "class": ed.pkg.class_name_of(target) }));
                }
            }
            group_actors.push((desc, actors));
        }
    }
    let (length, groups) = ed.with_object(data, |pkg, o| (num(prop(o, pkg, "InterpLength")).unwrap_or(5.0), refs(prop(o, pkg, "InterpGroups"))))?;
    let mut gjson = Vec::new();
    for g in groups.iter().filter_map(|g| g.export_index()) {
        let (name, color, tracks) = ed.with_object(g, |pkg, o| {
            let name = match prop(o, pkg, "GroupName") {
                Some(Value::Name(n)) => pkg.fname_to_string(*n),
                _ => String::new(),
            };
            let color = prop(o, pkg, "GroupColor").map(|v| value_json(v, pkg)).unwrap_or(J::Null);
            (name, color, refs(prop(o, pkg, "InterpTracks")))
        })?;
        let mut tj = Vec::new();
        for t in tracks.iter().filter_map(|t| t.export_index()) {
            tj.extend(track_json(ed, t, None)?);
        }
        let actors = group_actors.iter().find(|(d, _)| d.eq_ignore_ascii_case(&name)).map(|(_, a)| a.clone()).unwrap_or_default();
        gjson.push(json!({ "export": g, "class": ed.pkg.class_name_of(PackageIndex::export(g)), "name": name, "color": color, "tracks": tj, "actors": actors }));
    }
    Ok(json!({ "action": action, "data": data, "name": ed.pkg.object_name(PackageIndex::export(data)), "length": length, "groups": gjson }))
}

/// The text `Editor::set` takes for a number.
fn n(v: f64) -> String {
    format!("{v:?}")
}

/// `matinee.setKey {track, curve, key, time?, value?}`: move a key in time
/// and/or set its value (a number, or [x, y, z] / [r, g, b, a]); `curve` is
/// the path the reader gave (`PosTrack.Points`, `EventTrack`).
pub fn set_key(ed: &mut Editor, track: usize, curve: &str, key: usize, time: Option<f64>, value: Option<&J>) -> Result<()> {
    let is_curve = curve.ends_with(".Points");
    let at = format!("{curve}[{key}]");
    if let Some(t) = time {
        ed.set(track, &format!("{at}.{}", if is_curve { "InVal" } else { "Time" }), &n(t))?;
    }
    if let (Some(v), true) = (value, is_curve) {
        match v {
            J::Number(x) => ed.set(track, &format!("{at}.OutVal"), &n(x.as_f64().unwrap_or(0.0)))?,
            J::Array(a) => {
                let names: &[&str] = if a.len() == 4 { &["R", "G", "B", "A"] } else { &["X", "Y", "Z"] };
                for (k, c) in names.iter().enumerate() {
                    if let Some(x) = a.get(k).and_then(|x| x.as_f64()) {
                        ed.set(track, &format!("{at}.OutVal.{c}"), &n(x))?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// `matinee.addKey {track, curve, time}`: a key at `time`, a copy of the key
/// before it (or the first), inserted so the keys stay in time order.
pub fn add_key(ed: &mut Editor, track: usize, curve: &str, time: f64) -> Result<usize> {
    let is_curve = curve.ends_with(".Points");
    let times: Vec<f64> = ed.with_object(track, |pkg, o| {
        let mut parts = curve.split('.');
        let first = parts.next().unwrap_or("");
        let mut v = prop(o, pkg, first);
        for p in parts {
            v = v.and_then(|x| member(x, pkg, p));
        }
        match v {
            Some(Value::Array(items)) => items.iter().map(|k| num(member(k, pkg, if is_curve { "InVal" } else { "Time" })).unwrap_or(0.0)).collect(),
            _ => Vec::new(),
        }
    })?;
    if times.is_empty() {
        return Err(Error::Format("this track has no key to copy; add one in the properties first".into()));
    }
    let before = times.iter().rposition(|t| *t <= time);
    let new = match before {
        Some(k) => {
            ed.edit_list(track, curve, ListOp::Duplicate(k))?;
            k + 1
        }
        None => {
            // Before every key: copy the first after it, then swap the times.
            ed.edit_list(track, curve, ListOp::Duplicate(0))?;
            set_key(ed, track, curve, 1, Some(times[0]), None)?;
            0
        }
    };
    set_key(ed, track, curve, new, Some(time), None)?;
    Ok(new)
}

pub fn remove_key(ed: &mut Editor, track: usize, curve: &str, key: usize) -> Result<()> {
    ed.edit_list(track, curve, ListOp::Remove(key))
}
