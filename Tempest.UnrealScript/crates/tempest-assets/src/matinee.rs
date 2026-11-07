//! Matinee (UE3's cutscene/mover timelines) as cooked maps store it: a
//! `SeqAct_Interp` Kismet action whose "Data" variable is an `InterpData`
//! (`InterpLength`, `InterpGroups`); each `InterpGroup` (`GroupName`) holds
//! `InterpTracks`, and a track (`InterpTrackMove` with `PosTrack` and
//! `EulerTrack`, float/vector/colour property tracks with `FloatTrack` /
//! `VectorTrack`, `InterpTrackEvent` `EventTrack`, `InterpTrackDirector`
//! `CutTrack`, `InterpTrackToggle` `ToggleTrack`, Hi-Rez's
//! `InterpTrackAkEvent`…) keeps its keys either as an `InterpCurve*`
//! (`Points[] {InVal, OutVal, ArriveTangent, LeaveTangent, InterpMode}`) or
//! as an array of keys with a `Time` (`StartTime` for anim sequences). A move
//! track may split its axes into `SubTracks` (`InterpTrackMoveAxis`, one
//! float curve each). The actors a group drives are the `SeqVar_Object`s
//! linked to the action's variable connector named like the group.
//!
//! Everything is ordinary properties (no track, group or data object has a
//! native part), so the curves are read and written generically by
//! property path (`PosTrack.Points[2].OutVal.X`), and new objects are
//! property-only exports. Editing follows the engine's editor code
//! (UnInterpolation.cpp, InterpEditor*.cpp): key lists that must stay
//! parallel (a move track's `PosTrack`/`EulerTrack`/`LookupTrack`, an axis'
//! `FloatTrack`/`LookupTrack`) change together, curves get
//! `AutoSetTangents` after every change, and `USeqAct_Interp::
//! UpdateConnectorsFromData` keeps the action's connectors in step with the
//! groups and event names.

use serde_json::{json, Map, Value as J};
use tempest_upk::io::{Error, PackageIndex, Result};
use tempest_upk::Package;

use crate::edit::{Editor, ListOp};
use crate::map::prop;
use crate::matinee_curve::{auto_set_tangents, eval, Mode, Point};
use crate::object::Object;
use crate::value::{StructValue, Value};

fn member<'a>(v: &'a Value, pkg: &Package, name: &str) -> Option<&'a Value> {
    match v {
        Value::Struct(StructValue::Tagged(ps)) => ps.iter().find(|p| p.array_index == 0 && pkg.fname_to_string(p.name).eq_ignore_ascii_case(name)).map(|p| &p.value),
        Value::Struct(StructValue::Binary(ms)) => ms.iter().find(|m| m.name.eq_ignore_ascii_case(name)).and_then(|m| m.elems.first()),
        _ => None,
    }
}

fn member_mut<'a>(v: &'a mut Value, pkg: &Package, name: &str) -> Option<&'a mut Value> {
    match v {
        Value::Struct(StructValue::Tagged(ps)) => ps.iter_mut().find(|p| p.array_index == 0 && pkg.fname_to_string(p.name).eq_ignore_ascii_case(name)).map(|p| &mut p.value),
        Value::Struct(StructValue::Binary(ms)) => ms.iter_mut().find(|m| m.name.eq_ignore_ascii_case(name)).and_then(|m| m.elems.first_mut()),
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

fn name_of(v: Option<&Value>, pkg: &Package) -> Option<String> {
    match v? {
        Value::Name(n) | Value::Enum(n) => Some(pkg.fname_to_string(*n)),
        Value::Str(s) => Some(s.clone()),
        _ => None,
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
        Value::Object(i) => {
            if i.is_null() {
                J::Null
            } else {
                json!(pkg.object_name(*i))
            }
        }
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

/// The text `Editor::set` takes for an object reference (export or import).
pub(crate) fn ref_text(pkg: &Package, i: PackageIndex) -> String {
    if i.is_null() {
        return "None".into();
    }
    format!("{}'{}'", pkg.class_name_of(i), pkg.object_path(i))
}

/// The text `Editor::set` takes for a number.
fn n(v: f64) -> String {
    format!("{v:?}")
}

// ----- key lists -------------------------------------------------------------------

/// The value components of a curve type (`InterpCurveFloat` has one, unnamed).
fn components(struct_name: &str) -> Option<&'static [&'static str]> {
    Some(match struct_name.to_ascii_lowercase().as_str() {
        "interpcurvefloat" => &[],
        "interpcurvevector2d" => &["X", "Y"],
        "interpcurvevector" => &["X", "Y", "Z"],
        "interpcurvelinearcolor" => &["R", "G", "B", "A"],
        _ => return None,
    })
}

/// The key lists a track class has even when it stores none of them (a
/// track only stores what differs from its class default, so an empty curve
/// is absent): `(property, curve type or None for timed keys)`.
fn expected_lists(db: &crate::reflect::ClassDb, class: &str) -> Vec<(&'static str, Option<&'static str>)> {
    let is = |b: &str| db.class_is_a(class, b);
    if is("InterpTrackMove") {
        vec![("PosTrack", Some("interpcurvevector")), ("EulerTrack", Some("interpcurvevector"))]
    } else if is("InterpTrackMoveAxis") || is("InterpTrackFloatBase") {
        vec![("FloatTrack", Some("interpcurvefloat"))]
    } else if is("InterpTrackSound") {
        vec![("Sounds", None), ("VectorTrack", Some("interpcurvevector"))]
    } else if is("InterpTrackVectorBase") {
        vec![("VectorTrack", Some("interpcurvevector"))]
    } else if is("InterpTrackLinearColorBase") {
        vec![("LinearColorTrack", Some("interpcurvelinearcolor"))]
    } else if is("InterpTrackEvent") {
        vec![("EventTrack", None)]
    } else if is("InterpTrackDirector") {
        vec![("CutTrack", None)]
    } else if is("InterpTrackToggle") {
        vec![("ToggleTrack", None)]
    } else if is("InterpTrackAnimControl") {
        vec![("AnimSeqs", None)]
    } else if is("InterpTrackVisibility") {
        vec![("VisibilityTrack", None)]
    } else if is("InterpTrackParticleReplay") {
        vec![("TrackKeys", None)]
    } else if is("InterpTrackNotify") {
        vec![("NotifyTrack", None)]
    } else if class.eq_ignore_ascii_case("InterpTrackAkEvent") {
        vec![("AkEvents", None)]
    } else {
        Vec::new()
    }
}

/// The member holding a timed key's time.
fn time_field(list: &str) -> &'static str {
    if list.eq_ignore_ascii_case("AnimSeqs") {
        "StartTime"
    } else {
        "Time"
    }
}

/// The lists that must keep the same keys as `path` (the engine's
/// `check(PosTrack.Points.Num() == EulerTrack.Points.Num() == LookupTrack.Points.Num())`).
fn linked(class: &str, path: &str) -> Vec<String> {
    let lower = path.to_ascii_lowercase();
    let set: &[&str] = if class.eq_ignore_ascii_case("interptrackmove") && ["postrack.points", "eulertrack.points", "lookuptrack.points"].contains(&lower.as_str()) {
        &["PosTrack.Points", "EulerTrack.Points", "LookupTrack.Points"]
    } else if class.eq_ignore_ascii_case("interptrackmoveaxis") && ["floattrack.points", "lookuptrack.points"].contains(&lower.as_str()) {
        &["FloatTrack.Points", "LookupTrack.Points"]
    } else {
        return vec![path.to_string()];
    };
    set.iter().map(|s| s.to_string()).collect()
}

fn is_curve_path(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".points") && !path.to_ascii_lowercase().starts_with("lookuptrack")
}

fn time_member(path: &str) -> &'static str {
    if path.to_ascii_lowercase().starts_with("lookuptrack") {
        "Time"
    } else if is_curve_path(path) {
        "InVal"
    } else {
        time_field(path)
    }
}

/// The array value at `path` (`Prop` or `Prop.Points`).
fn array_at<'a>(o: &'a Object, pkg: &Package, path: &str) -> Option<&'a Vec<Value>> {
    let mut parts = path.split('.');
    let mut v = prop(o, pkg, parts.next()?);
    for p in parts {
        v = v.and_then(|x| member(x, pkg, p));
    }
    match v {
        Some(Value::Array(a)) => Some(a),
        _ => None,
    }
}

fn array_at_mut<'a>(o: &'a mut Object, pkg: &Package, path: &str) -> Option<&'a mut Vec<Value>> {
    let mut parts = path.split('.');
    let first = parts.next()?;
    let p = o.properties.iter_mut().find(|p| p.array_index == 0 && pkg.fname_to_string(p.name).eq_ignore_ascii_case(first))?;
    let mut v = &mut p.value;
    for part in parts {
        v = member_mut(v, pkg, part)?;
    }
    match v {
        Value::Array(a) => Some(a),
        _ => None,
    }
}

/// The struct type of a curve property, from the stored tag or the class table.
fn curve_type(o: &Object, pkg: &Package, db: &crate::reflect::ClassDb, class: &str, prop_name: &str) -> Option<String> {
    if let Some(p) = o.property(prop_name, pkg) {
        if let Some(s) = p.struct_name {
            return Some(pkg.fname_to_string(s));
        }
    }
    expected_lists(db, class).into_iter().find(|(n, _)| n.eq_ignore_ascii_case(prop_name)).and_then(|(_, t)| t.map(str::to_string))
}

/// The points of curve `path` of track `t`.
fn read_points(ed: &mut Editor, t: usize, path: &str) -> Result<(Vec<Point>, &'static [&'static str])> {
    let class = ed.pkg.class_name_of(PackageIndex::export(t));
    let db = ed.db;
    let prop_name = path.split('.').next().unwrap_or("").to_string();
    ed.with_object(t, |pkg, o| {
        let comps = curve_type(o, pkg, db, &class, &prop_name).and_then(|s| components(&s)).unwrap_or(&[]);
        let vals = |v: Option<&Value>| -> Vec<f64> {
            match v {
                None => vec![0.0; comps.len().max(1)],
                Some(x) if comps.is_empty() => vec![num(Some(x)).unwrap_or(0.0)],
                Some(x) => comps.iter().map(|c| num(member(x, pkg, c)).unwrap_or(0.0)).collect(),
            }
        };
        let points = array_at(o, pkg, path)
            .map(|items| {
                items
                    .iter()
                    .map(|k| Point {
                        time: num(member(k, pkg, "InVal")).unwrap_or(0.0),
                        out: vals(member(k, pkg, "OutVal")),
                        arrive: vals(member(k, pkg, "ArriveTangent")),
                        leave: vals(member(k, pkg, "LeaveTangent")),
                        mode: name_of(member(k, pkg, "InterpMode"), pkg).map(|m| Mode::parse(&m)).unwrap_or(Mode::Linear),
                    })
                    .collect()
            })
            .unwrap_or_default();
        (points, comps)
    })
}

/// Write `v` into `<at>.<field>` (one number, or the components).
fn write_vec(ed: &mut Editor, t: usize, at: &str, field: &str, comps: &[&str], v: &[f64]) -> Result<()> {
    if comps.is_empty() {
        return ed.set(t, &format!("{at}.{field}"), &n(v.first().copied().unwrap_or(0.0)));
    }
    for (k, c) in comps.iter().enumerate() {
        ed.set(t, &format!("{at}.{field}.{c}"), &n(v.get(k).copied().unwrap_or(0.0)))?;
    }
    Ok(())
}

/// The tension the editor passes to `AutoSetTangents` for a curve.
fn tension(ed: &mut Editor, t: usize, path: &str) -> f64 {
    let lower = path.to_ascii_lowercase();
    let name = if lower.starts_with("postrack") {
        "LinCurveTension"
    } else if lower.starts_with("eulertrack") {
        "AngCurveTension"
    } else {
        "CurveTension"
    };
    ed.with_object(t, |pkg, o| num(prop(o, pkg, name))).ok().flatten().unwrap_or(0.0)
}

/// `AutoSetTangents` on curve `path` of track `t`, written back where it
/// changed a tangent. Linear colour curves have no clamping overload in the
/// engine (the general `ComputeCurveTangent` ignores `bWantClamping`).
fn retangent(ed: &mut Editor, t: usize, path: &str) -> Result<()> {
    let (before, comps) = read_points(ed, t, path)?;
    let mut after = before.clone();
    let tension = tension(ed, t, path);
    auto_set_tangents(&mut after, tension, comps.len() != 4);
    for (k, (a, b)) in before.iter().zip(&after).enumerate() {
        let at = format!("{path}[{k}]");
        let same = |x: &[f64], y: &[f64]| x.iter().zip(y).all(|(p, q)| (*p as f32) == (*q as f32));
        if !same(&a.arrive, &b.arrive) {
            write_vec(ed, t, &at, "ArriveTangent", comps, &b.arrive)?;
        }
        if !same(&a.leave, &b.leave) {
            write_vec(ed, t, &at, "LeaveTangent", comps, &b.leave)?;
        }
    }
    Ok(())
}

fn retangent_all(ed: &mut Editor, t: usize, lists: &[String]) -> Result<()> {
    for l in lists.iter().filter(|l| is_curve_path(l)) {
        retangent(ed, t, l)?;
    }
    Ok(())
}

/// The times of a key list.
fn times(ed: &mut Editor, t: usize, path: &str) -> Result<Vec<f64>> {
    let field = time_member(path);
    ed.with_object(t, |pkg, o| array_at(o, pkg, path).map(|items| items.iter().map(|k| num(member(k, pkg, field)).unwrap_or(0.0)).collect()).unwrap_or_default())
}

/// Move key `from` to index `to` in every list of `lists`.
fn move_key(ed: &mut Editor, t: usize, lists: &[String], from: usize, to: usize) -> Result<()> {
    if from == to {
        return Ok(());
    }
    let mut obj = ed.object(t)?.clone();
    for l in lists {
        if let Some(a) = array_at_mut(&mut obj, &ed.pkg, l) {
            if from < a.len() && to < a.len() {
                let k = a.remove(from);
                a.insert(to, k);
            }
        }
    }
    ed.replace_object(t, obj)
}

// ----- reading ---------------------------------------------------------------------

fn keys_json(items: &[Value], pkg: &Package, time: &str) -> Vec<J> {
    items
        .iter()
        .map(|k| {
            let mut fields = Map::new();
            let mut label = None;
            if let Value::Struct(StructValue::Tagged(ps)) = k {
                for f in ps {
                    let name = pkg.fname_to_string(f.name);
                    if name.eq_ignore_ascii_case(time) {
                        continue;
                    }
                    if label.is_none() && matches!(f.value, Value::Name(_) | Value::Enum(_) | Value::Str(_) | Value::Object(_)) {
                        label = Some(value_json(&f.value, pkg));
                    }
                    let v = match &f.value {
                        Value::Object(i) if !i.is_null() => json!({ "path": pkg.object_path(*i), "class": pkg.class_name_of(*i), "name": pkg.object_name(*i) }),
                        other => value_json(other, pkg),
                    };
                    fields.insert(name, v);
                }
            }
            json!({ "time": num(member(k, pkg, time)).unwrap_or(0.0), "value": label.unwrap_or(J::Null), "fields": fields })
        })
        .collect()
}

/// The key lists of a track: curves (`<prop>.Points`) and timed key arrays,
/// the class's expected lists included when empty. `LookupTrack` is
/// internal (it follows the move keys) and left out.
fn track_curves(o: &Object, pkg: &Package, db: &crate::reflect::ClassDb, class: &str) -> Vec<J> {
    let mut out = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for p in &o.properties {
        let name = pkg.fname_to_string(p.name);
        if name.eq_ignore_ascii_case("LookupTrack") {
            continue;
        }
        if let Some(Value::Array(points)) = member(&p.value, pkg, "Points") {
            let comps = p.struct_name.map(|s| pkg.fname_to_string(s)).and_then(|s| components(&s)).unwrap_or(&[]);
            let vals = |v: Option<&Value>| -> J {
                match v {
                    None => J::Null,
                    Some(x) if comps.is_empty() => json!(num(Some(x)).unwrap_or(0.0)),
                    Some(x) => json!(comps.iter().map(|c| num(member(x, pkg, c)).unwrap_or(0.0)).collect::<Vec<_>>()),
                }
            };
            let keys: Vec<J> = points
                .iter()
                .map(|k| {
                    json!({
                        "time": num(member(k, pkg, "InVal")).unwrap_or(0.0),
                        "value": vals(member(k, pkg, "OutVal")),
                        "arrive": vals(member(k, pkg, "ArriveTangent")),
                        "leave": vals(member(k, pkg, "LeaveTangent")),
                        "mode": name_of(member(k, pkg, "InterpMode"), pkg).map(|m| Mode::parse(&m).name()).unwrap_or("CIM_Linear"),
                    })
                })
                .collect();
            seen.push(name.to_ascii_lowercase());
            out.push(json!({ "name": name, "kind": "curve", "path": format!("{name}.Points"), "width": comps.len().max(1), "components": comps, "keys": keys }));
            continue;
        }
        if let Value::Array(items) = &p.value {
            let tf = time_field(&name);
            if items.first().is_some_and(|k| member(k, pkg, tf).is_some()) {
                seen.push(name.to_ascii_lowercase());
                out.push(json!({ "name": name, "kind": "keys", "path": name, "timeField": tf, "keys": keys_json(items, pkg, tf) }));
            }
        }
    }
    for (name, ty) in expected_lists(db, class) {
        if seen.contains(&name.to_ascii_lowercase()) {
            continue;
        }
        match ty {
            Some(t) => {
                let comps = components(t).unwrap_or(&[]);
                out.push(json!({ "name": name, "kind": "curve", "path": format!("{name}.Points"), "width": comps.len().max(1), "components": comps, "keys": [] }));
            }
            None => out.push(json!({ "name": name, "kind": "keys", "path": name, "timeField": time_field(name), "keys": [] })),
        }
    }
    out
}

fn bool_of(o: &Object, pkg: &Package, name: &str) -> bool {
    matches!(prop(o, pkg, name), Some(Value::Bool(true)))
}

fn track_json(ed: &mut Editor, t: usize, parent: Option<usize>) -> Result<Vec<J>> {
    let db = ed.db;
    let (mut me, subs) = ed.with_object(t, |pkg, o| {
        let class = pkg.class_name_of(PackageIndex::export(t));
        let title = match prop(o, pkg, "TrackTitle") {
            Some(Value::Str(s)) => s.clone(),
            _ => class.trim_start_matches("interptrack").to_string(),
        };
        let is_move = db.class_is_a(&class, "InterpTrackMove");
        (
            json!({
                "export": t,
                "class": class,
                "title": title,
                "parent": parent,
                "moveFrame": if is_move { prop(o, pkg, "MoveFrame").map(|v| value_json(v, pkg)).unwrap_or(json!("imf_world")) } else { J::Null },
                // `MoveAxis` defaults to its first value (AXIS_TranslationX), which cooked axes leave out.
                "moveAxis": if db.class_is_a(&class, "InterpTrackMoveAxis") { prop(o, pkg, "MoveAxis").map(|v| value_json(v, pkg)).unwrap_or(json!("axis_translationx")) } else { J::Null },
                "rotMode": if is_move { prop(o, pkg, "RotMode").map(|v| value_json(v, pkg)).unwrap_or(json!("imr_keyframed")) } else { J::Null },
                "lookAtGroup": name_of(prop(o, pkg, "LookAtGroupName"), pkg).filter(|s| !s.eq_ignore_ascii_case("none")),
                "rawInitial": bool_of(o, pkg, "bUseRawActorTMforRelativeToInitial"),
                "property": prop(o, pkg, "PropertyName").or_else(|| prop(o, pkg, "ParamName")).map(|v| value_json(v, pkg)),
                "disabled": bool_of(o, pkg, "bDisableTrack"),
                "activeCondition": prop(o, pkg, "ActiveCondition").map(|v| value_json(v, pkg)).unwrap_or(json!("etac_always")),
                "curves": track_curves(o, pkg, db, &class),
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

/// The action and InterpData of a timeline export (a `SeqAct_Interp` or an `InterpData`).
pub fn resolve(ed: &mut Editor, index: usize) -> Result<(Option<usize>, usize)> {
    let class = ed.pkg.class_name_of(PackageIndex::export(index));
    if class.eq_ignore_ascii_case("InterpData") {
        return Ok((None, index));
    }
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
    Ok((Some(index), data.ok_or_else(|| Error::Format("this Matinee action has no InterpData linked".into()))?))
}

fn groups_of(ed: &mut Editor, data: usize) -> Result<Vec<usize>> {
    ed.with_object(data, |pkg, o| refs(prop(o, pkg, "InterpGroups")).iter().filter_map(|g| g.export_index()).collect())
}

fn group_name(ed: &mut Editor, g: usize) -> String {
    let is_dir = ed.db.class_is_a(&ed.pkg.class_name_of(PackageIndex::export(g)), "InterpGroupDirector");
    ed.with_object(g, |pkg, o| name_of(prop(o, pkg, "GroupName"), pkg))
        .ok()
        .flatten()
        // The class defaults: InterpGroup 'InterpGroup', InterpGroupDirector 'DirGroup'.
        .unwrap_or_else(|| if is_dir { "DirGroup".into() } else { "InterpGroup".into() })
}

/// `matinee.get`: the timeline of a `SeqAct_Interp` (or of an `InterpData`).
pub fn get(ed: &mut Editor, index: usize) -> Result<J> {
    let (action, data) = resolve(ed, index)?;
    // Actors per group: the action's variable connectors named after the groups.
    let mut group_actors: Vec<(String, Vec<J>)> = Vec::new();
    let mut settings = json!({});
    let mut sequence = J::Null;
    if let Some(a) = action {
        let (links, s, seq) = ed.with_object(a, |pkg, o| {
            let links: Vec<(String, Vec<PackageIndex>)> = match prop(o, pkg, "VariableLinks") {
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
            };
            // Absent values are the class defaults (Default__SeqAct_Interp: PlayRate 1, every flag off).
            let s = json!({
                "playRate": num(prop(o, pkg, "PlayRate")).unwrap_or(1.0),
                "looping": bool_of(o, pkg, "bLooping"),
                "rewindOnPlay": bool_of(o, pkg, "bRewindOnPlay"),
                "noResetOnRewind": bool_of(o, pkg, "bNoResetOnRewind"),
                "rewindIfAlreadyPlaying": bool_of(o, pkg, "bRewindIfAlreadyPlaying"),
                "reversePlayback": bool_of(o, pkg, "bReversePlayback"),
                "forceStartPos": bool_of(o, pkg, "bForceStartPos"),
                "forceStartPosition": num(prop(o, pkg, "ForceStartPosition")).unwrap_or(0.0),
                "clientSideOnly": bool_of(o, pkg, "bClientSideOnly"),
            });
            (links, s, refs(prop(o, pkg, "ParentSequence")).first().and_then(|x| x.export_index()))
        })?;
        settings = s;
        sequence = json!(seq);
        for (desc, vars) in links {
            let mut actors = Vec::new();
            let resolved: Vec<(usize, usize)> = vars.iter().filter_map(|v| v.export_index()).filter_map(|v| resolve_var(ed, v).map(|r| (v, r))).collect();
            for (var, v) in resolved {
                let obj = ed.with_object(v, |pkg, o| refs(prop(o, pkg, "ObjValue")).first().copied());
                if let Ok(Some(target)) = obj {
                    actors.push(json!({ "export": target.export_index(), "name": ed.pkg.object_name(target), "class": ed.pkg.class_name_of(target), "variable": var }));
                }
            }
            group_actors.push((desc, actors));
        }
    }
    let (length, sec_start, sec_end) = ed.with_object(data, |pkg, o| (num(prop(o, pkg, "InterpLength")).unwrap_or(5.0), num(prop(o, pkg, "EdSectionStart")).unwrap_or(0.0), num(prop(o, pkg, "EdSectionEnd")).unwrap_or(1.0)))?;
    let groups = groups_of(ed, data)?;
    let mut gjson = Vec::new();
    for g in groups {
        let name = group_name(ed, g);
        let (color, tracks, folder, parented, anim_sets) = ed.with_object(g, |pkg, o| {
            // GroupColor default (InterpGroup): (R=100, G=80, B=200, A=255).
            let color = prop(o, pkg, "GroupColor").map(|v| value_json(v, pkg)).unwrap_or(json!([100, 80, 200, 255]));
            let sets: Vec<String> = refs(prop(o, pkg, "GroupAnimSets")).iter().map(|s| pkg.object_name(*s)).collect();
            (color, refs(prop(o, pkg, "InterpTracks")), bool_of(o, pkg, "bIsFolder"), bool_of(o, pkg, "bIsParented"), sets)
        })?;
        let mut tj = Vec::new();
        for t in tracks.iter().filter_map(|t| t.export_index()) {
            tj.extend(track_json(ed, t, None)?);
        }
        let actors = group_actors.iter().find(|(d, _)| d.eq_ignore_ascii_case(&name)).map(|(_, a)| a.clone()).unwrap_or_default();
        let class = ed.pkg.class_name_of(PackageIndex::export(g));
        let director = ed.db.class_is_a(&class, "InterpGroupDirector");
        gjson.push(json!({ "export": g, "class": class, "name": name, "color": color, "director": director, "folder": folder, "parented": parented, "animSets": anim_sets, "tracks": tj, "actors": actors }));
    }
    Ok(json!({
        "action": action,
        "data": data,
        "sequence": sequence,
        "name": ed.pkg.object_name(PackageIndex::export(data)),
        "length": length,
        "section": [sec_start, sec_end],
        "settings": settings,
        "groups": gjson,
    }))
}

// ----- keys ------------------------------------------------------------------------

fn nums(v: &J) -> Option<Vec<f64>> {
    match v {
        J::Number(x) => Some(vec![x.as_f64()?]),
        J::Array(a) => a.iter().map(|x| x.as_f64()).collect(),
        _ => None,
    }
}

/// What `matinee.setKey` changes; every field is optional.
#[derive(Default)]
pub struct KeyEdit<'a> {
    pub time: Option<f64>,
    /// A number, or [x, y, z] / [r, g, b, a].
    pub value: Option<&'a J>,
    /// `CIM_*` (curves; on a move track every curve of the key).
    pub mode: Option<&'a str>,
    pub arrive: Option<&'a J>,
    pub leave: Option<&'a J>,
    /// Timed keys: member name → text as `Editor::set` takes it.
    pub fields: Option<&'a Map<String, J>>,
}

fn set_mode(ed: &mut Editor, t: usize, path: &str, key: usize, mode: Mode) -> Result<()> {
    ed.set(t, &format!("{path}[{key}].InterpMode"), &mode.name().to_ascii_lowercase())
}

/// `matinee.setKey {track, curve, key, …}`: retime (every parallel list of
/// the key moves with it and the key is moved to keep them in time order),
/// set the value, interp mode, tangents (setting a tangent of an auto key
/// makes it `CIM_CurveUser`, as dragging a handle in the curve editor
/// does) or timed-key members; then `AutoSetTangents`. Returns the key's
/// index afterwards.
pub fn set_key(ed: &mut Editor, track: usize, curve: &str, key: usize, e: &KeyEdit) -> Result<usize> {
    let class = ed.pkg.class_name_of(PackageIndex::export(track));
    let lists = linked(&class, curve);
    let at = format!("{curve}[{key}]");
    if is_curve_path(curve) {
        let (points, comps) = read_points(ed, track, curve)?;
        let cur = points.get(key).ok_or_else(|| Error::Format(format!("no key {key}")))?.clone();
        if let Some(v) = e.value.and_then(nums) {
            write_vec(ed, track, &at, "OutVal", comps, &v)?;
        }
        if let Some(m) = e.mode {
            let mode = Mode::parse(m);
            for l in lists.iter().filter(|l| is_curve_path(l)) {
                if times(ed, track, l)?.len() > key {
                    set_mode(ed, track, l, key, mode)?;
                }
            }
        }
        let mode = e.mode.map(Mode::parse).unwrap_or(cur.mode);
        if e.arrive.is_some() || e.leave.is_some() {
            if matches!(mode, Mode::CurveAuto | Mode::CurveAutoClamped | Mode::Linear | Mode::Constant) {
                set_mode(ed, track, curve, key, Mode::CurveUser)?;
            }
            if let Some(v) = e.arrive.and_then(nums) {
                write_vec(ed, track, &at, "ArriveTangent", comps, &v)?;
            }
            if let Some(v) = e.leave.and_then(nums) {
                write_vec(ed, track, &at, "LeaveTangent", comps, &v)?;
            }
        }
    }
    if let Some(fields) = e.fields {
        for (name, v) in fields {
            let text = match v {
                J::String(s) => s.clone(),
                J::Bool(b) => if *b { "True".into() } else { "False".into() },
                J::Number(x) => n(x.as_f64().unwrap_or(0.0)),
                J::Null => "None".into(),
                _ => continue,
            };
            ed.set(track, &format!("{at}.{name}"), &text)?;
        }
    }
    let mut index = key;
    if let Some(time) = e.time {
        for l in &lists {
            if times(ed, track, l)?.len() > key {
                ed.set(track, &format!("{l}[{key}].{}", time_member(l)), &n(time))?;
            }
        }
        // `SetKeyframeTime(bUpdateOrder)`: keep the keys sorted.
        let ts = times(ed, track, curve)?;
        let others: Vec<f64> = ts.iter().enumerate().filter(|(k, _)| *k != key).map(|(_, t)| *t).collect();
        let to = others.iter().take_while(|t| **t <= time).count();
        move_key(ed, track, &lists, key, to)?;
        index = to;
    }
    retangent_all(ed, track, &lists)?;
    Ok(index)
}

/// `matinee.addKey {track, curve, time, value?, mode?}`: a key at `time` in
/// every parallel list, inserted in time order (`FInterpCurve::AddPoint`):
/// curves get `value` or what they evaluate to at `time` and `mode`
/// (default `CIM_CurveAutoClamped`), timed keys a copy of the key before
/// (or the default key of an empty list). Returns its index.
pub fn add_key(ed: &mut Editor, track: usize, curve: &str, time: f64, value: Option<&J>, mode: Option<&str>) -> Result<usize> {
    let class = ed.pkg.class_name_of(PackageIndex::export(track));
    let lists = linked(&class, curve);
    let ts = times(ed, track, curve)?;
    let idx = ts.iter().take_while(|t| **t <= time).count();
    let mode = Mode::parse(mode.unwrap_or("CIM_CurveAutoClamped"));
    for l in &lists {
        let (points, comps) = if is_curve_path(l) { read_points(ed, track, l)? } else { (Vec::new(), &[][..]) };
        let count = times(ed, track, l)?.len();
        if count == 0 {
            // An empty curve is not stored at all: add the property first.
            let top = l.split('.').next().unwrap_or(l);
            if ed.with_object(track, |pkg, o| prop(o, pkg, top).is_none())? {
                ed.add_property(track, "", top)?;
            }
            ed.edit_list(track, l, ListOp::Add)?;
        } else {
            let src = idx.saturating_sub(1).min(count - 1);
            ed.edit_list(track, l, ListOp::Duplicate(src))?;
            move_key(ed, track, std::slice::from_ref(l), src + 1, idx.min(count))?;
        }
        let at = format!("{l}[{idx}]");
        ed.set(track, &format!("{at}.{}", time_member(l)), &n(time))?;
        if is_curve_path(l) {
            let v = if l.eq_ignore_ascii_case(curve) { value.and_then(nums) } else { None };
            let v = v.unwrap_or_else(|| eval(&points, time, &vec![0.0; comps.len().max(1)]));
            write_vec(ed, track, &at, "OutVal", comps, &v)?;
            write_vec(ed, track, &at, "ArriveTangent", comps, &vec![0.0; comps.len().max(1)])?;
            write_vec(ed, track, &at, "LeaveTangent", comps, &vec![0.0; comps.len().max(1)])?;
            set_mode(ed, track, l, idx, mode)?;
        } else if l.to_ascii_lowercase().starts_with("lookuptrack") {
            ed.set(track, &format!("{at}.GroupName"), "None")?;
        }
    }
    retangent_all(ed, track, &lists)?;
    Ok(idx)
}

/// `matinee.removeKey {track, curve, key}`: out of every parallel list.
pub fn remove_key(ed: &mut Editor, track: usize, curve: &str, key: usize) -> Result<()> {
    let class = ed.pkg.class_name_of(PackageIndex::export(track));
    let lists = linked(&class, curve);
    for l in &lists {
        if times(ed, track, l)?.len() > key {
            ed.edit_list(track, l, ListOp::Remove(key))?;
        }
    }
    retangent_all(ed, track, &lists)
}

// ----- new objects -----------------------------------------------------------------

/// An import of script class `class` (`Engine.InterpTrackMove`…).
pub fn class_import(ed: &mut Editor, class: &str) -> Result<PackageIndex> {
    let db = ed.db;
    let cref = db.class(class).ok_or_else(|| Error::Format(format!("no class {class}")))?;
    let package = db.ctx.packages[cref.pkg].name.clone();
    let name = db.ctx.packages[cref.pkg].export_name(cref.export).to_string();
    // Content packages keep their names lower-case.
    let outer = ed.pkg.add_import("Core", "Package", PackageIndex::NONE, &package.to_ascii_lowercase());
    Ok(ed.pkg.add_import("Core", "Class", outer, &name.to_ascii_lowercase()))
}

/// `<class>_<n>`, unused under `outer`.
fn fresh_name(ed: &Editor, outer: PackageIndex, class: &str) -> String {
    let taken = |n: &str| ed.pkg.exports.iter().enumerate().any(|(j, e)| e.outer == outer && ed.pkg.object_name(PackageIndex::export(j)).eq_ignore_ascii_case(n));
    (0..).map(|k| format!("{}_{k}", class.to_ascii_lowercase())).find(|n| !taken(n)).unwrap()
}

/// A new object of `class` under `outer` with no properties of its own
/// (every value its class default), laid out like the cooker writes Matinee
/// objects: no archetype, `Transactional | LoadForClient | LoadForServer |
/// LoadForEdit`, NetIndex -1, a payload of the NetIndex and `None`.
pub fn new_object(ed: &mut Editor, class: &str, outer: usize) -> Result<usize> {
    use tempest_upk::flags::rf;
    let class_idx = class_import(ed, class)?;
    let outer_idx = PackageIndex::export(outer);
    let name = fresh_name(ed, outer_idx, class);
    let none = ed.pkg.intern_fname("None");
    let mut data = Vec::with_capacity(12);
    data.extend_from_slice(&(-1i32).to_le_bytes());
    data.extend_from_slice(&(none.index as i32).to_le_bytes());
    data.extend_from_slice(&(none.number as i32).to_le_bytes());
    let export = tempest_upk::ObjectExport {
        class: class_idx,
        outer: outer_idx,
        object_name: ed.pkg.intern_fname(&name),
        object_flags: rf::TRANSACTIONAL | rf::LOAD_FOR_CLIENT | rf::LOAD_FOR_SERVER | rf::LOAD_FOR_EDIT,
        ..Default::default()
    };
    Ok(ed.add_export(export, data))
}

fn push(ed: &mut Editor, i: usize, list: &str, value: usize) -> Result<()> {
    crate::kismet::push_ref(ed, i, list, value)
}

fn remove_ref(ed: &mut Editor, i: usize, list: &str, value: usize) -> Result<()> {
    let at = ed.with_object(i, |pkg, o| refs(prop(o, pkg, list)).iter().position(|x| *x == PackageIndex::export(value)))?;
    if let Some(k) = at {
        ed.edit_list(i, list, ListOp::Remove(k))?;
    }
    Ok(())
}

// ----- groups, tracks, connectors ----------------------------------------------------

/// Group colours for new groups (the editor picks `FColor::MakeRandomColor`).
const COLORS: [[u8; 3]; 8] = [[230, 90, 80], [90, 170, 230], [120, 200, 110], [230, 180, 60], [180, 110, 220], [70, 200, 190], [230, 120, 180], [160, 160, 160]];

/// `matinee.addGroup {name, director?}`: a new `InterpGroup` (or an
/// `InterpGroupDirector` with its director track, as the editor's "New
/// Director Group" makes) appended to `InterpGroups`. Group names are
/// unique in a Matinee (`FindGroupByName`).
pub fn add_group(ed: &mut Editor, action: Option<usize>, data: usize, name: &str, director: bool) -> Result<usize> {
    let groups = groups_of(ed, data)?;
    if !director && groups.iter().any(|&g| group_name(ed, g).eq_ignore_ascii_case(name)) {
        return Err(Error::Format(format!("there already is a group {name}")));
    }
    if director && groups.iter().any(|&g| ed.db.class_is_a(&ed.pkg.class_name_of(PackageIndex::export(g)), "InterpGroupDirector")) {
        return Err(Error::Format("a Matinee has one director group".into()));
    }
    let g = new_object(ed, if director { "InterpGroupDirector" } else { "InterpGroup" }, data)?;
    if !director {
        ed.set(g, "GroupName", &format!("'{name}'"))?;
        let c = COLORS[groups.len() % COLORS.len()];
        ed.set(g, "GroupColor.R", &c[0].to_string())?;
        ed.set(g, "GroupColor.G", &c[1].to_string())?;
        ed.set(g, "GroupColor.B", &c[2].to_string())?;
        ed.set(g, "GroupColor.A", "255")?;
    }
    push(ed, data, "InterpGroups", g)?;
    if director {
        add_track(ed, action, g, "InterpTrackDirector", None)?;
    }
    if let Some(a) = action {
        sync_connectors(ed, a, data)?;
    }
    Ok(g)
}

/// `matinee.removeGroup {group}`: out of `InterpGroups`, then deleted with
/// its tracks; its connector goes (the variables linked to it stay).
pub fn remove_group(ed: &mut Editor, action: Option<usize>, data: usize, g: usize) -> Result<()> {
    remove_ref(ed, data, "InterpGroups", g)?;
    ed.delete(g)?;
    if let Some(a) = action {
        sync_connectors(ed, a, data)?;
    }
    Ok(())
}

/// `matinee.setGroup {group, name?, color?}`: renaming keeps the group's
/// connector (and its links) and the director cuts and look-at targets
/// that named the group.
pub fn set_group(ed: &mut Editor, action: Option<usize>, data: usize, g: usize, name: Option<&str>, color: Option<&[f64]>) -> Result<()> {
    if let Some(c) = color {
        for (k, ch) in ["R", "G", "B"].iter().enumerate() {
            ed.set(g, &format!("GroupColor.{ch}"), &(c.get(k).copied().unwrap_or(0.0).clamp(0.0, 255.0) as u8).to_string())?;
        }
        ed.set(g, "GroupColor.A", "255")?;
    }
    let Some(new) = name else { return Ok(()) };
    let old = group_name(ed, g);
    if old == new {
        return Ok(());
    }
    let groups = groups_of(ed, data)?;
    if groups.iter().any(|&o| o != g && group_name(ed, o).eq_ignore_ascii_case(new)) {
        return Err(Error::Format(format!("there already is a group {new}")));
    }
    ed.set(g, "GroupName", &format!("'{new}'"))?;
    if let Some(a) = action {
        let descs = link_descs(ed, a, "VariableLinks")?;
        if let Some(k) = descs.iter().position(|d| d.eq_ignore_ascii_case(&old)) {
            ed.set(a, &format!("VariableLinks[{k}].LinkDesc"), &format!("\"{new}\""))?;
        }
    }
    // References by name: director cuts (`TargetCamGroup`), look-at groups.
    for o in groups {
        let tracks = ed.with_object(o, |pkg, ob| refs(prop(ob, pkg, "InterpTracks")))?;
        for t in tracks.iter().filter_map(|t| t.export_index()) {
            let (cuts, look) = ed.with_object(t, |pkg, ob| {
                let cuts: Vec<usize> = array_at(ob, pkg, "CutTrack").map(|c| c.iter().enumerate().filter(|(_, k)| name_of(member(k, pkg, "TargetCamGroup"), pkg).is_some_and(|n| n.eq_ignore_ascii_case(&old))).map(|(i, _)| i).collect()).unwrap_or_default();
                (cuts, name_of(prop(ob, pkg, "LookAtGroupName"), pkg).is_some_and(|n| n.eq_ignore_ascii_case(&old)))
            })?;
            for k in cuts {
                ed.set(t, &format!("CutTrack[{k}].TargetCamGroup"), &format!("'{new}'"))?;
            }
            if look {
                ed.set(t, "LookAtGroupName", &format!("'{new}'"))?;
            }
        }
    }
    if let Some(a) = action {
        sync_connectors(ed, a, data)?;
    }
    Ok(())
}

/// `matinee.addTrack {group, class, property?}`: a new track of `class`
/// appended to the group's `InterpTracks`, refused where the editor refuses
/// it (`bDirGroupOnly` outside a director group, a second `bOnePerGroup`
/// track). Move tracks are made `IMF_RelativeToInitial` with a zero key at
/// 0 (`SetTrackToSensibleDefault`, `AddKeyframe`); property tracks take
/// `PropertyName` and `TrackTitle`, material parameter tracks `ParamName`.
pub fn add_track(ed: &mut Editor, action: Option<usize>, g: usize, class: &str, property: Option<&str>) -> Result<usize> {
    let db = ed.db;
    if !db.class_is_a(class, "InterpTrack") {
        return Err(Error::Format(format!("{class} is not a Matinee track class")));
    }
    let cref = db.class(class).ok_or_else(|| Error::Format(format!("no class {class}")))?;
    let flag = |name: &str| matches!(db.cdo_scalar(cref, name), Some(crate::reflect::CdoValue::Bool(true)));
    let gclass = ed.pkg.class_name_of(PackageIndex::export(g));
    let is_dir_group = db.class_is_a(&gclass, "InterpGroupDirector");
    if flag("bDirGroupOnly") && !is_dir_group {
        return Err(Error::Format(format!("{class} only goes in a director group")));
    }
    if flag("bSubTrackOnly") {
        return Err(Error::Format(format!("{class} is only a sub-track of a split move track")));
    }
    let tracks = ed.with_object(g, |pkg, o| refs(prop(o, pkg, "InterpTracks")))?;
    if flag("bOnePerGroup") && tracks.iter().any(|t| ed.pkg.class_name_of(*t).eq_ignore_ascii_case(class)) {
        return Err(Error::Format(format!("a group has one {class}")));
    }
    let t = new_object(ed, class, g)?;
    if db.class_is_a(class, "InterpTrackMove") {
        ed.set(t, "MoveFrame", "imf_relativetoinitial")?;
        add_key(ed, t, "PosTrack.Points", 0.0, Some(&json!([0.0, 0.0, 0.0])), None)?;
    }
    if let Some(p) = property.filter(|p| !p.is_empty()) {
        let field = if db.class_is_a(class, "InterpTrackFloatMaterialParam") || db.class_is_a(class, "InterpTrackVectorMaterialParam") || db.class_is_a(class, "InterpTrackFloatParticleParam") { "ParamName" } else { "PropertyName" };
        ed.set(t, field, &format!("'{p}'"))?;
        if field == "PropertyName" {
            ed.set(t, "TrackTitle", &format!("\"{p}\""))?;
        }
    }
    push(ed, g, "InterpTracks", t)?;
    let _ = action;
    Ok(t)
}

/// `matinee.removeTrack {track}`: out of its group (or its move track's
/// `SubTracks`), then deleted.
pub fn remove_track(ed: &mut Editor, action: Option<usize>, data: usize, t: usize) -> Result<()> {
    let outer = ed.pkg.exports[t].outer.export_index().ok_or_else(|| Error::Format("the track has no group".into()))?;
    remove_ref(ed, outer, "InterpTracks", t)?;
    remove_ref(ed, outer, "SubTracks", t)?;
    ed.delete(t)?;
    if let Some(a) = action {
        sync_connectors(ed, a, data)?;
    }
    Ok(())
}

fn link_descs(ed: &mut Editor, a: usize, list: &str) -> Result<Vec<String>> {
    ed.with_object(a, |pkg, o| array_at(o, pkg, list).map(|l| l.iter().map(|x| name_of(member(x, pkg, "LinkDesc"), pkg).unwrap_or_default()).collect()).unwrap_or_default())
}

/// Every event name of the Matinee's event tracks (`UInterpData::GetAllEventNames`).
fn event_names(ed: &mut Editor, data: usize) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for g in groups_of(ed, data)? {
        let tracks = ed.with_object(g, |pkg, o| refs(prop(o, pkg, "InterpTracks")))?;
        for t in tracks.iter().filter_map(|t| t.export_index()) {
            if !ed.db.class_is_a(&ed.pkg.class_name_of(PackageIndex::export(t)), "InterpTrackEvent") {
                continue;
            }
            let names = ed.with_object(t, |pkg, o| array_at(o, pkg, "EventTrack").map(|k| k.iter().filter_map(|x| name_of(member(x, pkg, "EventName"), pkg)).collect::<Vec<_>>()).unwrap_or_default())?;
            for nm in names {
                if !nm.eq_ignore_ascii_case("none") && !out.iter().any(|o| o.eq_ignore_ascii_case(&nm)) {
                    out.push(nm);
                }
            }
        }
    }
    Ok(out)
}

/// `USeqAct_Interp::UpdateConnectorsFromData`: one variable connector per
/// group that is neither a director group nor a folder (`SeqVar_Object`,
/// 0..255 variables), one output per event name; connectors past the class
/// default's (one variable link "Data", two outputs "Completed" and
/// "Reversed" in the 8.1 `Default__SeqAct_Interp`) that match nothing go,
/// except exposed property links.
pub fn sync_connectors(ed: &mut Editor, a: usize, data: usize) -> Result<()> {
    const DEFAULT_VARS: usize = 1;
    const DEFAULT_OUTPUTS: usize = 2;
    let mut wanted: Vec<String> = Vec::new();
    for g in groups_of(ed, data)? {
        let class = ed.pkg.class_name_of(PackageIndex::export(g));
        let folder = ed.with_object(g, |pkg, o| bool_of(o, pkg, "bIsFolder"))?;
        if !ed.db.class_is_a(&class, "InterpGroupDirector") && !folder {
            wanted.push(group_name(ed, g));
        }
    }
    // Variable links.
    let vars = ed.with_object(a, |pkg, o| {
        array_at(o, pkg, "VariableLinks").map(|l| l.iter().map(|x| (name_of(member(x, pkg, "LinkDesc"), pkg).unwrap_or_default(), name_of(member(x, pkg, "PropertyName"), pkg).is_some_and(|p| !p.eq_ignore_ascii_case("none")))).collect::<Vec<_>>()).unwrap_or_default()
    })?;
    for (k, (desc, exposed)) in vars.iter().enumerate().rev() {
        if k >= DEFAULT_VARS && !exposed && !wanted.iter().any(|w| w.eq_ignore_ascii_case(desc)) {
            ed.edit_list(a, "VariableLinks", ListOp::Remove(k))?;
        }
    }
    for w in &wanted {
        let descs = link_descs(ed, a, "VariableLinks")?;
        if descs.iter().any(|d| d.eq_ignore_ascii_case(w)) {
            continue;
        }
        let k = descs.len();
        // A copy of the last connector, made a group connector.
        ed.edit_list(a, "VariableLinks", ListOp::Duplicate(k - 1))?;
        let at = format!("VariableLinks[{k}]");
        let class = class_import(ed, "SeqVar_Object")?;
        ed.set(a, &format!("{at}.ExpectedType"), &ref_text(&ed.pkg, class))?;
        ed.set(a, &format!("{at}.LinkDesc"), &format!("\"{w}\""))?;
        ed.set(a, &format!("{at}.MinVars"), "0")?;
        ed.set(a, &format!("{at}.MaxVars"), "255")?;
        let draw = ed.get(a, &format!("VariableLinks[{}].DrawX", k - 1)).ok().and_then(|s| s.trim().parse::<i64>().ok());
        if let Some(x) = draw {
            ed.set(a, &format!("{at}.DrawX"), &(x + 56).to_string())?;
        }
        let linked = ed.with_object(a, |pkg, o| array_at(o, pkg, "VariableLinks").and_then(|l| l.get(k)).and_then(|l| match member(l, pkg, "LinkedVariables") { Some(Value::Array(v)) => Some(v.len()), _ => None }).unwrap_or(0))?;
        for _ in 0..linked {
            ed.edit_list(a, &format!("{at}.LinkedVariables"), ListOp::Remove(0))?;
        }
    }
    // Outputs.
    let events = event_names(ed, data)?;
    let outs = link_descs(ed, a, "OutputLinks")?;
    for (k, desc) in outs.iter().enumerate().rev() {
        if k >= DEFAULT_OUTPUTS && !events.iter().any(|e| e.eq_ignore_ascii_case(desc)) {
            ed.edit_list(a, "OutputLinks", ListOp::Remove(k))?;
        }
    }
    for e in &events {
        let descs = link_descs(ed, a, "OutputLinks")?;
        if descs.iter().any(|d| d.eq_ignore_ascii_case(e)) {
            continue;
        }
        let k = descs.len();
        ed.edit_list(a, "OutputLinks", ListOp::Duplicate(k - 1))?;
        let at = format!("OutputLinks[{k}]");
        ed.set(a, &format!("{at}.LinkDesc"), &format!("\"{e}\""))?;
        let y = ed.get(a, &format!("OutputLinks[{}].DrawY", k - 1)).ok().and_then(|s| s.trim().parse::<i64>().ok());
        if let Some(y) = y {
            ed.set(a, &format!("{at}.DrawY"), &(y + 22).to_string())?;
        }
        let linked = ed.with_object(a, |pkg, o| array_at(o, pkg, "OutputLinks").and_then(|l| l.get(k)).and_then(|l| match member(l, pkg, "Links") { Some(Value::Array(v)) => Some(v.len()), _ => None }).unwrap_or(0))?;
        for _ in 0..linked {
            ed.edit_list(a, &format!("{at}.Links"), ListOp::Remove(0))?;
        }
    }
    Ok(())
}

/// Is export `i` the timeline's action, data, or inside its data?
fn part_of(ed: &Editor, action: Option<usize>, data: usize, i: usize) -> bool {
    if Some(i) == action || i == data {
        return true;
    }
    let mut cur = ed.pkg.exports.get(i).map(|e| e.outer).unwrap_or_default();
    while let Some(o) = cur.export_index() {
        if o == data {
            return true;
        }
        cur = ed.pkg.exports[o].outer;
    }
    false
}

/// `matinee.set {object, values: {Prop: text}}`: properties of the action
/// (`bLooping`, `PlayRate`…), the data (`InterpLength`, `EdSectionStart`…),
/// a group or a track (`bDisableTrack`, `MoveFrame`, `RotMode`,
/// `LookAtGroupName`, `TrackTitle`…), as text `Editor::set` takes.
pub fn set_props(ed: &mut Editor, action: Option<usize>, data: usize, object: usize, values: &Map<String, J>) -> Result<()> {
    if !part_of(ed, action, data, object) {
        return Err(Error::Format("that object is not part of this Matinee".into()));
    }
    let mut events = false;
    for (name, v) in values {
        let text = match v {
            J::String(s) => s.clone(),
            J::Bool(b) => if *b { "True".into() } else { "False".into() },
            J::Number(x) => n(x.as_f64().unwrap_or(0.0)),
            _ => return Err(Error::Format(format!("{name}: a value is text, a number or a boolean"))),
        };
        ed.set(object, name, &text)?;
        events |= name.to_ascii_lowercase().contains("event");
    }
    if let (Some(a), true) = (action, events) {
        sync_connectors(ed, a, data)?;
    }
    Ok(())
}

/// After a key edit of an event track, the action's outputs follow the event names.
pub fn after_key_edit(ed: &mut Editor, action: Option<usize>, data: usize, track: usize) -> Result<()> {
    if let Some(a) = action {
        if ed.db.class_is_a(&ed.pkg.class_name_of(PackageIndex::export(track)), "InterpTrackEvent") {
            sync_connectors(ed, a, data)?;
        }
    }
    Ok(())
}

/// The variable connector of a group.
fn group_connector(ed: &mut Editor, a: usize, data: usize, g: usize) -> Result<usize> {
    sync_connectors(ed, a, data)?;
    let name = group_name(ed, g);
    link_descs(ed, a, "VariableLinks")?.iter().position(|d| d.eq_ignore_ascii_case(&name)).ok_or_else(|| Error::Format(format!("group {name} has no connector (a director group or folder drives nothing)")))
}

/// `matinee.connect {group, actor, remove?}`: make `actor` (an export of
/// the level) one of what the group drives: a `SeqVar_Object` holding it in
/// the action's sequence (an existing one, else a new one like the
/// cooker's: `ObjValue`, `ObjInstanceVersion` 1, `ParentSequence`,
/// `DrawWidth`/`DrawHeight` 32) linked to the group's connector. `remove`
/// unlinks every variable of the connector holding the actor.
pub fn connect(ed: &mut Editor, action: usize, data: usize, g: usize, actor: usize, remove: bool) -> Result<()> {
    let k = group_connector(ed, action, data, g)?;
    let target = PackageIndex::export(actor);
    let linked = ed.with_object(action, |pkg, o| array_at(o, pkg, "VariableLinks").and_then(|l| l.get(k)).map(|l| refs(member(l, pkg, "LinkedVariables"))).unwrap_or_default())?;
    let holds = |ed: &mut Editor, v: usize| ed.with_object(v, |pkg, o| refs(prop(o, pkg, "ObjValue")).first() == Some(&target)).unwrap_or(false);
    if remove {
        for v in linked.iter().filter_map(|v| v.export_index()) {
            if holds(ed, v) {
                crate::kismet::link(ed, &json!({ "from": action, "variable": k, "to": v, "remove": true }))?;
            }
        }
        return Ok(());
    }
    for v in linked.iter().filter_map(|v| v.export_index()) {
        if holds(ed, v) {
            return Ok(());
        }
    }
    let seq = ed.with_object(action, |pkg, o| refs(prop(o, pkg, "ParentSequence")).first().and_then(|x| x.export_index()))?.ok_or_else(|| Error::Format("the action is in no sequence".into()))?;
    let objects = ed.with_object(seq, |pkg, o| refs(prop(o, pkg, "SequenceObjects")))?;
    let mut var = None;
    for o in objects.iter().filter_map(|o| o.export_index()) {
        if ed.pkg.class_name_of(PackageIndex::export(o)).eq_ignore_ascii_case("SeqVar_Object") && holds(ed, o) {
            var = Some(o);
            break;
        }
    }
    let var = match var {
        Some(v) => v,
        None => {
            let v = new_object(ed, "SeqVar_Object", seq)?;
            ed.set(v, "ObjValue", &ref_text(&ed.pkg, target))?;
            seq_object_props(ed, v, seq)?;
            push(ed, seq, "SequenceObjects", v)?;
            v
        }
    };
    crate::kismet::link(ed, &json!({ "from": action, "variable": k, "to": var }))?;
    Ok(())
}

/// What every cooked sequence variable/data object stores besides its value.
fn seq_object_props(ed: &mut Editor, v: usize, seq: usize) -> Result<()> {
    ed.set(v, "ObjInstanceVersion", "1")?;
    ed.set(v, "ParentSequence", &ref_text(&ed.pkg, PackageIndex::export(seq)))?;
    ed.set(v, "DrawWidth", "32")?;
    ed.set(v, "DrawHeight", "32")
}

/// Make export `a` (a fresh copy of a `SeqAct_Interp`, already adopted into
/// sequence `seq` by `kismet::adopt`) a new, empty Matinee: the example's
/// settings and its group/event connectors dropped (connectors back to the
/// class default's count), and a new `InterpData` of `length` seconds in the
/// sequence linked to its "Data" connector. Returns the data.
pub fn init_new(ed: &mut Editor, a: usize, seq: usize, length: f64) -> Result<usize> {
    let keep = ["inputlinks", "outputlinks", "variablelinks", "objinstanceversion", "parentsequence", "drawwidth", "drawheight"];
    let mut obj = ed.object(a)?.clone();
    obj.properties.retain(|p| keep.contains(&ed.pkg.fname_to_string(p.name).to_ascii_lowercase().as_str()));
    ed.replace_object(a, obj)?;
    for (list, count) in [("VariableLinks", 1usize), ("OutputLinks", 2)] {
        let n = link_descs(ed, a, list)?.len();
        for k in (count..n).rev() {
            ed.edit_list(a, list, ListOp::Remove(k))?;
        }
    }
    let data = new_object(ed, "InterpData", seq)?;
    ed.set(data, "InterpLength", &n(length))?;
    ed.set(data, "EdSectionEnd", &n(length))?;
    seq_object_props(ed, data, seq)?;
    push(ed, seq, "SequenceObjects", data)?;
    crate::kismet::link(ed, &json!({ "from": a, "variable": 0, "to": data }))?;
    Ok(data)
}
