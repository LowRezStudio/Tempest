//! ActionScript 2: every script of a movie (frame actions of the root and of
//! sprites, `DoInitAction` class definitions, clip event handlers and button
//! actions), the bytecode as editable P-code (`action`, `pcode`), and an
//! ActionScript decompiler and compiler (`decompile`, `compile`) over a
//! shared syntax tree (`ast`).

pub mod action;
pub mod ast;
pub mod class;
pub mod compile;
pub mod decompile;
pub mod parse;
pub mod pcode;
pub mod print;
pub mod verify;

use serde_json::{json, Value as J};
use tempest_upk::io::{Error, Result};

use super::{display, parse_tags, u16_at, u32_at, write_tags, Movie, Tag};

/// Where a script lives. `sprite` 0 is the root timeline; `tag` indexes its tag stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Loc {
    /// `DoAction` or `DoInitAction`.
    Tag { sprite: u16, tag: usize },
    /// The `event`th clip action record of a `PlaceObject2/3`.
    Clip { sprite: u16, tag: usize, event: usize },
    /// The `cond`th condition of a `DefineButton2` (0 for the actions of a `DefineButton`).
    Button { tag: usize, cond: usize },
}

impl Loc {
    pub fn to_json(&self) -> J {
        match *self {
            Loc::Tag { sprite, tag } => json!({ "sprite": sprite, "tag": tag }),
            Loc::Clip { sprite, tag, event } => json!({ "sprite": sprite, "tag": tag, "event": event }),
            Loc::Button { tag, cond } => json!({ "tag": tag, "button": cond }),
        }
    }

    pub fn from_json(v: &J) -> Result<Loc> {
        let n = |k: &str| v.get(k).and_then(|x| x.as_u64());
        let tag = n("tag").ok_or_else(|| Error::Format("script location without tag".into()))? as usize;
        let sprite = n("sprite").unwrap_or(0) as u16;
        Ok(if let Some(cond) = n("button") {
            Loc::Button { tag, cond: cond as usize }
        } else if let Some(event) = n("event") {
            Loc::Clip { sprite, tag, event: event as usize }
        } else {
            Loc::Tag { sprite, tag }
        })
    }

    /// A short key: `12`, `s40:3`, `s40:3e1`, `b7:0`.
    pub fn key(&self) -> String {
        match *self {
            Loc::Tag { sprite: 0, tag } => format!("{tag}"),
            Loc::Tag { sprite, tag } => format!("s{sprite}:{tag}"),
            Loc::Clip { sprite, tag, event } => format!("s{sprite}:{tag}e{event}"),
            Loc::Button { tag, cond } => format!("b{tag}:{cond}"),
        }
    }
}

/// Clip event names by bit (`CLIPEVENTFLAGS`, SWF 6+ layout).
pub const CLIP_EVENTS: [(u32, &str); 19] = [
    (0x01, "load"),
    (0x02, "enterFrame"),
    (0x04, "unload"),
    (0x08, "mouseMove"),
    (0x10, "mouseDown"),
    (0x20, "mouseUp"),
    (0x40, "keyDown"),
    (0x80, "keyUp"),
    (0x100, "data"),
    (0x200, "initialize"),
    (0x400, "press"),
    (0x800, "release"),
    (0x1000, "releaseOutside"),
    (0x2000, "rollOver"),
    (0x4000, "rollOut"),
    (0x8000, "dragOver"),
    (0x10000, "dragOut"),
    (0x20000, "keyPress"),
    (0x40000, "construct"),
];

pub fn clip_event_names(flags: u32) -> String {
    CLIP_EVENTS.iter().filter(|(b, _)| flags & b != 0).map(|(_, n)| *n).collect::<Vec<_>>().join(", ")
}

/// Button condition names by bit (`BUTTONCONDACTION`; the key code sits in bits 9..15).
const BUTTON_CONDS: [(u16, &str); 9] = [
    (0x0001, "rollOver"),
    (0x0002, "rollOut"),
    (0x0004, "press"),
    (0x0008, "release"),
    (0x0010, "dragOut"),
    (0x0020, "dragOver"),
    (0x0040, "releaseOutside"),
    (0x0080, "dragOver (menu)"),
    (0x0100, "dragOut (menu)"),
];

/// One clip action record: event flags, key code, and the byte range of its actions in the tag.
struct ClipRecord {
    flags: u32,
    key: Option<u8>,
    /// Where the record's `ActionRecordSize` field is, and the actions' range.
    size_at: usize,
    actions: std::ops::Range<usize>,
}

fn clip_records(t: &Tag, version: u8) -> Option<Vec<ClipRecord>> {
    let p = display::Place::parse(t.code, &t.data)?;
    let mut at = p.clip_actions_at(t.data.len())?;
    let wide = version >= 6;
    let d = &t.data;
    at += 2 + if wide { 4 } else { 2 };
    let mut out = Vec::new();
    loop {
        let flags = if wide { u32_at(d, at)? } else { u16_at(d, at)? as u32 };
        at += if wide { 4 } else { 2 };
        if flags == 0 {
            break;
        }
        let size = u32_at(d, at)? as usize;
        let size_at = at;
        at += 4;
        let end = at + size;
        let mut key = None;
        if flags & 0x20000 != 0 {
            key = Some(*d.get(at)?);
            at += 1;
        }
        if end > d.len() || at > end {
            return None;
        }
        out.push(ClipRecord { flags, key, size_at, actions: at..end });
        at = end;
    }
    Some(out)
}

/// The conditions of a `DefineButton2` as `(flags, actions range, size field offset)`.
fn button_conds(t: &Tag) -> Option<Vec<(u16, std::ops::Range<usize>, usize)>> {
    let d = &t.data;
    if t.code == 7 {
        // DefineButton: id, button records up to a 0 flag byte, then the actions.
        let mut at = 2;
        while *d.get(at)? != 0 {
            // Records: flags u8, id u16, depth u16, matrix.
            at += 5;
            let mut br = super::BitReader { d: d.get(at..)?, pos: 0 };
            display::Matrix::read(&mut br);
            at += br.byte_pos();
        }
        return Some(vec![(0, at + 1..d.len(), usize::MAX)]);
    }
    let off = u16_at(d, 3)? as usize;
    if off == 0 {
        return Some(Vec::new());
    }
    let mut at = 3 + off;
    let mut out = Vec::new();
    loop {
        let size = u16_at(d, at)? as usize;
        let flags = u16_at(d, at + 2)?;
        let start = at + 4;
        let end = if size == 0 { d.len() } else { at + size };
        if end > d.len() || start > end {
            return None;
        }
        out.push((flags, start..end, at));
        if size == 0 {
            break;
        }
        at = end;
    }
    Some(out)
}

/// One script of a movie.
pub struct ScriptRef {
    pub loc: Loc,
    /// `DoInitAction`'s sprite, the clip's event names, the button condition.
    pub label: String,
    pub kind: &'static str,
    pub bytes: Vec<u8>,
    /// Clip actions: the placement's depth, character and instance name.
    pub placed: Option<(u16, Option<u16>, Option<String>)>,
    /// The frame (from 0) of the timeline the tag is on.
    pub frame: usize,
}

fn stream_scripts(tags: &[Tag], sprite: u16, version: u8, out: &mut Vec<ScriptRef>) {
    let mut frame = 0;
    for (i, t) in tags.iter().enumerate() {
        let first = out.len();
        match t.code {
            1 => frame += 1,
            12 => out.push(ScriptRef { loc: Loc::Tag { sprite, tag: i }, label: String::new(), kind: "DoAction", bytes: t.data.clone(), placed: None, frame: 0 }),
            59 => out.push(ScriptRef {
                loc: Loc::Tag { sprite, tag: i },
                label: u16_at(&t.data, 0).map(|s| s.to_string()).unwrap_or_default(),
                kind: "DoInitAction",
                bytes: t.data.get(2..).unwrap_or(&[]).to_vec(),
                placed: None,
                frame: 0,
            }),
            26 | 70 => {
                let placed = display::Place::parse(t.code, &t.data).map(|p| (p.depth, p.character, p.name.clone()));
                for (e, r) in clip_records(t, version).unwrap_or_default().into_iter().enumerate() {
                    let mut label = clip_event_names(r.flags);
                    if let Some(k) = r.key {
                        label.push_str(&format!(" (key {k})"));
                    }
                    out.push(ScriptRef { loc: Loc::Clip { sprite, tag: i, event: e }, label, kind: "ClipAction", bytes: t.data[r.actions].to_vec(), placed: placed.clone(), frame: 0 });
                }
            }
            7 | 34 if sprite == 0 => {
                for (c, (flags, range, _)) in button_conds(t).unwrap_or_default().into_iter().enumerate() {
                    let mut label: Vec<String> = BUTTON_CONDS.iter().filter(|(b, _)| flags & b != 0).map(|(_, n)| n.to_string()).collect();
                    if flags >> 9 != 0 {
                        label.push(format!("key {}", flags >> 9));
                    }
                    out.push(ScriptRef { loc: Loc::Button { tag: i, cond: c }, label: label.join(", "), kind: "ButtonAction", bytes: t.data[range].to_vec(), placed: None, frame: 0 });
                }
            }
            39 => {
                if let (Some(id), Ok(inner)) = (u16_at(&t.data, 0), parse_tags(t.data.get(4..).unwrap_or(&[]))) {
                    stream_scripts(&inner, id, version, out);
                }
                continue;
            }
            _ => {}
        }
        for s in &mut out[first..] {
            s.frame = frame;
        }
    }
}

/// Every script of the movie, in tag order (sprite scripts where the sprite is defined).
pub fn scripts(m: &Movie) -> Vec<ScriptRef> {
    let mut out = Vec::new();
    stream_scripts(&m.tags, 0, m.version, &mut out);
    out
}

pub fn script_bytes(m: &Movie, loc: &Loc) -> Result<Vec<u8>> {
    scripts_at(m, loc).ok_or_else(|| Error::Format(format!("no script at {}", loc.key())))
}

fn scripts_at(m: &Movie, loc: &Loc) -> Option<Vec<u8>> {
    let sprite = match loc {
        Loc::Tag { sprite, .. } | Loc::Clip { sprite, .. } => *sprite,
        Loc::Button { .. } => 0,
    };
    let mut out = Vec::new();
    if sprite == 0 {
        stream_scripts(&m.tags, 0, m.version, &mut out);
    } else {
        let t = m.tags.iter().find(|t| t.code == 39 && u16_at(&t.data, 0) == Some(sprite))?;
        stream_scripts(&parse_tags(t.data.get(4..)?).ok()?, sprite, m.version, &mut out);
    }
    out.into_iter().find(|s| s.loc == *loc).map(|s| s.bytes)
}

/// Replace the actions of the script at `loc` with `bytes` (a whole action
/// stream, `End` included).
pub fn replace(m: &mut Movie, loc: &Loc, bytes: &[u8]) -> Result<()> {
    let version = m.version;
    let set = |t: &mut Tag| -> Result<()> {
        match *loc {
            Loc::Tag { .. } => match t.code {
                12 => t.data = bytes.to_vec(),
                59 => {
                    let mut d = t.data[..2].to_vec();
                    d.extend_from_slice(bytes);
                    t.data = d;
                }
                _ => return Err(Error::Format("not a script tag".into())),
            },
            Loc::Clip { event, .. } => {
                let recs = clip_records(t, version).ok_or_else(|| Error::Format("no clip actions".into()))?;
                let r = recs.get(event).ok_or_else(|| Error::Format(format!("no clip event {event}")))?;
                let start = r.size_at + 4;
                let mut d = t.data[..r.actions.start].to_vec();
                d.extend_from_slice(bytes);
                d.extend_from_slice(&t.data[r.actions.end..]);
                let size = (r.actions.start - start + bytes.len()) as u32;
                d[r.size_at..r.size_at + 4].copy_from_slice(&size.to_le_bytes());
                t.data = d;
            }
            Loc::Button { cond, .. } => {
                let conds = button_conds(t).ok_or_else(|| Error::Format("no button actions".into()))?;
                let (_, range, size_at) = conds.get(cond).cloned().ok_or_else(|| Error::Format(format!("no button condition {cond}")))?;
                let mut d = t.data[..range.start].to_vec();
                d.extend_from_slice(bytes);
                d.extend_from_slice(&t.data[range.end..]);
                if size_at != usize::MAX && cond + 1 < conds.len() {
                    let size = u16::try_from(range.start - size_at + bytes.len()).map_err(|_| Error::Format("button actions larger than 64 KiB".into()))?;
                    d[size_at..size_at + 2].copy_from_slice(&size.to_le_bytes());
                }
                t.data = d;
            }
        }
        t.long = t.long || t.data.len() >= 0x3F;
        Ok(())
    };
    let (sprite, tag) = match *loc {
        Loc::Tag { sprite, tag } | Loc::Clip { sprite, tag, .. } => (sprite, tag),
        Loc::Button { tag, .. } => (0, tag),
    };
    if sprite == 0 {
        let t = m.tags.get_mut(tag).ok_or_else(|| Error::Format(format!("no tag {tag}")))?;
        return set(t);
    }
    let t = m.tags.iter_mut().find(|t| t.code == 39 && u16_at(&t.data, 0) == Some(sprite)).ok_or_else(|| Error::Format(format!("no sprite {sprite}")))?;
    let mut body = parse_tags(t.data.get(4..).unwrap_or(&[]))?;
    set(body.get_mut(tag).ok_or_else(|| Error::Format(format!("no tag {tag} in sprite {sprite}")))?)?;
    let mut data = t.data[..4].to_vec();
    write_tags(&body, &mut data);
    t.data = data;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::action::{Action, Script, Value};
    use super::{compile, decompile, pcode};

    /// Compile, check the bytecode survives encoding and P-code, and that decompiling gives `expect`.
    fn round(src: &str, expect: &str) -> Script {
        let s = compile::compile(src).unwrap_or_else(|e| panic!("{e}\n{src}"));
        let bytes = s.encode().unwrap();
        assert_eq!(Script::decode(&bytes).unwrap(), s);
        assert_eq!(pcode::parse(&pcode::print(&s)).unwrap().encode().unwrap(), bytes);
        let out = decompile::source(&s);
        assert_eq!(out.trim(), expect.trim(), "\n{}", pcode::print(&s));
        // And again: a fixpoint.
        assert_eq!(decompile::source(&compile::compile(&out).unwrap()), out);
        s
    }

    fn same(src: &str) -> Script {
        round(src, src)
    }

    #[test]
    fn statements_round_trip() {
        same("trace(\"hi\");\nstop();\ngotoAndPlay(3);\n");
        same("var count = 0;\nif (count > 2) {\n    count -= 1;\n} else if (count == 1) {\n    trace(count);\n} else {\n    count += 3;\n}\n");
        same("while (x < 10) {\n    x++;\n    if (x == 5) {\n        break;\n    }\n}\n");
        same("do {\n    y--;\n} while (y > 0);\n");
        same("for (k in obj) {\n    trace(k);\n}\n");
        same("switch (mode) {\n    case 1:\n        a = 2;\n        break;\n    case \"two\":\n    case 3:\n        b = 1;\n        break;\n    default:\n        c = 0;\n}\n");
        same("try {\n    risky();\n} catch (e) {\n    trace(e);\n}\n");
        same("with (clip) {\n    gotoAndStop(1);\n}\n");
    }

    #[test]
    fn expressions_round_trip() {
        same("x = a && b || !c;\n");
        same("x = a ? b : c + 1;\n");
        same("o.list[i] = [1, 2.5, \"s\", null, undefined, true];\n");
        same("o = {b: 1, a: \"two\"};\n");
        same("v = a.b.c(d, e)(f);\n");
        same("n = new Foo(1);\nm = new a.b.Bar();\n");
        same("t = typeof x + -y + ~z;\n");
        same("delete o.p;\ndelete q;\n");
        same("a.x = b.y = 3;\n");
        round("r = (x >= 1) != (y <= 2);\n", "r = x >= 1 != y <= 2;");
        same("s = eval(\"_root.a\");\nset(\"_root.b\", 1);\n");
        same("c = __cast(TextField, f);\n");
    }

    #[test]
    fn functions_use_registers() {
        let s = round("f = function (a, b) {\n    var t = a + b;\n    return t * this.k;\n};\n", "f = function (a, b) {\n    var _loc4_ = a + b;\n    return _loc4_ * this.k;\n};");
        let Some(Action::Function(f)) = s.actions.iter().find(|a| matches!(a, Action::Function(_))) else { panic!() };
        let (_, flags) = f.v2.unwrap();
        assert!(flags & super::action::PRELOAD_THIS != 0);
        assert!(f.params.iter().all(|(r, _)| *r != 0));
        // A local read by a nested function stays a variable.
        let s = compile::compile("f = function () {\n    var shared = 1;\n    return function () {\n        return shared;\n    };\n};\n").unwrap();
        let text = pcode::print(&s);
        assert!(text.contains("DefineLocal"), "{text}");
    }

    #[test]
    fn decompiled_names_avoid_variables() {
        // A register parameter named like a variable a nested function reads by name is renamed.
        let src = "f = function (x) {\n    return function () {\n        return x;\n    };\n};\n";
        let s = compile::compile(src).unwrap();
        assert_eq!(decompile::source(&s).trim(), src.trim());
    }

    #[test]
    fn forin_break_and_return() {
        let src = "f = function (o) {\n    for (var k in o) {\n        if (k == \"a\") {\n            break;\n        }\n        if (k == \"b\") {\n            return k;\n        }\n    }\n    return null;\n};\n";
        round(src, &src.replace("k ", "_loc2_ ").replace("k)", "_loc2_)").replace("return k;", "return _loc2_;"));
    }

    #[test]
    fn classes_round_trip() {
        same(
            "class a.b.Thing extends MovieClip {\n    function Thing(n) {\n        super();\n        this.n = n;\n    }\n    function grow(by) {\n        this.n += by;\n    }\n    static function make() {\n        return new a.b.Thing(1);\n    }\n    function get size() {\n        return this.n;\n    }\n    function set size(value) {\n        this.n = value;\n    }\n    static var LIMIT = 10;\n    var n = 0;\n}\n",
        );
        same("interface a.IThing {\n}\n");
    }

    #[test]
    fn pcode_blocks_compile() {
        let s = compile::compile("trace(1);\n__pcode {\n    Push \"x\", 2\n    SetVariable\n}\n").unwrap();
        assert!(s.actions.iter().any(|a| matches!(a, Action::Push(v) if v.first() == Some(&Value::Str("x".into())))));
    }

    #[test]
    fn errors_name_the_line() {
        let e = compile::compile("a = 1;\nb = (2;\n").unwrap_err().to_string();
        assert!(e.contains("line 2"), "{e}");
    }
}
