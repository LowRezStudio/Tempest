//! Kismet (UE3 visual scripting) as stored in cooked level packages: a
//! `Sequence` export (`Main_Sequence`, sub-sequences inside it) lists its
//! nodes in `SequenceObjects`. Ops link through `OutputLinks[].Links[]`
//! (`LinkedOp`, `InputLinkIdx`) to the `InputLinks` of other ops, and read
//! or write variables through `VariableLinks[].LinkedVariables`; events
//! fire from their `Originator` actor. Engine: `USequenceOp`,
//! `FSeqOpOutputLink`, `FSeqVarLink` (Sequence.h).
//!
//! The cooker strips the node positions (`ObjPosX`/`ObjPosY` are editor-only)
//! but keeps each connector's `DrawY` (inputs, outputs) and `DrawX`
//! (variables, events) and the node's `DrawWidth`/`DrawHeight`, so the
//! Studio rebuilds most of the layout from them.

use serde_json::{json, Value as J};
use tempest_upk::io::{Error, PackageIndex, Result};
use tempest_upk::Package;

use crate::edit::{Editor, ListOp};
use crate::map::prop;
use crate::object::Object;
use crate::reflect::ClassDb;
use crate::value::{StructValue, Value};

fn member<'a>(v: &'a Value, pkg: &Package, name: &str) -> Option<&'a Value> {
    match v {
        Value::Struct(StructValue::Tagged(ps)) => ps.iter().find(|p| p.array_index == 0 && pkg.fname_to_string(p.name).eq_ignore_ascii_case(name)).map(|p| &p.value),
        Value::Struct(StructValue::Binary(ms)) => ms.iter().find(|m| m.name.eq_ignore_ascii_case(name)).and_then(|m| m.elems.first()),
        _ => None,
    }
}

fn int(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Int(i) => Some(*i as i64),
        Value::Byte(b) => Some(*b as i64),
        Value::Float(f) => Some(*f as i64),
        _ => None,
    }
}

fn text(v: Option<&Value>, pkg: &Package) -> Option<String> {
    match v? {
        Value::Str(s) => Some(s.clone()),
        Value::Name(n) => Some(pkg.fname_to_string(*n)),
        _ => None,
    }
}

fn boolean(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Bool(true)))
}

fn refs(v: Option<&Value>) -> Vec<PackageIndex> {
    match v {
        Some(Value::Array(a)) => a.iter().filter_map(|x| if let Value::Object(i) = x { Some(*i) } else { None }).collect(),
        Some(Value::Object(i)) => vec![*i],
        _ => Vec::new(),
    }
}

fn array(v: Option<&Value>) -> &[Value] {
    match v {
        Some(Value::Array(a)) => a,
        _ => &[],
    }
}

/// What a node is, from its class.
pub fn node_kind(db: &ClassDb, class: &str) -> &'static str {
    let is = |b: &str| db.class_is_a(class, b);
    if is("SequenceFrame") {
        "frame"
    } else if is("Sequence") {
        "sequence"
    } else if is("SequenceEvent") {
        "event"
    } else if is("SequenceCondition") {
        "condition"
    } else if is("SequenceVariable") {
        "variable"
    } else if is("SequenceAction") || is("SequenceOp") {
        "action"
    } else {
        "other"
    }
}

fn reference(pkg: &Package, i: PackageIndex) -> J {
    if i.is_null() {
        return J::Null;
    }
    json!({ "export": i.export_index(), "path": pkg.object_path(i), "class": pkg.class_name_of(i) })
}

/// A variable's value as text (object, int, float, bool, string, name).
fn variable_value(o: &Object, pkg: &Package) -> Option<String> {
    for name in ["ObjValue", "IntValue", "FloatValue", "bValue", "StrValue", "NameValue", "VectValue", "ExternalVariableName", "VariableName"] {
        let Some(v) = prop(o, pkg, name) else { continue };
        return Some(match v {
            Value::Object(i) if !i.is_null() => pkg.object_name(*i),
            Value::Object(_) => "None".into(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Str(s) => format!("\"{s}\""),
            Value::Name(n) => pkg.fname_to_string(*n),
            Value::Struct(_) => {
                let c = |n| int(member(v, pkg, n)).unwrap_or(0);
                format!("({}, {}, {})", c("X"), c("Y"), c("Z"))
            }
            _ => continue,
        });
    }
    None
}

/// `kismet.sequences`: the sequences of a package (top level first).
pub fn sequences(ed: &mut Editor, db: &ClassDb) -> Vec<J> {
    let mut out = Vec::new();
    for i in 0..ed.pkg.exports.len() {
        let class = ed.pkg.class_name_of(PackageIndex::export(i));
        if !db.class_is_a(&class, "Sequence") || db.class_is_a(&class, "SequenceFrame") {
            continue;
        }
        let info = ed.with_object(i, |pkg, o| (refs(prop(o, pkg, "SequenceObjects")).len(), refs(prop(o, pkg, "ParentSequence")).first().copied().unwrap_or_default(), text(prop(o, pkg, "ObjName"), pkg)));
        let Ok((count, parent, title)) = info else { continue };
        out.push(json!({ "export": i, "name": ed.pkg.object_name(PackageIndex::export(i)), "path": ed.pkg.object_path(PackageIndex::export(i)), "class": class, "objects": count, "parent": parent.export_index(), "title": title }));
    }
    out.sort_by_key(|s| (s["parent"].is_number(), s["export"].as_u64()));
    out
}

/// `kismet.graph`: the nodes of one sequence with their connectors and links.
pub fn graph(ed: &mut Editor, db: &ClassDb, seq: usize) -> Result<J> {
    let objects = ed.with_object(seq, |pkg, o| refs(prop(o, pkg, "SequenceObjects")))?;
    let mut nodes = Vec::new();
    for idx in objects {
        let Some(i) = idx.export_index() else { continue };
        let class = ed.pkg.class_name_of(idx);
        let kind = node_kind(db, &class);
        let node = ed.with_object(i, |pkg, o| {
            let inputs: Vec<J> = array(prop(o, pkg, "InputLinks"))
                .iter()
                .map(|l| json!({ "desc": text(member(l, pkg, "LinkDesc"), pkg).unwrap_or_default(), "drawY": int(member(l, pkg, "DrawY")), "hidden": boolean(member(l, pkg, "bHidden")), "disabled": boolean(member(l, pkg, "bDisabled")) }))
                .collect();
            let outputs: Vec<J> = array(prop(o, pkg, "OutputLinks"))
                .iter()
                .map(|l| {
                    let links: Vec<J> = array(member(l, pkg, "Links"))
                        .iter()
                        .map(|k| {
                            let op = refs(member(k, pkg, "LinkedOp")).first().copied().unwrap_or_default();
                            json!({ "op": op.export_index(), "input": int(member(k, pkg, "InputLinkIdx")).unwrap_or(0) })
                        })
                        .collect();
                    json!({ "desc": text(member(l, pkg, "LinkDesc"), pkg).unwrap_or_default(), "drawY": int(member(l, pkg, "DrawY")), "hidden": boolean(member(l, pkg, "bHidden")), "delay": member(l, pkg, "ActivateDelay").and_then(|v| if let Value::Float(f) = v { Some(*f) } else { None }), "links": links })
                })
                .collect();
            let vars: Vec<J> = array(prop(o, pkg, "VariableLinks"))
                .iter()
                .map(|l| {
                    let ty = refs(member(l, pkg, "ExpectedType")).first().map(|t| pkg.object_name(*t));
                    json!({
                        "desc": text(member(l, pkg, "LinkDesc"), pkg).unwrap_or_default(),
                        "drawX": int(member(l, pkg, "DrawX")),
                        "hidden": boolean(member(l, pkg, "bHidden")),
                        "writeable": boolean(member(l, pkg, "bWriteable")),
                        "type": ty,
                        "property": text(member(l, pkg, "PropertyName"), pkg),
                        "linked": refs(member(l, pkg, "LinkedVariables")).iter().map(|v| v.export_index()).collect::<Vec<_>>(),
                    })
                })
                .collect();
            let events: Vec<J> = array(prop(o, pkg, "EventLinks"))
                .iter()
                .map(|l| json!({ "desc": text(member(l, pkg, "LinkDesc"), pkg).unwrap_or_default(), "drawX": int(member(l, pkg, "DrawX")), "linked": refs(member(l, pkg, "LinkedEvents")).iter().map(|v| v.export_index()).collect::<Vec<_>>() }))
                .collect();
            let originator = refs(prop(o, pkg, "Originator")).first().map(|x| reference(pkg, *x)).unwrap_or(J::Null);
            json!({
                "export": i,
                "name": pkg.object_name(idx),
                "class": class,
                "kind": kind,
                "title": text(prop(o, pkg, "ObjName"), pkg),
                "comment": text(prop(o, pkg, "ObjComment"), pkg),
                "width": int(prop(o, pkg, "DrawWidth")),
                "height": int(prop(o, pkg, "DrawHeight")),
                "posX": int(prop(o, pkg, "ObjPosX")),
                "posY": int(prop(o, pkg, "ObjPosY")),
                "sizeX": int(prop(o, pkg, "SizeX")),
                "sizeY": int(prop(o, pkg, "SizeY")),
                "inputs": inputs,
                "outputs": outputs,
                "variables": vars,
                "events": events,
                "value": if kind == "variable" { variable_value(o, pkg) } else { None },
                "originator": originator,
                "enabled": prop(o, pkg, "bEnabled").map(|v| matches!(v, Value::Bool(true))),
                "maxTrigger": int(prop(o, pkg, "MaxTriggerCount")),
                "subObjects": if kind == "sequence" { refs(prop(o, pkg, "SequenceObjects")).len() } else { 0 },
            })
        })?;
        nodes.push(node);
    }
    Ok(json!({ "sequence": seq, "name": ed.pkg.object_name(PackageIndex::export(seq)), "nodes": nodes }))
}

/// The text `Editor::set` takes for an object value.
pub(crate) fn object_text(pkg: &Package, i: usize) -> String {
    let idx = PackageIndex::export(i);
    format!("{}'{}'", pkg.class_name_of(idx), pkg.object_path(idx))
}

/// `kismet.link`: connect or disconnect.
/// - `{from, output, to, input}`: output `output` of op `from` to input
///   `input` of op `to` (`OutputLinks[output].Links`);
/// - `{from, variable, to}`: variable connector `variable` of `from` to the
///   variable node `to` (`VariableLinks[variable].LinkedVariables`).
///
/// `remove: true` takes the link out. Nothing changes when it already
/// is as asked.
pub fn link(ed: &mut Editor, p: &J) -> Result<bool> {
    let from = p.get("from").and_then(|v| v.as_u64()).ok_or_else(|| Error::Format("missing from".into()))? as usize;
    let to = p.get("to").and_then(|v| v.as_u64()).ok_or_else(|| Error::Format("missing to".into()))? as usize;
    let remove = p.get("remove").and_then(|v| v.as_bool()).unwrap_or(false);
    let target = PackageIndex::export(to);
    if let Some(var) = p.get("variable").and_then(|v| v.as_u64()) {
        let list = format!("VariableLinks[{var}].LinkedVariables");
        let current: Vec<PackageIndex> = ed.with_object(from, |pkg, o| array(prop(o, pkg, "VariableLinks")).get(var as usize).map(|l| refs(member(l, pkg, "LinkedVariables"))))?.ok_or_else(|| Error::Format(format!("no variable connector {var}")))?;
        let at = current.iter().position(|x| *x == target);
        return match (remove, at) {
            (true, Some(k)) => {
                ed.edit_list(from, &list, ListOp::Remove(k))?;
                Ok(true)
            }
            (false, None) => {
                ed.edit_list(from, &list, ListOp::Add)?;
                let text = object_text(&ed.pkg, to);
                ed.set(from, &format!("{list}[{}]", current.len()), &text)?;
                Ok(true)
            }
            _ => Ok(false),
        };
    }
    let output = p.get("output").and_then(|v| v.as_u64()).ok_or_else(|| Error::Format("missing output or variable".into()))?;
    let input = p.get("input").and_then(|v| v.as_u64()).unwrap_or(0) as i64;
    let list = format!("OutputLinks[{output}].Links");
    let current: Vec<(PackageIndex, i64)> = ed
        .with_object(from, |pkg, o| {
            array(prop(o, pkg, "OutputLinks")).get(output as usize).map(|l| array(member(l, pkg, "Links")).iter().map(|k| (refs(member(k, pkg, "LinkedOp")).first().copied().unwrap_or_default(), int(member(k, pkg, "InputLinkIdx")).unwrap_or(0))).collect())
        })?
        .ok_or_else(|| Error::Format(format!("no output {output}")))?;
    let at = current.iter().position(|(op, i)| *op == target && *i == input);
    match (remove, at) {
        (true, Some(k)) => {
            ed.edit_list(from, &list, ListOp::Remove(k))?;
            Ok(true)
        }
        (false, None) => {
            let n = current.len();
            ed.edit_list(from, &list, ListOp::Add)?;
            let text = object_text(&ed.pkg, to);
            ed.set(from, &format!("{list}[{n}].LinkedOp"), &text)?;
            ed.set(from, &format!("{list}[{n}].InputLinkIdx"), &input.to_string())?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

// ----- adding and removing nodes -----------------------------------------------

fn struct_props(v: &mut Value) -> Option<&mut Vec<crate::value::Property>> {
    match v {
        Value::Struct(StructValue::Tagged(ps)) => Some(ps),
        _ => None,
    }
}

/// Empty every link list of a node (outputs, variables, events), its event
/// originator, title and comment: a copy starts unconnected.
fn clear_links(obj: &mut Object, pkg: &Package) {
    // The example's title and comment are about the example.
    obj.properties.retain(|p| !["objname", "objcomment"].contains(&pkg.fname_to_string(p.name).to_ascii_lowercase().as_str()));
    for p in obj.properties.iter_mut() {
        let name = pkg.fname_to_string(p.name).to_ascii_lowercase();
        let member = match name.as_str() {
            "outputlinks" => "links",
            "variablelinks" => "linkedvariables",
            "eventlinks" => "linkedevents",
            "originator" => {
                p.value = Value::Object(PackageIndex::default());
                continue;
            }
            _ => continue,
        };
        let Value::Array(items) = &mut p.value else { continue };
        for item in items.iter_mut() {
            let Some(ps) = struct_props(item) else { continue };
            for q in ps.iter_mut() {
                if pkg.fname_to_string(q.name).eq_ignore_ascii_case(member) {
                    if let Value::Array(a) = &mut q.value {
                        a.clear();
                    }
                }
            }
        }
    }
}

/// A name for a new node under `outer`: `<class>_<n>`, unused there.
fn fresh_name(ed: &Editor, outer: PackageIndex, class: &str) -> String {
    let taken = |n: &str| ed.pkg.exports.iter().enumerate().any(|(j, e)| e.outer == outer && ed.pkg.object_name(PackageIndex::export(j)).eq_ignore_ascii_case(n));
    let mut k = 0;
    loop {
        let name = format!("{}_{k}", class.to_ascii_lowercase());
        if !taken(&name) {
            return name;
        }
        k += 1;
    }
}

/// Append `value` (an export) to the object array `prop` of `i`, creating
/// the property when the object does not store it.
pub(crate) fn push_ref(ed: &mut Editor, i: usize, prop: &str, value: usize) -> Result<()> {
    let count = ed.with_object(i, |pkg, o| match prop_of(o, pkg, prop) {
        Some(Value::Array(a)) => Some(a.len()),
        _ => None,
    })?;
    let n = match count {
        Some(n) => n,
        None => {
            ed.add_property(i, "", prop)?;
            0
        }
    };
    ed.edit_list(i, prop, ListOp::Add)?;
    let text = object_text(&ed.pkg, value);
    ed.set(i, &format!("{prop}[{n}]"), &text)
}

fn prop_of<'a>(o: &'a Object, pkg: &Package, name: &str) -> Option<&'a Value> {
    prop(o, pkg, name)
}

/// Make export `i` (a fresh copy of a node) a node of sequence `seq`: outer
/// and `ParentSequence` the sequence, links emptied, connectors placed so the
/// Studio lays it out at (`x`, `y`), listed in `SequenceObjects`; an event
/// gets `originator` (an actor export, also given the event in its
/// `GeneratedEvents`), an object variable `object` as its value.
pub fn adopt(ed: &mut Editor, i: usize, seq: usize, x: i64, y: i64, originator: Option<usize>, object: Option<usize>) -> Result<()> {
    let seq_idx = PackageIndex::export(seq);
    let class = ed.pkg.class_name_of(PackageIndex::export(i));
    let name = fresh_name(ed, seq_idx, &class);
    ed.export_mut(i)?.outer = seq_idx;
    ed.rename(i, &name)?;
    let pkg_clone_free = {
        let mut obj = ed.object(i)?.clone();
        clear_links(&mut obj, &ed.pkg);
        obj
    };
    ed.replace_object(i, pkg_clone_free)?;
    ed.set(i, "ParentSequence", &object_text(&ed.pkg, seq))?;
    // Connectors where the layout expects them (see the Studio's kismet-layout.ts).
    let (inputs, outputs, vars, events, width) = ed.with_object(i, |pkg, o| {
        let w = int(prop(o, pkg, "DrawWidth")).unwrap_or(140);
        (array(prop(o, pkg, "InputLinks")).len(), array(prop(o, pkg, "OutputLinks")).len(), array(prop(o, pkg, "VariableLinks")).len(), array(prop(o, pkg, "EventLinks")).len(), w)
    })?;
    const TITLE: i64 = 22;
    const ROW: i64 = 20;
    for k in 0..inputs {
        ed.set(i, &format!("InputLinks[{k}].DrawY"), &(y + TITLE + ROW / 2 + k as i64 * ROW).to_string())?;
    }
    for k in 0..outputs {
        ed.set(i, &format!("OutputLinks[{k}].DrawY"), &(y + TITLE + ROW / 2 + k as i64 * ROW).to_string())?;
    }
    let count = (vars + events) as i64;
    for k in 0..vars {
        ed.set(i, &format!("VariableLinks[{k}].DrawX"), &(x + (k as i64 + 1) * width / (count + 1)).to_string())?;
    }
    for k in 0..events {
        ed.set(i, &format!("EventLinks[{k}].DrawX"), &(x + (vars + k) as i64 * width / (count + 1) + width / (count + 1)).to_string())?;
    }
    if let Some(a) = originator {
        ed.set(i, "Originator", &object_text(&ed.pkg, a))?;
        push_ref(ed, a, "GeneratedEvents", i)?;
    }
    if let Some(v) = object {
        ed.set(i, "ObjValue", &object_text(&ed.pkg, v))?;
    }
    push_ref(ed, seq, "SequenceObjects", i)?;
    ed.clear_net_index(i)?;
    Ok(())
}

/// `kismet.remove`: take node `node` out of sequence `seq`: out of
/// `SequenceObjects`, every link to it in the sequence's other nodes, its
/// originator's `GeneratedEvents`; then the export is deleted.
pub fn remove(ed: &mut Editor, seq: usize, node: usize) -> Result<()> {
    let target = PackageIndex::export(node);
    let objects = ed.with_object(seq, |pkg, o| refs(prop(o, pkg, "SequenceObjects")))?;
    let at = objects.iter().position(|x| *x == target).ok_or_else(|| Error::Format("the node is not in this sequence".into()))?;
    ed.edit_list(seq, "SequenceObjects", ListOp::Remove(at))?;
    for other in objects.iter().filter_map(|x| x.export_index()).filter(|&j| j != node) {
        let mut obj = match ed.object(other) {
            Ok(o) => o.clone(),
            Err(_) => continue,
        };
        let mut changed = false;
        for p in obj.properties.iter_mut() {
            let name = ed.pkg.fname_to_string(p.name).to_ascii_lowercase();
            let Value::Array(items) = &mut p.value else { continue };
            for item in items.iter_mut() {
                let Some(ps) = struct_props(item) else { continue };
                for q in ps.iter_mut() {
                    let qn = ed.pkg.fname_to_string(q.name).to_ascii_lowercase();
                    let Value::Array(a) = &mut q.value else { continue };
                    let before = a.len();
                    match (name.as_str(), qn.as_str()) {
                        ("outputlinks", "links") => a.retain(|l| {
                            let Value::Struct(StructValue::Tagged(lp)) = l else { return true };
                            !lp.iter().any(|f| ed.pkg.fname_to_string(f.name).eq_ignore_ascii_case("LinkedOp") && f.value == Value::Object(target))
                        }),
                        ("variablelinks", "linkedvariables") | ("eventlinks", "linkedevents") => a.retain(|v| *v != Value::Object(target)),
                        _ => {}
                    }
                    changed |= a.len() != before;
                }
            }
        }
        if changed {
            ed.replace_object(other, obj)?;
        }
    }
    let originator = ed.with_object(node, |pkg, o| refs(prop(o, pkg, "Originator")).first().and_then(|x| x.export_index()))?;
    if let Some(a) = originator {
        let at = ed.with_object(a, |pkg, o| refs(prop(o, pkg, "GeneratedEvents")).iter().position(|x| *x == target))?;
        if let Some(k) = at {
            ed.edit_list(a, "GeneratedEvents", ListOp::Remove(k))?;
        }
    }
    ed.delete(node)?;
    Ok(())
}
