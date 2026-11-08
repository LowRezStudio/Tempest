//! What the views show, read from the backend's replies (the same JSON the
//! Electron Studio receives, see `serve.rs` / `json.rs`).

use std::collections::HashSet;

use serde_json::Value as J;

fn str_of(v: &J, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string()
}

/// One package file of the workspace folder.
#[derive(Clone, Debug)]
pub struct PackageEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub script: bool,
    pub map: bool,
}

pub fn packages(v: &J) -> Vec<PackageEntry> {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|p| PackageEntry {
            name: str_of(p, "name"),
            path: str_of(p, "path"),
            size: p.get("size").and_then(|x| x.as_u64()).unwrap_or(0),
            script: p.get("script").and_then(|x| x.as_bool()).unwrap_or(false),
            map: p.get("map").and_then(|x| x.as_bool()).unwrap_or(false),
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct Export {
    pub name: String,
    pub class: String,
    pub size: u64,
}

/// An open package's export table as a tree (outers as parents).
pub struct Tables {
    pub path: String,
    pub name: String,
    pub exports: Vec<Export>,
    /// `children[0]`: the top-level exports; `children[i + 1]`: those of export `i`.
    pub children: Vec<Vec<usize>>,
    /// The parent of each export in the tree (`None` at the top).
    pub parent: Vec<Option<usize>>,
}

impl Tables {
    /// `package.open` / `package.tables` (column form, `json::tables_compact`).
    pub fn from_json(path: &str, v: &J) -> Tables {
        let ex = &v["exports"];
        let col = |k: &str| ex.get(k).and_then(|x| x.as_array()).cloned().unwrap_or_default();
        let names: Vec<String> = col("names").iter().map(|x| x.as_str().unwrap_or_default().to_string()).collect();
        let classes: Vec<i64> = col("classes").iter().map(|x| x.as_i64().unwrap_or(0)).collect();
        let outers: Vec<i64> = col("outers").iter().map(|x| x.as_i64().unwrap_or(0)).collect();
        let sizes: Vec<u64> = col("sizes").iter().map(|x| x.as_u64().unwrap_or(0)).collect();
        let imports: Vec<String> = v["imports"].as_array().into_iter().flatten().map(|i| str_of(i, "name")).collect();
        let class_name = |c: i64| -> String {
            if c < 0 {
                imports.get((-c - 1) as usize).cloned().unwrap_or_default()
            } else if c > 0 {
                names.get(c as usize - 1).cloned().unwrap_or_default()
            } else {
                "class".into()
            }
        };
        let n = names.len();
        let exports: Vec<Export> = (0..n).map(|i| Export { name: names[i].clone(), class: class_name(classes.get(i).copied().unwrap_or(0)), size: sizes.get(i).copied().unwrap_or(0) }).collect();
        let mut children = vec![Vec::new(); n + 1];
        let mut parent = vec![None; n];
        for (i, up) in parent.iter_mut().enumerate() {
            let o = outers.get(i).copied().unwrap_or(0);
            let p = if o > 0 && (o as usize) <= n && o as usize != i + 1 { o as usize } else { 0 };
            children[p].push(i);
            *up = (p > 0).then(|| p - 1);
        }
        for kids in &mut children {
            // Groups first, then by name, like the Electron export tree.
            kids.sort_by_cached_key(|&i| (exports[i].class != "package", exports[i].name.to_ascii_lowercase()));
        }
        Tables { path: path.to_string(), name: str_of(v, "name"), exports, children, parent }
    }

    /// The rows the tree shows: (export, depth), children of expanded rows.
    pub fn visible(&self, expanded: &HashSet<usize>, filter: &str) -> Vec<(usize, u16)> {
        let mut out = Vec::new();
        if !filter.is_empty() {
            // A filter lists every match flat.
            let f = filter.to_ascii_lowercase();
            out.extend((0..self.exports.len()).filter(|&i| self.exports[i].name.to_ascii_lowercase().contains(&f) || self.exports[i].class.to_ascii_lowercase().contains(&f)).map(|i| (i, 0)));
            return out;
        }
        // Empty `None` placeholders (cooked leftovers with no class or data) are left out of the tree.
        let shown = |i: usize| !(self.exports[i].size == 0 && self.exports[i].name.eq_ignore_ascii_case("none"));
        let mut stack: Vec<(usize, u16)> = self.children[0].iter().rev().filter(|&&i| shown(i)).map(|&i| (i, 0)).collect();
        while let Some((i, d)) = stack.pop() {
            out.push((i, d));
            if expanded.contains(&i) {
                stack.extend(self.children[i + 1].iter().rev().filter(|&&c| shown(c)).map(|&c| (c, d + 1)));
            }
        }
        out
    }

    /// `Package.Group.Name`: the path other packages and the search index use.
    pub fn object_path(&self, i: usize) -> String {
        let mut parts = vec![self.exports[i].name.clone()];
        let mut p = self.parent[i];
        while let Some(q) = p {
            parts.push(self.exports[q].name.clone());
            p = self.parent[q];
        }
        parts.push(self.name.trim_end_matches(".upk").to_string());
        parts.reverse();
        parts.join(".")
    }

    pub fn has_children(&self, i: usize) -> bool {
        !self.children[i + 1].is_empty()
    }
}

/// How a value is edited in place.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Edit {
    #[default]
    None,
    Bool(bool),
    /// Free text `object.set` parses; the draft it starts from.
    Text(String),
}

/// What a row is to the array operations (`object.list`).
#[derive(Clone, Debug, PartialEq, Default)]
pub enum ListRole {
    #[default]
    None,
    /// A dynamic array (elements can be added).
    Array,
    /// Element `at` of the array at `array`.
    Element { array: String, at: usize },
}

/// One line of the inspector: a property, a struct member or an array element.
#[derive(Clone, Debug, Default)]
pub struct PropRow {
    pub depth: u16,
    pub name: String,
    pub kind: String,
    pub value: String,
    /// Its path for `object.set` (`Outer.Inner[2].Leaf`).
    pub path: String,
    pub edit: Edit,
    pub list: ListRole,
    /// A property stored on the object or in a tagged struct (can be reset to its default).
    pub resettable: bool,
}

/// The editor for a scalar value: what `object.set` accepts (the Electron
/// inspector's `ScalarEditor` sends the same text).
fn edit_of(v: &J) -> Edit {
    let x = &v["v"];
    match v.get("t").and_then(|t| t.as_str()).unwrap_or_default() {
        "bool" => Edit::Bool(x.as_bool().unwrap_or(false)),
        "byte" | "int" | "float" => Edit::Text(x.to_string()),
        "enum" | "name" | "str" => Edit::Text(x.as_str().unwrap_or_default().to_string()),
        "object" => Edit::Text(if x["ref"].as_i64().unwrap_or(0) == 0 { "None".into() } else { str_of(x, "path") }),
        _ => Edit::None,
    }
}

fn scalar(v: &J) -> Option<String> {
    let t = v.get("t")?.as_str()?;
    let x = &v["v"];
    Some(match t {
        "bool" | "byte" | "int" | "float" => x.to_string(),
        "enum" | "name" => x.as_str().unwrap_or_default().to_string(),
        "str" => format!("\"{}\"", x.as_str().unwrap_or_default()),
        "object" => str_of(x, "path"),
        "delegate" => format!("{}.{}", str_of(&x["object"], "path"), str_of(x, "function")),
        "raw" => format!("{} bytes", x["len"]),
        _ => return None,
    })
}

fn push_value(out: &mut Vec<PropRow>, depth: u16, name: String, kind: String, path: String, v: &J) {
    if let Some(s) = scalar(v) {
        out.push(PropRow { depth, name, kind, value: s, edit: edit_of(v), path, ..Default::default() });
        return;
    }
    let t = v.get("t").and_then(|t| t.as_str()).unwrap_or_default();
    let items = v["v"].as_array().cloned().unwrap_or_default();
    match t {
        "struct" => {
            out.push(PropRow { depth, name, kind, value: String::new(), path: path.clone(), ..Default::default() });
            for p in &items {
                push_property(out, depth + 1, &path, p);
            }
        }
        "binary" => {
            out.push(PropRow { depth, name, kind, value: String::new(), path: path.clone(), ..Default::default() });
            for m in &items {
                let elems = m["elems"].as_array().cloned().unwrap_or_default();
                let mt = str_of(m, "type");
                let mn = str_of(m, "name");
                if elems.len() == 1 {
                    push_value(out, depth + 1, mn.clone(), mt, format!("{path}.{mn}"), &elems[0]);
                } else {
                    for (k, e) in elems.iter().enumerate() {
                        push_value(out, depth + 1, format!("{mn}[{k}]"), mt.clone(), format!("{path}.{mn}[{k}]"), e);
                    }
                }
            }
        }
        "array" => {
            let len = v.get("len").and_then(|l| l.as_u64()).unwrap_or(items.len() as u64);
            out.push(PropRow { depth, name, kind, value: format!("{len} elements"), path: path.clone(), list: ListRole::Array, ..Default::default() });
            for (k, e) in items.iter().enumerate() {
                let at = out.len();
                push_value(out, depth + 1, format!("[{k}]"), String::new(), format!("{path}[{k}]"), e);
                out[at].list = ListRole::Element { array: path.clone(), at: k };
            }
        }
        _ => out.push(PropRow { depth, name, kind, value: v.to_string(), path, ..Default::default() }),
    }
}

/// A property (`Name` or `Name[i]` for a static-array slot) under `parent`.
fn push_property(out: &mut Vec<PropRow>, depth: u16, parent: &str, p: &J) {
    let index = p.get("index").and_then(|i| i.as_i64()).unwrap_or(0);
    let name = if index > 0 { format!("{}[{index}]", str_of(p, "name")) } else { str_of(p, "name") };
    let kind = match (p.get("struct").and_then(|s| s.as_str()), p.get("enum").and_then(|s| s.as_str())) {
        (Some(s), _) => s.to_string(),
        (_, Some(e)) => e.to_string(),
        _ => str_of(p, "type").trim_end_matches("Property").to_string(),
    };
    let path = if parent.is_empty() { name.clone() } else { format!("{parent}.{name}") };
    let at = out.len();
    push_value(out, depth, name, kind, path, &p["value"]);
    out[at].resettable = true;
}

/// The inspector's view of an export (`object.get`).
pub struct Inspected {
    pub path: String,
    pub class: String,
    pub kind: String,
    pub rows: Vec<PropRow>,
    /// The native part of the object as text (meshes, textures, levels…).
    pub native: String,
}

pub fn inspected(v: &J) -> Inspected {
    let mut rows = Vec::new();
    for p in v["properties"].as_array().into_iter().flatten() {
        push_property(&mut rows, 0, "", p);
    }
    let native = match v.get("native") {
        Some(n) if !n["text"].as_str().unwrap_or_default().is_empty() => format!("{} ({} bytes)\n{}", str_of(n, "type"), n["size"], str_of(n, "text")),
        _ => v.get("text").and_then(|t| t.as_str()).unwrap_or_default().to_string(),
    };
    Inspected { path: str_of(v, "path"), class: str_of(v, "class"), kind: str_of(v, "kind"), rows, native }
}

pub fn human_size(n: u64) -> String {
    match n {
        n if n >= 1 << 30 => format!("{:.1} GB", n as f64 / (1u64 << 30) as f64),
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / (1u64 << 20) as f64),
        n if n >= 1 << 10 => format!("{:.0} KB", n as f64 / 1024.0),
        n => format!("{n} B"),
    }
}

/// An object reference as the backend sends it (`json::reference`).
#[derive(Clone, Debug)]
pub struct Ref {
    /// `PackageIndex`: > 0 export `ref - 1`, < 0 an import, 0 none.
    pub index: i64,
    pub path: String,
    pub class: String,
}

pub fn reference(v: &J) -> Ref {
    Ref { index: v["ref"].as_i64().unwrap_or(0), path: str_of(v, "path"), class: str_of(v, "class") }
}

/// `object.deps`: the table links, what the object references, what refers to it.
pub struct Deps {
    pub links: Vec<(&'static str, Ref)>,
    pub references: Vec<Ref>,
    pub referrers: Vec<Ref>,
}

pub fn deps(v: &J) -> Deps {
    let list = |k: &str| v[k].as_array().into_iter().flatten().map(reference).collect();
    let links = ["class", "outer", "archetype", "super"].into_iter().map(|k| (k, reference(&v[k]))).filter(|(_, r)| r.index != 0).collect();
    Deps { links, references: list("references"), referrers: list("referrers") }
}
