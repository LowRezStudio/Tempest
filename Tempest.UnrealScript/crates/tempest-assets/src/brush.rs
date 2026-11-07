//! Brush geometry the way UnrealEd builds it, for volumes (blocking,
//! trigger, reverb… any `Brush` actor): the brush builders, the brush's own
//! BSP and the convex hulls the engine collides with.
//!
//! A volume's shape lives in two places. `Brush` → `Model` (+ `Polys`) is the
//! editor's polygon soup with a BSP built from it (`FBSPOps::RebuildBrush`);
//! `BrushComponent.BrushAggGeom.ConvexElems` is what gameplay checks against
//! (`UBrushComponent::LineCheck`/`PointCheck` → `FKAggregateGeom`), computed
//! from that BSP by `KModelToHulls` (every solid leaf's planes → one convex
//! hull, `FKConvexElem::HullFromPlanes` → `GenerateHullData`).
//!
//! Ported from UE3 10897: `UBrushBuilder.cpp` + the UnrealScript builders
//! (CubeBuilder, CylinderBuilder, ConeBuilder, TetrahedronBuilder),
//! `UnFPoly.cpp` (Init, Finalize, Fix, SplitWithPlane, SplitWithPlaneFast,
//! Split), `BSPOps.cpp` (bspBuild, SplitPolyList, FindBestSplit, bspAddNode,
//! bspRefresh, bspBuildBounds/FilterBound, bspValidateBrush),
//! `UnPhysUtils.cpp` (KModelToHulls) and `UnPhysCollision.cpp`
//! (HullFromPlanes, GenerateHullData, PermuteVertexData). Floats are `f32`
//! like the engine's. One deviation: `GenerateHullData` asks PhysX
//! (`NxCookConvexMesh`) for the hull's vertices and triangles; here an
//! incremental hull gives them (vertices in input order), then the rest
//! (edges, face normals, planes, box, SIMD copy) is the engine's code.

pub type V = [f32; 3];

pub fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn scale(a: V, s: f32) -> V {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: V, b: V) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
/// `FVector::operator^`.
pub fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn size_sq(a: V) -> f32 {
    dot(a, a)
}
fn size(a: V) -> f32 {
    size_sq(a).sqrt()
}
fn is_zero(a: V) -> bool {
    a == [0.0; 3]
}
/// `FVector::SafeNormal` (the editor's `appInvSqrt` is an SSE estimate plus
/// one Newton step; this is the exact reciprocal, within an ulp or two).
pub fn safe_normal(a: V) -> V {
    let s = size_sq(a);
    if s == 1.0 {
        a
    } else if s < 1e-8 {
        [0.0; 3]
    } else {
        scale(a, 1.0 / s.sqrt())
    }
}
/// `FPlane(Base, Normal)`.
fn plane(base: V, normal: V) -> [f32; 4] {
    [normal[0], normal[1], normal[2], dot(base, normal)]
}
fn plane_dot(p: [f32; 4], v: V) -> f32 {
    p[0] * v[0] + p[1] * v[1] + p[2] * v[2] - p[3]
}
fn pnormal(p: [f32; 4]) -> V {
    [p[0], p[1], p[2]]
}
/// `FPointPlaneDist`.
fn point_plane_dist(point: V, base: V, normal: V) -> f32 {
    dot(sub(point, base), normal)
}
/// `FVector::FindBestAxisVectors`.
fn find_best_axis_vectors(n: V) -> (V, V) {
    let (nx, ny, nz) = (n[0].abs(), n[1].abs(), n[2].abs());
    let a1 = if nz > nx && nz > ny { [1.0, 0.0, 0.0] } else { [0.0, 0.0, 1.0] };
    let a1 = safe_normal(sub(a1, scale(n, dot(a1, n))));
    (a1, cross(a1, n))
}

const THRESH_POINTS_ARE_SAME: f32 = 0.002;
const THRESH_POINTS_ARE_NEAR: f32 = 0.015;
const THRESH_NORMALS_ARE_SAME: f32 = 0.00002;
const THRESH_VECTORS_ARE_NEAR: f32 = 0.0004;
const THRESH_SPLIT_POLY_WITH_PLANE: f32 = 0.25;
const THRESH_SPLIT_POLY_PRECISELY: f32 = 0.01;
const THRESH_ZERO_NORM_SQUARED: f32 = 0.0001;
const WORLD_MAX: f32 = 524288.0;
const HALF_WORLD_MAX: f32 = 262144.0;
/// `MIN_HULL_VERT_DISTANCE` = 0.1 · `U2PScale` (0.02).
const MIN_HULL_VERT_DISTANCE: f32 = 0.1 * 0.02;
const DIST_COMPARE_THRESH: f32 = 0.1;
const DIR_COMPARE_THRESH: f32 = 0.0003;
const LOCAL_EPS: f32 = 0.01;

// Poly flags (UnModel.h).
pub const PF_INVISIBLE: u32 = 0x1;
pub const PF_NOT_SOLID: u32 = 0x8;
pub const PF_SEMISOLID: u32 = 0x20;
pub const PF_TWO_SIDED: u32 = 0x100;
pub const PF_PORTAL: u32 = 0x0400_0000;
const PF_ED_CUT: u32 = 0x8000_0000;
const PF_ADD_LAST: u32 = PF_SEMISOLID | PF_NOT_SOLID;
const PF_NO_ADD_TO_BSP: u32 = 0x8000_0000 | 0x4000_0000 | 0x0200_0000 | 0x1000_0000 | 0x0100_0000;
/// `PF_DefaultFlags` = AcceptsLights | AcceptsDynamicLights | ForceLightMap.
pub const PF_DEFAULT_FLAGS: u32 = 0x400 | 0x800 | 0x200;
const NF_NOT_CSG: u8 = 0x01;
const NF_NOT_VIS_BLOCKING: u8 = 0x04;
const NF_IS_NEW: u8 = 0x20;
const INDEX_NONE: i32 = -1;

// ----- FPoly -------------------------------------------------------------------

/// `FPoly` (the parts geometry needs; the rest is written at the defaults
/// `FPoly::Init` gives: no material, shadow map scale 32, lighting channels
/// BSP|initialised, Lightmass defaults).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EdPoly {
    pub base: V,
    pub normal: V,
    pub texture_u: V,
    pub texture_v: V,
    pub vertices: Vec<V>,
    pub poly_flags: u32,
    /// `ItemName` (builders name faces: `Wall`, `Cap`, `Top`…); empty = None.
    pub item_name: String,
    pub i_link: i32,
    pub i_brush_poly: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Split {
    Coplanar,
    Front,
    Back,
    Split,
}

impl EdPoly {
    /// `FPoly::Init`.
    pub fn init() -> EdPoly {
        EdPoly { poly_flags: PF_DEFAULT_FLAGS, i_link: INDEX_NONE, i_brush_poly: INDEX_NONE, ..Default::default() }
    }

    /// `FPoly::Reverse`.
    pub fn reverse(&mut self) {
        self.normal = scale(self.normal, -1.0);
        self.vertices.reverse();
    }

    /// `FPoly::Fix`: drop vertices identical to the previous one; fewer than
    /// three left empties the poly.
    pub fn fix(&mut self) -> usize {
        let n = self.vertices.len();
        if n == 0 {
            return 0;
        }
        let mut j = 0;
        let mut prev = n - 1;
        for i in 0..n {
            if !points_are_same(self.vertices[i], self.vertices[prev]) {
                if j != i {
                    self.vertices[j] = self.vertices[i];
                }
                prev = j;
                j += 1;
            }
        }
        if j < 3 {
            self.vertices.clear();
        } else {
            self.vertices.truncate(j);
        }
        self.vertices.len()
    }

    /// `FPoly::Area`.
    pub fn area(&self) -> f32 {
        if self.vertices.len() < 3 {
            return 0.0;
        }
        let mut area = 0.0;
        let mut side1 = sub(self.vertices[1], self.vertices[0]);
        for i in 2..self.vertices.len() {
            let side2 = sub(self.vertices[i], self.vertices[0]);
            area += size(cross(side1, side2)) * 0.5;
            side1 = side2;
        }
        area
    }

    /// `FPoly::CalcNormal`: nonzero result when the poly has no area.
    fn calc_normal(&mut self) -> bool {
        let mut n = [0.0; 3];
        for i in 2..self.vertices.len() {
            n = add(n, cross(sub(self.vertices[i - 1], self.vertices[0]), sub(self.vertices[i], self.vertices[0])));
        }
        if size_sq(n) < THRESH_ZERO_NORM_SQUARED {
            self.normal = [0.0; 3];
            return true;
        }
        // `FVector::Normalize`.
        self.normal = scale(n, 1.0 / size_sq(n).sqrt());
        false
    }

    /// `FPoly::Finalize`: normal and texture axes when blank; false when the
    /// poly is degenerate.
    pub fn finalize(&mut self) -> bool {
        self.fix();
        if self.vertices.len() < 3 {
            return false;
        }
        if is_zero(self.normal) && self.calc_normal() {
            return false;
        }
        if is_zero(self.texture_u) && is_zero(self.texture_v) {
            for i in 1..self.vertices.len() {
                self.texture_u = safe_normal(cross(sub(self.vertices[0], self.vertices[i]), self.normal));
                self.texture_v = safe_normal(cross(self.normal, self.texture_u));
                if size_sq(self.texture_u) != 0.0 && size_sq(self.texture_v) != 0.0 {
                    break;
                }
            }
        }
        true
    }

    /// `FPoly::SplitWithPlane` (numerically stable); the halves only when it
    /// splits.
    pub fn split_with_plane(&self, plane_base: V, plane_normal: V, very_precise: bool) -> (Split, Option<(EdPoly, EdPoly)>) {
        let thresh = if very_precise { THRESH_SPLIT_POLY_PRECISELY } else { THRESH_SPLIT_POLY_WITH_PLANE };
        #[derive(Clone, Copy, PartialEq)]
        enum S {
            Front,
            Back,
            Either,
        }
        let mut prev_status = S::Either;
        let (mut max_dist, mut min_dist, mut dist) = (0.0f32, 0.0f32, 0.0f32);
        for (i, v) in self.vertices.iter().enumerate() {
            dist = point_plane_dist(*v, plane_base, plane_normal);
            if i == 0 || dist > max_dist {
                max_dist = dist;
            }
            if i == 0 || dist < min_dist {
                min_dist = dist;
            }
            if dist > thresh {
                prev_status = S::Front;
            } else if dist < -thresh {
                prev_status = S::Back;
            }
        }
        if max_dist < thresh && min_dist > -thresh {
            return (Split::Coplanar, None);
        } else if max_dist < thresh {
            return (Split::Back, None);
        } else if min_dist > -thresh {
            return (Split::Front, None);
        }
        let mut front = self.clone();
        front.poly_flags |= PF_ED_CUT;
        front.vertices.clear();
        let mut back = front.clone();
        let n = self.vertices.len();
        let mut j = n - 1;
        for i in 0..n {
            let prev_dist = dist;
            dist = point_plane_dist(self.vertices[i], plane_base, plane_normal);
            let status = if dist > thresh {
                S::Front
            } else if dist < -thresh {
                S::Back
            } else {
                prev_status
            };
            if status != prev_status {
                if dist >= -thresh && dist < thresh {
                    // This point lies on the plane.
                    if prev_status == S::Front {
                        front.vertices.push(self.vertices[i]);
                        back.vertices.push(self.vertices[i]);
                    } else {
                        back.vertices.push(self.vertices[i]);
                        front.vertices.push(self.vertices[i]);
                    }
                } else if prev_dist >= -thresh && prev_dist < thresh {
                    // The previous point lies on the plane.
                    let target = if status == S::Front { &mut front } else { &mut back };
                    target.vertices.push(self.vertices[j]);
                    target.vertices.push(self.vertices[i]);
                } else {
                    let x = line_plane_intersection(self.vertices[j], self.vertices[i], plane_base, plane_normal);
                    if prev_status == S::Front {
                        front.vertices.push(x);
                        back.vertices.push(x);
                        back.vertices.push(self.vertices[i]);
                    } else {
                        back.vertices.push(x);
                        front.vertices.push(x);
                        front.vertices.push(self.vertices[i]);
                    }
                }
            } else if status == S::Front {
                front.vertices.push(self.vertices[i]);
            } else {
                back.vertices.push(self.vertices[i]);
            }
            j = i;
            prev_status = status;
        }
        // Slivers from precision errors.
        if front.fix() < 3 {
            return (Split::Back, None);
        } else if back.fix() < 3 {
            return (Split::Front, None);
        }
        (Split::Split, Some((front, back)))
    }

    /// `FPoly::SplitWithPlaneFast`; `want` asks for the halves.
    pub fn split_with_plane_fast(&self, p: [f32; 4], want: bool) -> (Split, Option<(EdPoly, EdPoly)>) {
        let status: Vec<bool> = self.vertices.iter().map(|v| plane_dot(p, *v) >= 0.0).collect();
        let (mut front, mut back) = (false, false);
        for v in &self.vertices {
            let d = plane_dot(p, *v);
            if d >= 0.0 {
                front |= d > THRESH_SPLIT_POLY_WITH_PLANE;
            } else {
                back |= d < -THRESH_SPLIT_POLY_WITH_PLANE;
            }
        }
        if !front {
            return (if back { Split::Back } else { Split::Coplanar }, None);
        }
        if !back {
            return (Split::Front, None);
        }
        if !want {
            return (Split::Split, None);
        }
        let mut f = EdPoly::init();
        let mut b = EdPoly::init();
        let n = self.vertices.len();
        let mut prev = status[n - 1];
        let mut w = self.vertices[n - 1];
        for (&v, &s) in self.vertices.iter().zip(&status) {
            if s != prev {
                let x = line_plane_intersection_p(w, v, p);
                f.vertices.push(x);
                b.vertices.push(x);
                if prev {
                    b.vertices.push(v);
                } else {
                    f.vertices.push(v);
                }
            } else if s {
                f.vertices.push(v);
            } else {
                b.vertices.push(v);
            }
            prev = s;
            w = v;
        }
        for h in [&mut f, &mut b] {
            h.base = self.base;
            h.normal = self.normal;
            h.poly_flags = self.poly_flags;
        }
        (Split::Split, Some((f, b)))
    }

    /// `FPoly::Split`: keep the half in front of the plane; the vertex count
    /// left (0 when clipped away).
    pub fn split(&mut self, normal: V, base: V) -> usize {
        match self.split_with_plane_fast(plane(base, normal), true) {
            (Split::Back, _) => 0,
            (Split::Split, Some((f, _))) => {
                *self = f;
                self.vertices.len()
            }
            _ => self.vertices.len(),
        }
    }
}

fn points_are_same(p: V, q: V) -> bool {
    (0..3).all(|k| (p[k] - q[k]).abs() < THRESH_POINTS_ARE_SAME)
}

/// `FLinePlaneIntersection(Point1, Point2, PlaneOrigin, PlaneNormal)`.
fn line_plane_intersection(p1: V, p2: V, origin: V, normal: V) -> V {
    let d = sub(p2, p1);
    add(p1, scale(d, dot(sub(origin, p1), normal) / dot(d, normal)))
}

/// `FLinePlaneIntersection(Point1, Point2, Plane)`.
fn line_plane_intersection_p(p1: V, p2: V, p: [f32; 4]) -> V {
    let d = sub(p2, p1);
    add(p1, scale(d, (p[3] - dot(p1, pnormal(p))) / dot(d, pnormal(p))))
}

// ----- builders ------------------------------------------------------------------

/// What a brush builder makes, with the editor's parameter names and
/// defaults.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// `CubeBuilder` (X, Y, Z = 256; WallThickness 16 when hollow).
    Box { x: f32, y: f32, z: f32, hollow: bool, wall: f32, tessellated: bool },
    /// `CylinderBuilder` (Z 256, OuterRadius 512, InnerRadius 384, Sides 8, AlignToSide).
    Cylinder { z: f32, outer: f32, inner: f32, sides: u32, align_to_side: bool, hollow: bool },
    /// `ConeBuilder` (Z 256, CapZ 256, OuterRadius 512, InnerRadius 384, Sides 8).
    Cone { z: f32, cap_z: f32, outer: f32, inner: f32, sides: u32, align_to_side: bool, hollow: bool },
    /// `TetrahedronBuilder` (an octahedron subdivided toward a sphere:
    /// Radius 256, SphereExtrapolation 1–5).
    Sphere { radius: f32, extrapolation: u32 },
    /// The convex hull of points (a static mesh's vertices, an edited volume).
    Hull { points: Vec<V> },
    /// Polygons as given (vertex lists, outward by the right-hand rule of
    /// `(v1 - v0) × (v2 - v0)`), e.g. a volume whose vertices were moved;
    /// concave sets are split into convex hulls by the BSP.
    Polys { polys: Vec<Vec<V>> },
}

/// `UBrushBuilder`'s state (`Vertex3f`, `Poly3i`, `Poly4i`, `PolyBegin`/`Polyi`).
struct Builder {
    vertices: Vec<V>,
    polys: Vec<(i32, Vec<usize>, &'static str, u32)>,
}

impl Builder {
    fn vertex(&mut self, x: f32, y: f32, z: f32) -> usize {
        self.vertices.push([x, y, z]);
        self.vertices.len() - 1
    }
    fn poly(&mut self, dir: i32, idx: &[usize], item: &'static str) {
        self.polys.push((dir, idx.to_vec(), item, PF_DEFAULT_FLAGS));
    }

    /// `CubeBuilder.BuildCube`.
    fn cube(&mut self, dir: i32, dx: f32, dy: f32, dz: f32, tess: bool) {
        let n = self.vertices.len();
        for i in [-1.0f32, 1.0] {
            for j in [-1.0f32, 1.0] {
                for k in [-1.0f32, 1.0] {
                    self.vertex(i * dx / 2.0, j * dy / 2.0, k * dz / 2.0);
                }
            }
        }
        let quads = [[0, 1, 3, 2], [2, 3, 7, 6], [6, 7, 5, 4], [4, 5, 1, 0], [3, 1, 5, 7], [0, 2, 6, 4]];
        for q in quads {
            if tess {
                self.poly(dir, &[n + q[0], n + q[1], n + q[2]], "");
                self.poly(dir, &[n + q[0], n + q[2], n + q[3]], "");
            } else {
                self.poly(dir, &[n + q[0], n + q[1], n + q[2], n + q[3]], "");
            }
        }
    }

    /// `CylinderBuilder.BuildCylinder`.
    fn cylinder(&mut self, dir: i32, align: bool, sides: usize, z: f32, mut radius: f32) {
        let n = self.vertices.len();
        let pi = std::f32::consts::PI;
        let mut ofs = 0.0;
        if align {
            radius /= (pi / sides as f32).cos();
            ofs = 1.0;
        }
        for i in 0..sides {
            for j in [-1.0f32, 1.0] {
                let a = (2.0 * i as f32 + ofs) * pi / sides as f32;
                self.vertex(radius * a.sin(), radius * a.cos(), j * z / 2.0);
            }
        }
        for i in 0..sides {
            self.poly(dir, &[n + i * 2, n + i * 2 + 1, n + (i * 2 + 3) % (2 * sides), n + (i * 2 + 2) % (2 * sides)], "Wall");
        }
    }

    /// `ConeBuilder.BuildCone`.
    fn cone(&mut self, dir: i32, align: bool, sides: usize, z: f32, mut radius: f32, item: &'static str) {
        let n = self.vertices.len();
        let pi = std::f32::consts::PI;
        let mut ofs = 0.0;
        if align {
            radius /= (pi / sides as f32).cos();
            ofs = 1.0;
        }
        for i in 0..sides {
            let a = (2.0 * i as f32 + ofs) * pi / sides as f32;
            self.vertex(radius * a.sin(), radius * a.cos(), 0.0);
        }
        self.vertex(0.0, 0.0, z);
        for i in 0..sides {
            self.poly(dir, &[n + i, n + sides, n + (i + 1) % sides], item);
        }
    }

    /// `TetrahedronBuilder.Extrapolate`.
    fn extrapolate(&mut self, a: usize, b: usize, c: usize, count: u32, radius: f32) {
        if count > 1 {
            let norm = |s: &Self, p: usize, q: usize| scale(safe_normal(add(s.vertices[p], s.vertices[q])), radius);
            let ab = norm(self, a, b);
            let ab = self.vertex(ab[0], ab[1], ab[2]);
            let bc = norm(self, b, c);
            let bc = self.vertex(bc[0], bc[1], bc[2]);
            let ca = norm(self, c, a);
            let ca = self.vertex(ca[0], ca[1], ca[2]);
            self.extrapolate(a, ab, ca, count - 1, radius);
            self.extrapolate(b, bc, ab, count - 1, radius);
            self.extrapolate(c, ca, bc, count - 1, radius);
            self.extrapolate(ab, bc, ca, count - 1, radius);
        } else {
            self.poly(1, &[a, b, c], "");
        }
    }

    /// `UBrushBuilder::EndBrush`: reverse the polys built with a negative
    /// direction, `Base` = first vertex, `FPoly::Finalize`.
    fn end(self) -> Vec<EdPoly> {
        let mut out = Vec::new();
        for (dir, mut idx, item, flags) in self.polys {
            if dir < 0 {
                idx.reverse();
            }
            let mut p = EdPoly::init();
            p.item_name = item.to_string();
            p.base = self.vertices[idx[0]];
            p.poly_flags = flags;
            p.vertices = idx.iter().map(|&i| self.vertices[i]).collect();
            if p.finalize() {
                out.push(p);
            }
        }
        out
    }
}

fn bad(msg: &str) -> String {
    format!("bad brush parameters: {msg}")
}

/// The brush polys of a shape (`Build()` of its builder, then `EndBrush`),
/// linked like `bspValidateBrush(Brush, 1, …)` leaves them.
pub fn build(shape: &Shape) -> Result<Vec<EdPoly>, String> {
    let mut b = Builder { vertices: Vec::new(), polys: Vec::new() };
    match shape {
        &Shape::Box { x, y, z, hollow, wall, tessellated } => {
            if x <= 0.0 || y <= 0.0 || z <= 0.0 {
                return Err(bad("sizes must be positive"));
            }
            if hollow && (z / 2.0 <= wall || y / 2.0 <= wall || x / 2.0 <= wall) {
                return Err(bad("the wall is thicker than half the box"));
            }
            if hollow && tessellated {
                return Err(bad("'Tessellated' can't be used with 'Hollow'"));
            }
            b.cube(1, x, y, z, tessellated);
            if hollow {
                b.cube(-1, x - wall * 2.0, y - wall * 2.0, z - wall * 2.0, tessellated);
            }
        }
        &Shape::Cylinder { z, outer, inner, sides, align_to_side, hollow } => {
            if sides < 3 || z <= 0.0 || outer <= 0.0 {
                return Err(bad("at least 3 sides and positive sizes"));
            }
            if hollow && (inner <= 0.0 || inner >= outer) {
                return Err(bad("the inner radius must be between 0 and the outer radius"));
            }
            let s = sides as usize;
            b.cylinder(1, align_to_side, s, z, outer);
            if hollow {
                b.cylinder(-1, align_to_side, s, z, inner);
                for j in [-1i32, 1] {
                    let o = ((1 - j) / 2) as usize;
                    for i in 0..s {
                        b.poly(j, &[i * 2 + o, ((i + 1) % s) * 2 + o, ((i + 1) % s) * 2 + o + s * 2, i * 2 + o + s * 2], "Cap");
                    }
                }
            } else {
                for j in [-1i32, 1] {
                    let o = ((1 - j) / 2) as usize;
                    let idx: Vec<usize> = (0..s).map(|i| i * 2 + o).collect();
                    b.poly(j, &idx, "Cap");
                }
            }
        }
        &Shape::Cone { z, cap_z, outer, inner, sides, align_to_side, hollow } => {
            if sides < 3 || z <= 0.0 || outer <= 0.0 {
                return Err(bad("at least 3 sides and positive sizes"));
            }
            if hollow && (inner <= 0.0 || inner >= outer || cap_z > z || (cap_z == z && inner == outer)) {
                return Err(bad("the inner cone must fit in the outer one"));
            }
            let s = sides as usize;
            b.cone(1, align_to_side, s, z, outer, "Top");
            if hollow {
                b.cone(-1, align_to_side, s, cap_z, inner, "Cap");
                if outer != inner {
                    for i in 0..s {
                        b.poly(1, &[i, (i + 1) % s, s + 1 + (i + 1) % s, s + 1 + i], "Bottom");
                    }
                }
            } else {
                let idx: Vec<usize> = (0..s).collect();
                b.poly(1, &idx, "Bottom");
            }
        }
        &Shape::Sphere { radius, extrapolation } => {
            if radius <= 0.0 || extrapolation == 0 || extrapolation > 5 {
                return Err(bad("a positive radius and an extrapolation of 1 to 5"));
            }
            let r = radius;
            for v in [[r, 0.0, 0.0], [-r, 0.0, 0.0], [0.0, r, 0.0], [0.0, -r, 0.0], [0.0, 0.0, r], [0.0, 0.0, -r]] {
                b.vertex(v[0], v[1], v[2]);
            }
            for (a, c, d) in [(2, 1, 4), (1, 3, 4), (3, 0, 4), (0, 2, 4), (1, 2, 5), (3, 1, 5), (0, 3, 5), (2, 0, 5)] {
                b.extrapolate(a, c, d, extrapolation, radius);
            }
        }
        Shape::Hull { points } => {
            let (verts, faces) = hull_faces(points).ok_or_else(|| bad("the points have no volume"))?;
            b.vertices = verts;
            for f in faces {
                b.polys.push((1, f, "", PF_DEFAULT_FLAGS));
            }
        }
        Shape::Polys { polys } => {
            for p in polys {
                if p.len() < 3 {
                    return Err(bad("a polygon needs three vertices"));
                }
                let n = b.vertices.len();
                b.vertices.extend(p.iter().copied());
                b.polys.push((1, (n..n + p.len()).collect(), "", PF_DEFAULT_FLAGS));
            }
        }
    }
    let mut polys = b.end();
    if polys.len() < 4 {
        return Err(bad("the shape has no volume"));
    }
    validate_brush(&mut polys);
    Ok(polys)
}

/// `FBSPOps::bspValidateBrush` (forced): `iLink` of each poly = the first
/// identical poly (same flags, texture axes, coplanar).
pub fn validate_brush(polys: &mut [EdPoly]) {
    for (i, p) in polys.iter_mut().enumerate() {
        p.i_link = i as i32;
    }
    for i in 0..polys.len() {
        if polys[i].i_link != i as i32 {
            continue;
        }
        for j in i + 1..polys.len() {
            let (e, o) = (&polys[i], &polys[j]);
            if o.i_link == j as i32 && o.texture_u == e.texture_u && o.texture_v == e.texture_v && o.poly_flags == e.poly_flags && dot(o.normal, e.normal) > 0.9999 {
                let d = point_plane_dist(o.vertices[0], e.vertices[0], e.normal);
                if d > -0.001 && d < 0.001 {
                    polys[j].i_link = i as i32;
                }
            }
        }
    }
}

// ----- the brush BSP -------------------------------------------------------------

/// `FBspNode`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BspNode {
    pub plane: [f32; 4],
    pub i_vert_pool: i32,
    pub i_surf: i32,
    pub i_back: i32,
    pub i_front: i32,
    pub i_plane: i32,
    pub i_collision_bound: i32,
    pub i_zone: [u8; 2],
    pub num_vertices: u8,
    pub node_flags: u8,
    pub i_leaf: [i32; 2],
}

impl BspNode {
    fn is_csg(&self) -> bool {
        self.num_vertices > 0 && self.node_flags & (NF_IS_NEW | NF_NOT_CSG) == 0
    }
    /// `FBspNode::ChildOutside`.
    fn child_outside(&self, child: usize, outside: bool) -> bool {
        if child != 0 {
            outside || self.is_csg()
        } else {
            outside && !self.is_csg()
        }
    }
}

/// `FBspSurf` (what a brush model's surfaces hold; materials and actors none).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BspSurf {
    pub poly_flags: u32,
    pub p_base: i32,
    pub v_normal: i32,
    pub v_texture_u: i32,
    pub v_texture_v: i32,
    pub i_brush_poly: i32,
    pub plane: [f32; 4],
}

/// A brush's `UModel` tables after `FBSPOps::RebuildBrush`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BrushModel {
    pub nodes: Vec<BspNode>,
    pub surfs: Vec<BspSurf>,
    /// `FVert`: `pVertex`, `iSide`.
    pub verts: Vec<(i32, i32)>,
    pub points: Vec<V>,
    pub vectors: Vec<V>,
    pub leaf_hulls: Vec<i32>,
    /// `Bounds` (`UModel::BuildBound`): origin, extent, sphere radius.
    pub bounds: (V, V, f32),
    pub root_outside: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Place {
    Root,
    Front,
    Back,
    Plane,
}

/// `AddThing`: the index of a point/vector of `table` within `thresh` of
/// `v`, else `v` appended (vectors always; points only skip it, see
/// `add_point`).
fn add_thing(table: &mut Vec<V>, v: V, thresh: f32) -> i32 {
    for (i, t) in table.iter().enumerate() {
        let near = |k: usize| {
            let d = v[k] - t[k];
            d > -thresh && d < thresh
        };
        if near(0) && near(1) && near(2) {
            return i as i32;
        }
    }
    table.push(v);
    table.len() as i32 - 1
}

/// `FBoxSphereBounds(Points, NumPoints)`.
pub fn box_sphere_bounds(points: &[V]) -> (V, V, f32) {
    let (min, max) = bbox(points);
    let ext = scale(sub(max, min), 0.5);
    let origin = add(min, ext);
    let radius = points.iter().map(|p| size(sub(*p, origin))).fold(0.0f32, f32::max);
    (origin, ext, radius)
}

fn bbox(points: &[V]) -> (V, V) {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for p in points {
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
    }
    (min, max)
}

impl BrushModel {
    /// `FBSPOps::RebuildBrush`: `BuildBound`, `bspBuild(BSP_Good, 15, 70,
    /// RebuildSimplePolys = 1)`, `bspRefresh`, `bspBuildBounds`. Sets the
    /// polys' `iLink` to their surfaces as the editor does.
    pub fn rebuild(polys: &mut [EdPoly]) -> BrushModel {
        let all: Vec<V> = polys.iter().flat_map(|p| p.vertices.iter().copied()).collect();
        let mut m = BrushModel { bounds: box_sphere_bounds(&all), root_outside: true, ..Default::default() };
        // The pool: the brush's polys, then the pieces splits make.
        let mut arena: Vec<EdPoly> = polys.to_vec();
        let list: Vec<usize> = (0..arena.len()).filter(|&i| !arena[i].vertices.is_empty()).collect();
        if !list.is_empty() {
            m.split_poly_list(&mut arena, INDEX_NONE, Place::Root, list);
        }
        for (p, a) in polys.iter_mut().zip(&arena) {
            p.i_link = a.i_link;
        }
        m.refresh();
        m.build_bounds();
        m
    }

    /// `FindBestSplit` for `BSP_Good`.
    fn find_best_split(arena: &[EdPoly], list: &[usize], balance: i32) -> usize {
        let n = list.len();
        if n == 1 {
            return list[0];
        }
        let balance = (balance & 0xff) as f32;
        let inc = (n / 20).max(1);
        let all_semi = list.iter().all(|&i| arena[i].poly_flags & PF_ADD_LAST != 0);
        let mut best: Option<usize> = None;
        let mut best_score = 0.0f32;
        let mut i = 0;
        while i < n {
            let mut index = i as isize - 1;
            let mut poly;
            loop {
                index += 1;
                poly = &arena[list[index as usize]];
                if !((index as usize) < i + inc && (index as usize) < n && poly.poly_flags & PF_ADD_LAST != 0 && poly.poly_flags & PF_PORTAL == 0 && !all_semi) {
                    break;
                }
            }
            let index = index as usize;
            if index >= i + inc || index >= n {
                i += inc;
                continue;
            }
            let (mut splits, mut front, mut back) = (0i32, 0i32, 0i32);
            let p = plane(poly.vertices[0], poly.normal);
            let mut j = 0;
            while j < n {
                if j != index {
                    let other = &arena[list[j]];
                    match other.split_with_plane_fast(p, false).0 {
                        Split::Front => front += 1,
                        Split::Back => back += 1,
                        Split::Split => splits += if other.poly_flags & PF_PORTAL == 0 { 1 } else { 16 },
                        Split::Coplanar => {}
                    }
                }
                j += inc;
            }
            let mut score = ((100.0 - balance as f64) * splits as f64 + balance as f64 * (front - back).abs() as f64) as f32;
            if poly.poly_flags & PF_PORTAL != 0 {
                // PortalBias 70.
                score = (score as f64 - (100.0 - balance as f64) * splits as f64 * 0.7) as f32;
            }
            if score < best_score || best.is_none() {
                best = Some(list[index]);
                best_score = score;
            }
            i += inc;
        }
        best.unwrap_or(list[0])
    }

    /// `FBSPOps::SplitPolyList` (RebuildSimplePolys: every splitter gets a
    /// surface of its own, its coplanars share it).
    fn split_poly_list(&mut self, arena: &mut Vec<EdPoly>, parent: i32, place: Place, list: Vec<usize>) {
        let split = Self::find_best_split(arena, &list, 15);
        arena[split].i_link = self.surfs.len() as i32;
        let our = self.add_node(arena, parent, place, 0, split);
        let mut plane_node = our;
        let (sv, sn) = (arena[split].vertices[0], arena[split].normal);
        let mut front = Vec::new();
        let mut back = Vec::new();
        for &i in &list {
            if i == split {
                continue;
            }
            match arena[i].split_with_plane(sv, sn, false) {
                (Split::Coplanar, _) => {
                    arena[i].i_link = self.surfs.len() as i32 - 1;
                    plane_node = self.add_node(arena, plane_node, Place::Plane, 0, i);
                }
                (Split::Front, _) => front.push(i),
                (Split::Back, _) => back.push(i),
                (Split::Split, Some((f, b))) => {
                    arena.push(f);
                    front.push(arena.len() - 1);
                    arena.push(b);
                    back.push(arena.len() - 1);
                }
                (Split::Split, None) => {}
            }
        }
        if !front.is_empty() {
            self.split_poly_list(arena, our, Place::Front, front);
        }
        if !back.is_empty() {
            self.split_poly_list(arena, our, Place::Back, back);
        }
    }

    /// `FBSPOps::bspAddNode` (polys stay under `MAX_NODE_VERTICES`).
    fn add_node(&mut self, arena: &[EdPoly], mut parent: i32, place: Place, mut flags: u8, poly: usize) -> i32 {
        let ed = &arena[poly];
        if place == Place::Plane {
            while self.nodes[parent as usize].i_plane != INDEX_NONE {
                parent = self.nodes[parent as usize].i_plane;
            }
        }
        if ed.i_link == self.surfs.len() as i32 {
            let p_base = self.add_point(ed.base, THRESH_POINTS_ARE_SAME);
            let v_normal = add_thing(&mut self.vectors, ed.normal, THRESH_NORMALS_ARE_SAME);
            let v_texture_u = add_thing(&mut self.vectors, ed.texture_u, THRESH_VECTORS_ARE_NEAR);
            let v_texture_v = add_thing(&mut self.vectors, ed.texture_v, THRESH_VECTORS_ARE_NEAR);
            self.surfs.push(BspSurf { poly_flags: ed.poly_flags & !PF_NO_ADD_TO_BSP, p_base, v_normal, v_texture_u, v_texture_v, i_brush_poly: ed.i_brush_poly, plane: plane(ed.vertices[0], ed.normal) });
        }
        let surf_flags = self.surfs[ed.i_link as usize].poly_flags;
        if surf_flags & PF_NOT_SOLID != 0 {
            flags |= NF_NOT_CSG;
        }
        if surf_flags & (PF_INVISIBLE | PF_PORTAL) != 0 {
            flags |= NF_NOT_VIS_BLOCKING;
        }
        let i_node = self.nodes.len() as i32;
        let node_plane = plane(ed.vertices[0], ed.normal);
        let mut node = BspNode { i_surf: ed.i_link, node_flags: flags, i_collision_bound: INDEX_NONE, plane: node_plane, i_vert_pool: self.verts.len() as i32, i_front: INDEX_NONE, i_back: INDEX_NONE, i_plane: INDEX_NONE, ..Default::default() };
        // `Verts.Add(n)`: the pool reserves one entry per poly vertex.
        self.verts.extend(std::iter::repeat_n((0, 0), ed.vertices.len()));
        match place {
            Place::Root => node.i_leaf = [INDEX_NONE; 2],
            Place::Front | Place::Back => {
                let p = &self.nodes[parent as usize];
                let z = (place == Place::Front) as usize;
                node.i_leaf = [p.i_leaf[z]; 2];
                node.i_zone = [p.i_zone[z]; 2];
            }
            Place::Plane => {
                let p = &self.nodes[parent as usize];
                let flipped = (dot(pnormal(node_plane), pnormal(p.plane)) + node_plane[3] * p.plane[3] < 0.0) as usize;
                node.i_leaf = [p.i_leaf[flipped], p.i_leaf[1 - flipped]];
                node.i_zone = [p.i_zone[flipped], p.i_zone[1 - flipped]];
            }
        }
        match place {
            Place::Front => self.nodes[parent as usize].i_front = i_node,
            Place::Back => self.nodes[parent as usize].i_back = i_node,
            Place::Plane => self.nodes[parent as usize].i_plane = i_node,
            Place::Root => {}
        }
        let pool = node.i_vert_pool as usize;
        // In the table before its points are added, `NumVertices` kept up
        // to date: `bspAddPoint` searches the tree, this node included.
        self.nodes.push(node);
        let mut nv = 0usize;
        for v in &ed.vertices {
            let p = self.add_point(*v, THRESH_POINTS_ARE_NEAR);
            if nv == 0 || self.verts[pool + nv - 1].0 != p {
                self.verts[pool + nv] = (p, INDEX_NONE);
                nv += 1;
                self.nodes[i_node as usize].num_vertices = nv as u8;
            }
        }
        if nv >= 2 && self.verts[pool].0 == self.verts[pool + nv - 1].0 {
            nv -= 1;
        }
        if nv < 3 {
            nv = 0;
        }
        self.nodes[i_node as usize].num_vertices = nv as u8;
        i_node
    }

    /// `FBSPOps::bspAddPoint`: a point of the nodes (or their surfaces'
    /// bases) near enough (`UModel::FindNearestVertex`), else a new point.
    /// The table itself is not searched: `GFastRebuild` is set while the
    /// editor rebuilds geometry (`csgRebuild` → `csgPrepMovingBrush`), which
    /// is when volumes' brushes were last built (a reverb volume of
    /// Abyss_Collision has its first base point twice).
    fn add_point(&mut self, v: V, thresh: f32) -> i32 {
        let mut found = INDEX_NONE;
        if !self.nodes.is_empty() {
            let mut radius = thresh;
            self.find_nearest_vertex(v, &mut radius, 0, &mut found);
        }
        if found != INDEX_NONE {
            return found;
        }
        self.points.push(v);
        self.points.len() as i32 - 1
    }

    /// `FindNearestVertex` (UnModelCollision.cpp); `radius` shrinks to the
    /// nearest distance found.
    fn find_nearest_vertex(&self, v: V, radius: &mut f32, mut i_node: i32, found: &mut i32) -> bool {
        let mut any = false;
        while i_node != INDEX_NONE {
            let node = &self.nodes[i_node as usize];
            let back = node.i_back;
            let dist = plane_dot(node.plane, v);
            if dist >= -*radius && node.i_front != INDEX_NONE && self.find_nearest_vertex(v, radius, node.i_front, found) {
                any = true;
            }
            if dist > -*radius && dist <= *radius {
                let mut k = i_node;
                while k != INDEX_NONE {
                    let n = &self.nodes[k as usize];
                    let surf = &self.surfs[n.i_surf as usize];
                    let mut try_point = |p: i32, radius: &mut f32| {
                        let d2 = size_sq(sub(v, self.points[p as usize]));
                        if d2 < *radius * *radius {
                            *found = p;
                            *radius = d2.sqrt();
                            any = true;
                        }
                    };
                    try_point(surf.p_base, radius);
                    for b in 0..n.num_vertices as usize {
                        try_point(self.verts[n.i_vert_pool as usize + b].0, radius);
                    }
                    k = n.i_plane;
                }
            }
            if dist > *radius {
                break;
            }
            i_node = back;
        }
        any
    }

    /// `FBSPOps::bspRefresh(Model, NoRemapSurfs = 1)`: drop the nodes the
    /// tree does not reach and the points/vectors nothing uses.
    fn refresh(&mut self) {
        if self.nodes.is_empty() {
            return;
        }
        let mut node_ref = vec![INDEX_NONE; self.nodes.len()];
        let mut stack = vec![0usize];
        while let Some(i) = stack.pop() {
            node_ref[i] = 0;
            let n = &self.nodes[i];
            for c in [n.i_front, n.i_back, n.i_plane] {
                if c != INDEX_NONE {
                    stack.push(c as usize);
                }
            }
        }
        let mut k = 0;
        let mut kept = Vec::new();
        for (i, n) in self.nodes.iter().enumerate() {
            if node_ref[i] != INDEX_NONE {
                node_ref[i] = k;
                k += 1;
                kept.push(n.clone());
            }
        }
        self.nodes = kept;
        let remap = |x: i32| if x == INDEX_NONE { x } else { node_ref[x as usize] };
        for n in self.nodes.iter_mut() {
            n.i_front = remap(n.i_front);
            n.i_back = remap(n.i_back);
            n.i_plane = remap(n.i_plane);
        }
        let mut vector_ref = vec![INDEX_NONE; self.vectors.len()];
        let mut point_ref = vec![INDEX_NONE; self.points.len()];
        for s in &self.surfs {
            vector_ref[s.v_normal as usize] = 0;
            vector_ref[s.v_texture_u as usize] = 0;
            vector_ref[s.v_texture_v as usize] = 0;
            point_ref[s.p_base as usize] = 0;
        }
        for n in &self.nodes {
            for b in 0..n.num_vertices as usize {
                point_ref[self.verts[n.i_vert_pool as usize + b].0 as usize] = 0;
            }
        }
        let compact = |table: &mut Vec<V>, r: &mut Vec<i32>| {
            let mut k = 0;
            let mut out = Vec::new();
            for (i, v) in table.iter().enumerate() {
                if r[i] != INDEX_NONE {
                    r[i] = k;
                    k += 1;
                    out.push(*v);
                }
            }
            *table = out;
        };
        compact(&mut self.points, &mut point_ref);
        compact(&mut self.vectors, &mut vector_ref);
        for s in self.surfs.iter_mut() {
            s.v_normal = vector_ref[s.v_normal as usize];
            s.v_texture_u = vector_ref[s.v_texture_u as usize];
            s.v_texture_v = vector_ref[s.v_texture_v as usize];
            s.p_base = point_ref[s.p_base as usize];
        }
        for n in &self.nodes {
            for b in 0..n.num_vertices as usize {
                let v = &mut self.verts[n.i_vert_pool as usize + b];
                v.0 = point_ref[v.0 as usize];
            }
        }
    }

    /// `FBSPOps::bspBuildBounds`: filter a world-sized cube down the tree;
    /// each solid leaf records the partition planes that bound it and its box
    /// (`LeafHulls`, `iCollisionBound`).
    fn build_bounds(&mut self) {
        if self.nodes.is_empty() {
            return;
        }
        let h = HALF_WORLD_MAX;
        let faces: [([V; 4], V); 6] = [
            ([[-h, -h, h], [h, -h, h], [h, h, h], [-h, h, h]], [0.0, 0.0, 1.0]),
            ([[-h, h, -h], [h, h, -h], [h, -h, -h], [-h, -h, -h]], [0.0, 0.0, -1.0]),
            ([[-h, h, -h], [-h, h, h], [h, h, h], [h, h, -h]], [0.0, 1.0, 0.0]),
            ([[h, -h, -h], [h, -h, h], [-h, -h, h], [-h, -h, -h]], [0.0, -1.0, 0.0]),
            ([[h, h, -h], [h, h, h], [h, -h, h], [h, -h, -h]], [1.0, 0.0, 0.0]),
            ([[-h, -h, -h], [-h, -h, h], [-h, h, h], [-h, h, -h]], [-1.0, 0.0, 0.0]),
        ];
        let list: Vec<EdPoly> = faces
            .iter()
            .map(|(vs, n)| {
                let mut p = EdPoly::init();
                p.vertices = vs.to_vec();
                p.normal = *n;
                p.base = vs[0];
                p
            })
            .collect();
        self.leaf_hulls.clear();
        for n in self.nodes.iter_mut() {
            n.i_collision_bound = INDEX_NONE;
        }
        let root_outside = self.root_outside;
        self.filter_bound(0, list, root_outside);
    }

    /// `FBSPOps::BuildInfiniteFPoly`.
    fn infinite_poly(&self, i_node: usize) -> EdPoly {
        let surf = &self.surfs[self.nodes[i_node].i_surf as usize];
        let normal = pnormal(surf.plane);
        let base = scale(normal, surf.plane[3]);
        let (a1, a2) = find_best_axis_vectors(normal);
        let mut p = EdPoly::init();
        p.normal = normal;
        p.base = base;
        let w = WORLD_MAX;
        p.vertices = vec![add(add(base, scale(a1, w)), scale(a2, w)), add(sub(base, scale(a1, w)), scale(a2, w)), sub(sub(base, scale(a1, w)), scale(a2, w)), sub(add(base, scale(a1, w)), scale(a2, w))];
        p
    }

    /// `FilterBound`.
    fn filter_bound(&mut self, i_node: usize, list: Vec<EdPoly>, outside: bool) {
        let node = self.nodes[i_node].clone();
        let surf = &self.surfs[node.i_surf as usize];
        let base = scale(pnormal(surf.plane), surf.plane[3]);
        let normal = self.vectors[surf.v_normal as usize];
        let mut front = Vec::new();
        let mut back = Vec::new();
        for p in &list {
            match p.split_with_plane(base, normal, false) {
                (Split::Coplanar, _) => {
                    front.push(p.clone());
                    back.push(p.clone());
                }
                (Split::Front, _) => front.push(p.clone()),
                (Split::Back, _) => back.push(p.clone()),
                (Split::Split, Some((f, b))) => {
                    front.push(f);
                    back.push(b);
                }
                (Split::Split, None) => {}
            }
        }
        if !front.is_empty() && !back.is_empty() {
            // `SplitPartitioner`: the partition plane clipped to the hull,
            // facing out on the front side.
            let mut inf = self.infinite_poly(i_node);
            inf.i_brush_poly = i_node as i32;
            let mut ok = true;
            for p in &list {
                match inf.split_with_plane(p.vertices[0], p.normal, false) {
                    (Split::Front, _) => {
                        ok = false;
                        break;
                    }
                    (Split::Split, Some((_, b))) => inf = b,
                    _ => {}
                }
            }
            if ok {
                let mut rev = inf.clone();
                rev.reverse();
                rev.i_brush_poly |= 0x4000_0000;
                front.push(rev);
                back.push(inf);
            }
        }
        let csg = node.is_csg();
        if !front.is_empty() {
            if node.i_front != INDEX_NONE {
                self.filter_bound(node.i_front as usize, front, outside || csg);
            } else if !(outside || csg) {
                self.update_convolution(i_node, &front);
            }
        }
        if !back.is_empty() {
            if node.i_back != INDEX_NONE {
                self.filter_bound(node.i_back as usize, back, outside && !csg);
            } else if !(outside && !csg) {
                self.update_convolution(i_node, &back);
            }
        }
    }

    /// `UpdateConvolutionWithPolys`.
    fn update_convolution(&mut self, i_node: usize, list: &[EdPoly]) {
        self.nodes[i_node].i_collision_bound = self.leaf_hulls.len() as i32;
        let mut pts = Vec::new();
        for (i, p) in list.iter().enumerate() {
            if p.i_brush_poly != INDEX_NONE && !list[..i].iter().any(|q| q.i_brush_poly == p.i_brush_poly) {
                self.leaf_hulls.push(p.i_brush_poly);
            }
            pts.extend(p.vertices.iter().copied());
        }
        self.leaf_hulls.push(INDEX_NONE);
        let (min, max) = bbox(&pts);
        for x in [min[0], min[1], min[2], max[0], max[1], max[2]] {
            self.leaf_hulls.push(x.to_bits() as i32);
        }
    }

    /// `KModelToHulls` → `ModelToHullsWorker`: one convex element per solid
    /// leaf, from the planes on the path to it (`AddConvexPrim`; the model's
    /// points referenced by `Verts` snap the hull's corners).
    pub fn to_hulls(&self) -> Vec<ConvexElem> {
        let mut out = Vec::new();
        if self.nodes.is_empty() {
            return out;
        }
        let snap: Vec<V> = self.verts.iter().filter(|v| v.0 >= 0 && (v.0 as usize) < self.points.len()).map(|v| self.points[v.0 as usize]).collect();
        let mut planes = Vec::new();
        self.hulls_worker(0, self.root_outside, &mut planes, &snap, &mut out);
        out
    }

    fn hulls_worker(&self, i: usize, outside: bool, planes: &mut Vec<[f32; 4]>, snap: &[V], out: &mut Vec<ConvexElem>) {
        let node = &self.nodes[i];
        let flip = |p: [f32; 4]| [-p[0], -p[1], -p[2], -p[3]];
        for (child, side_plane) in [(0usize, node.plane), (1usize, flip(node.plane))] {
            let next = if child == 0 { node.i_back } else { node.i_front };
            let child_outside = node.child_outside(child, outside);
            if next != INDEX_NONE {
                planes.push(side_plane);
                self.hulls_worker(next as usize, child_outside, planes, snap, out);
                planes.pop();
            } else if !child_outside {
                planes.push(side_plane);
                if let Some(e) = ConvexElem::from_planes(planes, snap) {
                    out.push(e);
                }
                planes.pop();
            }
        }
    }
}

// ----- convex elements -------------------------------------------------------------

/// `FKConvexElem`: every member the cooked `BrushAggGeom` / `AggGeom` stores.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConvexElem {
    pub vertex_data: Vec<V>,
    /// `FPlane`s (X, Y, Z, W) holding four vertices' X, Y or Z each.
    pub permuted_vertex_data: Vec<[f32; 4]>,
    pub face_tri_data: Vec<i32>,
    pub edge_directions: Vec<V>,
    pub face_normal_directions: Vec<V>,
    pub face_plane_data: Vec<[f32; 4]>,
    /// `ElemBox`: min, max, valid.
    pub elem_box: (V, V, bool),
}

/// `AddVertexIfNotPresent`.
fn add_vertex_if_not_present(vs: &mut Vec<V>, v: V) {
    if !vs.iter().any(|x| size_sq(sub(v, *x)) < MIN_HULL_VERT_DISTANCE * MIN_HULL_VERT_DISTANCE) {
        vs.push(v);
    }
}

/// `AddPlaneIfNotPresent`.
fn add_plane_if_not_present(planes: &mut Vec<[f32; 4]>, p: [f32; 4]) {
    if !planes.iter().any(|t| (t[0] * p[0] + t[1] * p[1] + t[2] * p[2] - 1.0).abs() < DIR_COMPARE_THRESH && (t[3] - p[3]).abs() < DIST_COMPARE_THRESH) {
        planes.push(p);
    }
}

/// `AddDirIfNotPresent` (a direction or its opposite).
fn add_dir_if_not_present(dirs: &mut Vec<V>, d: V) {
    if !dirs.iter().any(|t| (dot(*t, d).abs() - 1.0).abs() < DIR_COMPARE_THRESH) {
        dirs.push(d);
    }
}

impl ConvexElem {
    /// `FKConvexElem::HullFromPlanes`: each plane clipped by all the others
    /// gives a face; its corners (snapped to `snap` within 0.1) are the hull's
    /// vertices. `None` when the engine would drop the element
    /// (`AddConvexPrim`: no area, or `ElemBox` left invalid).
    pub fn from_planes(planes: &[[f32; 4]], snap: &[V]) -> Option<ConvexElem> {
        let mut verts: Vec<V> = Vec::new();
        let mut total_area = 0.0f32;
        for (i, pl) in planes.iter().enumerate() {
            let mut poly = EdPoly::init();
            poly.normal = pnormal(*pl);
            let (ax, ay) = find_best_axis_vectors(poly.normal);
            let base = scale(pnormal(*pl), pl[3]);
            let h = HALF_WORLD_MAX;
            poly.vertices = vec![add(add(base, scale(ax, h)), scale(ay, h)), add(sub(base, scale(ax, h)), scale(ay, h)), sub(sub(base, scale(ax, h)), scale(ay, h)), sub(add(base, scale(ax, h)), scale(ay, h))];
            for (j, other) in planes.iter().enumerate() {
                if i != j && poly.split(scale(pnormal(*other), -1.0), scale(pnormal(*other), other[3])) == 0 {
                    poly.vertices.clear();
                    break;
                }
            }
            if poly.vertices.is_empty() {
                continue;
            }
            total_area += poly.area();
            for v in &poly.vertices {
                let mut nearest = None;
                let mut best = f32::MAX;
                for (k, s) in snap.iter().enumerate() {
                    let d = size_sq(sub(*v, *s));
                    if d < best {
                        nearest = Some(k);
                        best = d;
                    }
                }
                match nearest {
                    Some(k) if best < LOCAL_EPS => add_vertex_if_not_present(&mut verts, snap[k]),
                    _ => add_vertex_if_not_present(&mut verts, *v),
                }
            }
        }
        if total_area < 0.001 || verts.len() < 4 {
            return None;
        }
        // Not all on a line, not all on a plane.
        let d1 = safe_normal(sub(verts[1], verts[0]));
        let mut d2 = [0.0; 3];
        let mut found = false;
        for v in &verts[2..] {
            d2 = safe_normal(sub(*v, verts[0]));
            if dot(d1, d2) < 1.0 - LOCAL_EPS {
                found = true;
                break;
            }
        }
        if !found {
            return None;
        }
        let n = safe_normal(cross(d1, d2));
        let p = plane(verts[0], n);
        if !verts[2..].iter().any(|v| plane_dot(p, *v) > LOCAL_EPS) {
            return None;
        }
        ConvexElem::generate(&verts)
    }

    /// `FKConvexElem::GenerateHullData`: the hull of `points` (PhysX's
    /// convex cooking in the engine; an incremental hull here), then its
    /// triangles, edges, face normals and planes, box and SIMD copy.
    pub fn generate(points: &[V]) -> Option<ConvexElem> {
        // The engine cooks in PhysX scale (`U2PScale` 0.02) after
        // `EnsureHullIsValid` → `RemoveDuplicateVerts` there (so points
        // closer than 0.1 unit merge), and scales the result back
        // (`P2UScale` 50): 480 comes back as 480.00003.
        let mut phys: Vec<V> = Vec::new();
        for p in points {
            add_vertex_if_not_present(&mut phys, scale(*p, 0.02));
        }
        let (verts, tris) = hull_triangles(&phys)?;
        Some(ConvexElem::from_hull(verts.iter().map(|v| scale(*v, 50.0)).collect(), &tris))
    }

    /// The part of `GenerateHullData` after the hull: `tris` as `FaceTriData`
    /// stores them (I0, I1, I2 with `(V2 - V0) × (V1 - V0)` pointing out).
    pub fn from_hull(vertex_data: Vec<V>, tris: &[[i32; 3]]) -> ConvexElem {
        let mut e = ConvexElem { vertex_data, ..Default::default() };
        let (min, max) = bbox(&e.vertex_data);
        e.elem_box = (min, max, !e.vertex_data.is_empty());
        let mut edges: Vec<(i32, i32)> = Vec::new();
        let mut tri_normals = Vec::new();
        let add_edge = |edges: &mut Vec<(i32, i32)>, a: i32, b: i32| {
            if !edges.iter().any(|&(x, y)| (x == a && y == b) || (x == b && y == a)) {
                edges.push((a, b));
            }
        };
        for t in tris {
            let (i0, i1, i2) = (t[0], t[1], t[2]);
            let (v0, v1, v2) = (e.vertex_data[i0 as usize], e.vertex_data[i1 as usize], e.vertex_data[i2 as usize]);
            e.face_tri_data.extend([i0, i1, i2]);
            add_edge(&mut edges, i0, i1);
            add_edge(&mut edges, i1, i2);
            add_edge(&mut edges, i2, i0);
            let n = safe_normal(cross(sub(v2, v0), sub(v1, v0)));
            tri_normals.push(n);
            add_dir_if_not_present(&mut e.face_normal_directions, n);
            add_plane_if_not_present(&mut e.face_plane_data, plane(v0, n));
        }
        // The edges between faces that are not coplanar.
        let has = |t: &[i32], a: i32, b: i32| (t[0] == a && t[1] == b) || (t[1] == a && t[0] == b) || (t[0] == a && t[2] == b) || (t[2] == a && t[0] == b) || (t[1] == a && t[2] == b) || (t[2] == a && t[1] == b);
        for (a, b) in edges {
            let mut on = e.face_tri_data.chunks(3).enumerate().filter(|(_, t)| has(t, a, b)).map(|(k, _)| k);
            if let (Some(t0), Some(t1)) = (on.next(), on.next()) {
                if dot(tri_normals[t0], tri_normals[t1]) < 1.0 - DIR_COMPARE_THRESH {
                    let d = safe_normal(sub(e.vertex_data[a as usize], e.vertex_data[b as usize]));
                    add_dir_if_not_present(&mut e.edge_directions, d);
                }
            }
        }
        e.permute();
        e
    }

    /// `FKConvexElem::PermuteVertexData`.
    fn permute(&mut self) {
        let v = &self.vertex_data;
        let rem = v.len() % 4;
        let full = v.len() - rem;
        let mut out = Vec::new();
        for c in (0..full).step_by(4) {
            out.extend((0..3).map(|k| [v[c][k], v[c + 1][k], v[c + 2][k], v[c + 3][k]]));
        }
        if rem > 0 {
            let l1 = v[full];
            let l2 = if rem >= 2 { v[full + 1] } else { l1 };
            let l3 = if rem == 3 { v[full + 2] } else { l1 };
            for k in 0..3 {
                out.push([l1[k], l2[k], l3[k], l1[k]]);
            }
        }
        self.permuted_vertex_data = out;
    }

    /// Triangles to draw, outward by the right-hand rule (`FaceTriData` with
    /// I1 and I2 swapped).
    pub fn draw_triangles(&self) -> Vec<[i32; 3]> {
        self.face_tri_data.chunks(3).filter(|t| t.len() == 3).map(|t| [t[0], t[2], t[1]]).collect()
    }
}

// ----- convex hulls (the PhysX stand-in) ---------------------------------------------

/// Incremental 3D convex hull in f64: the hull's vertices (in the order
/// they appear in `points`, duplicates closer than the engine's
/// `MIN_HULL_VERT_DISTANCE` merged) and its triangles as `FaceTriData`
/// stores them. `None` when the points span no volume.
pub fn hull_triangles(points: &[V]) -> Option<(Vec<V>, Vec<[i32; 3]>)> {
    let (verts, faces) = hull(points)?;
    let tris = faces.iter().map(|f| [f[0] as i32, f[2] as i32, f[1] as i32]).collect();
    Some((verts, tris))
}

/// The hull's faces merged into polygons (coplanar triangles joined, corners
/// in order, outward by the right-hand rule), for brush polys.
pub fn hull_faces(points: &[V]) -> Option<(Vec<V>, Vec<Vec<usize>>)> {
    let (verts, tris) = hull(points)?;
    let p: Vec<[f64; 3]> = verts.iter().map(|v| [v[0] as f64, v[1] as f64, v[2] as f64]).collect();
    let normal = |t: &[usize; 3]| dnorm(dcross(dsub(p[t[1]], p[t[0]]), dsub(p[t[2]], p[t[0]])));
    let mut used = vec![false; tris.len()];
    let mut faces = Vec::new();
    for i in 0..tris.len() {
        if used[i] {
            continue;
        }
        let n = normal(&tris[i]);
        let d = ddot(n, p[tris[i][0]]);
        let mut corners: Vec<usize> = Vec::new();
        for j in i..tris.len() {
            if used[j] {
                continue;
            }
            let m = normal(&tris[j]);
            if ddot(n, m) > 1.0 - 1e-6 && tris[j].iter().all(|&k| (ddot(n, p[k]) - d).abs() < 0.01) {
                used[j] = true;
                for &k in &tris[j] {
                    if !corners.contains(&k) {
                        corners.push(k);
                    }
                }
            }
        }
        // Order around the centroid, counter-clockwise seen from outside.
        let c = corners.iter().fold([0.0; 3], |a, &k| dadd(a, p[k]));
        let c = [c[0] / corners.len() as f64, c[1] / corners.len() as f64, c[2] / corners.len() as f64];
        let ax = dnorm(dsub(p[corners[0]], c));
        let ay = dcross(n, ax);
        corners.sort_by(|&a, &b| {
            let ang = |k: usize| {
                let r = dsub(p[k], c);
                ddot(r, ay).atan2(ddot(r, ax))
            };
            ang(a).partial_cmp(&ang(b)).unwrap_or(std::cmp::Ordering::Equal)
        });
        // Drop corners on a straight edge.
        let mut k = 0;
        while corners.len() > 3 && k < corners.len() {
            let a = p[corners[(k + corners.len() - 1) % corners.len()]];
            let b = p[corners[k]];
            let cc = p[corners[(k + 1) % corners.len()]];
            if dlen(dcross(dsub(b, a), dsub(cc, b))) < 1e-6 * dlen(dsub(cc, a)).max(1.0) {
                corners.remove(k);
            } else {
                k += 1;
            }
        }
        faces.push(corners);
    }
    Some((verts, faces))
}

fn dsub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dadd(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn ddot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn dcross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dlen(a: [f64; 3]) -> f64 {
    ddot(a, a).sqrt()
}
fn dnorm(a: [f64; 3]) -> [f64; 3] {
    let l = dlen(a);
    if l == 0.0 {
        a
    } else {
        [a[0] / l, a[1] / l, a[2] / l]
    }
}

/// Vertices (input order) and outward triangles `(a, b, c)` with
/// `(b - a) × (c - a)` pointing out.
fn hull(points: &[V]) -> Option<(Vec<V>, Vec<[usize; 3]>)> {
    let mut uniq: Vec<V> = Vec::new();
    for p in points {
        add_vertex_if_not_present(&mut uniq, *p);
    }
    let p: Vec<[f64; 3]> = uniq.iter().map(|v| [v[0] as f64, v[1] as f64, v[2] as f64]).collect();
    if p.len() < 4 {
        return None;
    }
    let (lo, hi) = bbox(&uniq);
    let extent = (0..3).map(|k| (hi[k] - lo[k]) as f64).fold(0.0, f64::max);
    let eps = (extent * 1e-7).max(1e-6);
    // Initial tetrahedron from extreme points.
    let i0 = 0;
    let i1 = (0..p.len()).max_by(|&a, &b| dlen(dsub(p[a], p[i0])).partial_cmp(&dlen(dsub(p[b], p[i0]))).unwrap())?;
    let line = dsub(p[i1], p[i0]);
    let i2 = (0..p.len()).max_by(|&a, &b| dlen(dcross(line, dsub(p[a], p[i0]))).partial_cmp(&dlen(dcross(line, dsub(p[b], p[i0])))).unwrap())?;
    let n = dcross(line, dsub(p[i2], p[i0]));
    if dlen(n) <= eps * dlen(line) {
        return None;
    }
    let i3 = (0..p.len()).max_by(|&a, &b| ddot(n, dsub(p[a], p[i0])).abs().partial_cmp(&ddot(n, dsub(p[b], p[i0])).abs()).unwrap())?;
    if ddot(dnorm(n), dsub(p[i3], p[i0])).abs() <= eps {
        return None;
    }
    let mut faces: Vec<[usize; 3]> = Vec::new();
    let centroid = [0.25 * (p[i0][0] + p[i1][0] + p[i2][0] + p[i3][0]), 0.25 * (p[i0][1] + p[i1][1] + p[i2][1] + p[i3][1]), 0.25 * (p[i0][2] + p[i1][2] + p[i2][2] + p[i3][2])];
    let orient = |f: [usize; 3]| -> [usize; 3] {
        let n = dcross(dsub(p[f[1]], p[f[0]]), dsub(p[f[2]], p[f[0]]));
        if ddot(n, dsub(p[f[0]], centroid)) < 0.0 {
            [f[0], f[2], f[1]]
        } else {
            f
        }
    };
    for f in [[i0, i1, i2], [i0, i1, i3], [i0, i2, i3], [i1, i2, i3]] {
        faces.push(orient(f));
    }
    let outside = |f: &[usize; 3], q: [f64; 3]| -> f64 {
        let n = dnorm(dcross(dsub(p[f[1]], p[f[0]]), dsub(p[f[2]], p[f[0]])));
        ddot(n, dsub(q, p[f[0]]))
    };
    for (k, &q) in p.iter().enumerate() {
        if [i0, i1, i2, i3].contains(&k) {
            continue;
        }
        let visible: Vec<bool> = faces.iter().map(|f| outside(f, q) > eps).collect();
        if !visible.iter().any(|&v| v) {
            continue;
        }
        // Horizon: edges of visible faces whose twin face is not visible.
        let mut horizon = Vec::new();
        for (fi, f) in faces.iter().enumerate() {
            if !visible[fi] {
                continue;
            }
            for e in 0..3 {
                let (a, b) = (f[e], f[(e + 1) % 3]);
                let twin_visible = faces.iter().enumerate().any(|(gj, g)| gj != fi && visible[gj] && (0..3).any(|t| g[t] == b && g[(t + 1) % 3] == a));
                if !twin_visible {
                    horizon.push((a, b));
                }
            }
        }
        let mut kept: Vec<[usize; 3]> = faces.iter().zip(&visible).filter(|(_, v)| !**v).map(|(f, _)| *f).collect();
        for (a, b) in horizon {
            kept.push([a, b, k]);
        }
        faces = kept;
    }
    // Keep only the vertices the faces use, in input order.
    let mut map = vec![usize::MAX; p.len()];
    let mut verts = Vec::new();
    for (i, v) in uniq.iter().enumerate() {
        if faces.iter().any(|f| f.contains(&i)) {
            map[i] = verts.len();
            verts.push(*v);
        }
    }
    let faces = faces.into_iter().map(|f| [map[f[0]], map[f[1]], map[f[2]]]).collect();
    Some((verts, faces))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube() -> Vec<EdPoly> {
        build(&Shape::Box { x: 256.0, y: 256.0, z: 256.0, hollow: false, wall: 16.0, tessellated: false }).unwrap()
    }

    #[test]
    fn cube_brush() {
        let mut polys = cube();
        assert_eq!(polys.len(), 6);
        // Outward normals.
        for p in &polys {
            let c = p.vertices.iter().fold([0.0; 3], |a, v| add(a, *v));
            assert!(dot(c, p.normal) > 0.0);
        }
        let m = BrushModel::rebuild(&mut polys);
        // A convex brush: a chain of back children, one solid leaf.
        assert_eq!(m.nodes.len(), 6);
        assert_eq!(m.surfs.len(), 6);
        assert_eq!(m.points.len(), 8);
        assert_eq!(m.verts.len(), 24);
        assert_eq!(&m.leaf_hulls[..7], &[0, 1, 2, 3, 4, 5, -1]);
        let hulls = m.to_hulls();
        assert_eq!(hulls.len(), 1);
        let h = &hulls[0];
        assert_eq!(h.vertex_data.len(), 8);
        assert_eq!(h.face_tri_data.len(), 36);
        assert_eq!(h.face_plane_data.len(), 6);
        assert_eq!(h.face_normal_directions.len(), 3);
        assert_eq!(h.edge_directions.len(), 3);
        assert_eq!(h.permuted_vertex_data.len(), 6);
        assert_eq!(h.elem_box.0, [-128.0; 3]);
    }

    #[test]
    fn hollow_box_splits() {
        let mut polys = build(&Shape::Box { x: 512.0, y: 512.0, z: 512.0, hollow: true, wall: 32.0, tessellated: false }).unwrap();
        let m = BrushModel::rebuild(&mut polys);
        let hulls = m.to_hulls();
        assert!(hulls.len() >= 6, "{} hulls", hulls.len());
        // The hollow is not solid.
        for h in &hulls {
            let inside = h.face_plane_data.iter().all(|p| plane_dot(*p, [0.0; 3]) <= 0.0);
            assert!(!inside);
        }
    }

    #[test]
    fn shapes_build() {
        for s in [
            Shape::Cylinder { z: 256.0, outer: 512.0, inner: 384.0, sides: 8, align_to_side: true, hollow: false },
            Shape::Cone { z: 256.0, cap_z: 256.0, outer: 512.0, inner: 384.0, sides: 8, align_to_side: true, hollow: false },
            Shape::Sphere { radius: 256.0, extrapolation: 2 },
        ] {
            let mut polys = build(&s).unwrap();
            let m = BrushModel::rebuild(&mut polys);
            let hulls = m.to_hulls();
            assert_eq!(hulls.len(), 1, "{s:?}");
        }
        let mut polys = build(&Shape::Cylinder { z: 256.0, outer: 512.0, inner: 384.0, sides: 8, align_to_side: true, hollow: true }).unwrap();
        assert!(BrushModel::rebuild(&mut polys).to_hulls().len() >= 8);
    }

    #[test]
    fn hull_of_points() {
        let pts: Vec<V> = (0..200).map(|i| {
            let a = i as f32 * 0.37;
            let b = i as f32 * 0.91;
            [100.0 * a.cos() * b.sin(), 100.0 * a.sin() * b.sin(), 100.0 * b.cos()]
        }).collect();
        let e = ConvexElem::generate(&pts).unwrap();
        for t in e.draw_triangles() {
            let (a, b, c) = (e.vertex_data[t[0] as usize], e.vertex_data[t[1] as usize], e.vertex_data[t[2] as usize]);
            let n = cross(sub(b, a), sub(c, a));
            for v in &pts {
                assert!(dot(n, sub(*v, a)) / size(n).max(1e-6) < 0.01);
            }
        }
        let polys = build(&Shape::Hull { points: pts }).unwrap();
        assert!(polys.len() >= 4);
    }
}
