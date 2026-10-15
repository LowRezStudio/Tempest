//! Property layout (`UStruct::Link` / `UProperty::Link`) for the 64-bit
//! engine. Sizes drive return-value eating; linking also sets the
//! `CPF_NeedCtorLink` flags that end up serialized in property flags.

use tempest_upk::flags::cpf;

use super::engine::Compiler;
use super::model::FieldRef;
use super::scope::FieldKind;
use crate::objects::{ObjectBody, PropertyKind};

fn align(v: i32, a: i32) -> i32 {
    if a <= 1 {
        v
    } else {
        (v + a - 1) / a * a
    }
}

impl<'a> Compiler<'a> {
    /// Does the struct have a constructor link (any property, including
    /// inherited ones, flagged `CPF_NeedCtorLink`)?
    pub fn struct_has_ctor_link(&mut self, s: FieldRef) -> bool {
        match s {
            FieldRef::Local(n) => {
                self.struct_layout(s);
                self.model.node(n).strct.has_ctor_link
            }
            FieldRef::External(_) => {
                let r = self.res();
                let mut cur = Some(s);
                let mut guard = 0;
                while let Some(st) = cur {
                    for ch in r.children(st) {
                        if r.is_property(ch) && r.prop_flags(ch) & cpf::NEED_CTOR_LINK != 0 {
                            return true;
                        }
                    }
                    cur = r.inheritance_super(st);
                    guard += 1;
                    if guard > 100 {
                        break;
                    }
                }
                false
            }
        }
    }

    /// `UProperty::Link` for one property: returns `(element_size, min_alignment)`
    /// and applies the flag side effects on local nodes.
    pub fn property_layout(&mut self, p: FieldRef, owner_struct: Option<FieldRef>) -> (i32, i32) {
        let kind = self.res().prop_kind(p).unwrap_or(PropertyKind::Int);
        let flags = self.res().prop_flags(p);
        let mut new_flags = flags;
        let layout = match kind {
            PropertyKind::Byte => (1, 1),
            PropertyKind::Int | PropertyKind::Float | PropertyKind::Bool => (4, 4),
            PropertyKind::Name => (8, 4),
            PropertyKind::Class | PropertyKind::Component => (8, 8),
            PropertyKind::Object | PropertyKind::SafeObject => {
                if flags & cpf::EDIT_INLINE != 0 && flags & cpf::EXPORT_OBJECT != 0 && flags & cpf::COMPONENT == 0 {
                    new_flags |= cpf::NEED_CTOR_LINK;
                }
                (8, 8)
            }
            PropertyKind::Interface => {
                new_flags &= !(cpf::NEED_CTOR_LINK | cpf::EXPORT_OBJECT);
                (16, 8)
            }
            PropertyKind::Delegate => {
                new_flags |= cpf::NEED_CTOR_LINK;
                (16, 8)
            }
            PropertyKind::Str => {
                if flags & cpf::NATIVE == 0 {
                    new_flags |= cpf::NEED_CTOR_LINK;
                }
                (16, 8)
            }
            PropertyKind::Array => {
                if let Some(inner) = self.res().prop_refs(p).0 {
                    self.property_layout(inner, owner_struct);
                }
                if flags & cpf::NATIVE == 0 {
                    new_flags |= cpf::NEED_CTOR_LINK;
                }
                (16, 8)
            }
            PropertyKind::Map => {
                if flags & cpf::NATIVE == 0 {
                    new_flags |= cpf::NEED_CTOR_LINK;
                }
                (80, 8)
            }
            PropertyKind::Struct => {
                let Some(s) = self.res().prop_refs(p).0 else { return (0, 4) };
                let recursive = owner_struct == Some(s);
                let (size, al) = if recursive { (0, 4) } else { self.struct_layout(s) };
                let ctor = if recursive { true } else { self.struct_has_ctor_link(s) };
                if ctor && flags & cpf::NATIVE == 0 {
                    new_flags |= cpf::NEED_CTOR_LINK;
                }
                (align(size, al), al)
            }
        };
        if new_flags != flags {
            if let FieldRef::Local(n) = p {
                self.model.node_mut(n).prop.flags = new_flags;
            }
        }
        if let FieldRef::Local(n) = p {
            self.model.node_mut(n).prop.element_size = layout.0;
        }
        layout
    }

    /// `(PropertiesSize, MinAlignment)` of a struct, class, state or function.
    pub fn struct_layout(&mut self, s: FieldRef) -> (i32, i32) {
        match s {
            FieldRef::External(o) => {
                if let Some(ObjectBody::ScriptStruct { .. }) | Some(ObjectBody::Class { .. }) | Some(ObjectBody::Function { .. }) | Some(ObjectBody::State { .. }) =
                    self.ctx.object(o).map(|x| &x.body)
                {
                    self.compute_layout(s)
                } else {
                    (0, 1)
                }
            }
            FieldRef::Local(n) => {
                let node = self.model.node(n);
                if node.strct.linked {
                    return (node.properties_size, node.min_alignment);
                }
                self.compute_layout(s)
            }
        }
    }

    /// `UStruct::Link(Ar, Props=TRUE)`; stores the result on local nodes.
    pub fn link_struct(&mut self, s: FieldRef) -> (i32, i32) {
        if let FieldRef::Local(n) = s {
            self.model.node_mut(n).strct.linked = false;
        }
        self.compute_layout(s)
    }

    fn compute_layout(&mut self, s: FieldRef) -> (i32, i32) {
        let r = self.res();
        let kind = r.kind_of(s);
        let name = r.name_of(s);
        let is_function = kind == FieldKind::Function;
        let inheritance_super = r.inheritance_super(s);
        let children = r.children(s);
        let (mut size, mut min_align) = (0, 1);
        if let Some(sup) = inheritance_super {
            let (ss, sa) = self.struct_layout(sup);
            size = align(ss, 4);
            min_align = sa;
        }
        let mut prev_bool: Option<(i32, u32)> = None;
        let mut has_ctor = inheritance_super.map(|sup| self.struct_has_ctor_link(sup)).unwrap_or(false);
        let owner_struct = if kind == FieldKind::ScriptStruct { Some(s) } else { None };
        for ch in children {
            if !self.res().is_property(ch) {
                continue;
            }
            let (elem, al) = self.property_layout(ch, owner_struct);
            let dim = self.res().prop_array_dim(ch).max(1);
            let pk = self.res().prop_kind(ch);
            let offset;
            if pk == Some(PropertyKind::Bool) {
                match prev_bool {
                    Some((off, mask)) if !is_function && mask != 0x8000_0000 => {
                        offset = off;
                        prev_bool = Some((off, mask << 1));
                    }
                    _ => {
                        offset = align(size, al);
                        prev_bool = Some((offset, 1));
                    }
                }
            } else {
                offset = align(size, al);
                prev_bool = None;
            }
            size = offset + elem * dim;
            min_align = min_align.max(al);
            if self.res().prop_flags(ch) & cpf::NEED_CTOR_LINK != 0 {
                has_ctor = true;
            }
            if let FieldRef::Local(n) = ch {
                self.model.node_mut(n).prop.offset = offset;
            }
        }
        match name.as_str() {
            "Matrix" | "Plane" | "SHVector" | "Vector4" | "Quat" => min_align = 16,
            "QWord" | "Double" | "Pointer" => {
                min_align = 8;
                size = 8;
            }
            "Color" => min_align = 4,
            _ => min_align = min_align.max(4),
        }
        if let FieldRef::Local(n) = s {
            let node = self.model.node_mut(n);
            node.properties_size = size;
            node.min_alignment = min_align;
            node.strct.has_ctor_link = has_ctor;
            node.strct.linked = true;
        }
        (size, min_align)
    }

    pub fn link_struct_size(&mut self, s: FieldRef) -> i32 {
        self.struct_layout(s).0
    }
}
