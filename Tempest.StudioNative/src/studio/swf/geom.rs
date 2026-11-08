//! Placement matrices as the movie stores them (pixels; `a b c d` scale,
//! rotate and skew, `tx ty` translate) and the operations the stage's tools
//! compose them with.

use serde_json::{json, Value as J};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct M {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}

pub const IDENTITY: M = M { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 };

/// A rectangle in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn from_json(v: &J) -> Option<Rect> {
        Some(Rect { x: v["x"].as_f64()?, y: v["y"].as_f64()?, w: v["w"].as_f64()?, h: v["h"].as_f64()? })
    }

    pub fn json(&self) -> J {
        json!({ "x": self.x, "y": self.y, "w": self.w, "h": self.h })
    }

    pub fn union(&self, o: &Rect) -> Rect {
        let (x, y) = (self.x.min(o.x), self.y.min(o.y));
        Rect { x, y, w: (self.x + self.w).max(o.x + o.w) - x, h: (self.y + self.h).max(o.y + o.h) - y }
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }

    /// Entirely outside `o`.
    pub fn outside(&self, o: &Rect) -> bool {
        self.x >= o.x + o.w || self.y >= o.y + o.h || self.x + self.w <= o.x || self.y + self.h <= o.y
    }

    /// Overlap over union: nearly the same rectangle when ≥ 0.75.
    pub fn overlap(&self, o: &Rect) -> f64 {
        let w = (self.x + self.w).min(o.x + o.w) - self.x.max(o.x);
        let h = (self.y + self.h).min(o.y + o.h) - self.y.max(o.y);
        if w <= 0.0 || h <= 0.0 {
            return 0.0;
        }
        let inter = w * h;
        inter / (self.w * self.h + o.w * o.h - inter)
    }
}

impl M {
    pub fn from_json(v: &J) -> M {
        let g = |k: &str, d: f64| v[k].as_f64().unwrap_or(d);
        M { a: g("a", 1.0), b: g("b", 0.0), c: g("c", 0.0), d: g("d", 1.0), tx: g("tx", 0.0), ty: g("ty", 0.0) }
    }

    pub fn json(&self) -> J {
        json!({ "a": self.a, "b": self.b, "c": self.c, "d": self.d, "tx": self.tx, "ty": self.ty })
    }

    /// `self · o` (o applied first).
    pub fn mul(&self, o: &M) -> M {
        M {
            a: self.a * o.a + self.c * o.b,
            b: self.b * o.a + self.d * o.b,
            c: self.a * o.c + self.c * o.d,
            d: self.b * o.c + self.d * o.d,
            tx: self.a * o.tx + self.c * o.ty + self.tx,
            ty: self.b * o.tx + self.d * o.ty + self.ty,
        }
    }

    pub fn invert(&self) -> Option<M> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < 1e-12 {
            return None;
        }
        Some(M { a: self.d / det, b: -self.b / det, c: -self.c / det, d: self.a / det, tx: (self.c * self.ty - self.d * self.tx) / det, ty: (self.b * self.tx - self.a * self.ty) / det })
    }

    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.c * y + self.tx, self.b * x + self.d * y + self.ty)
    }

    pub fn translate(tx: f64, ty: f64) -> M {
        M { tx, ty, ..IDENTITY }
    }

    pub fn scaling(sx: f64, sy: f64) -> M {
        M { a: sx, d: sy, ..IDENTITY }
    }

    pub fn rotation(r: f64) -> M {
        let (s, c) = r.sin_cos();
        M { a: c, b: s, c: -s, d: c, tx: 0.0, ty: 0.0 }
    }
}

/// Translation, scale (1 = 100 %), rotation and skew (degrees).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parts {
    pub x: f64,
    pub y: f64,
    pub sx: f64,
    pub sy: f64,
    pub rot: f64,
    pub skew: f64,
}

fn degrees(r: f64) -> f64 {
    let mut d = r.to_degrees();
    while d > 180.0 {
        d -= 360.0;
    }
    while d <= -180.0 {
        d += 360.0;
    }
    if d.abs() < 1e-6 { 0.0 } else { d }
}

pub fn decompose(m: &M) -> Parts {
    let sx = m.a.hypot(m.b);
    let det = m.a * m.d - m.b * m.c;
    let rot = m.b.atan2(m.a);
    let sign = if det < 0.0 { -1.0 } else { 1.0 };
    let sy = sign * m.c.hypot(m.d);
    // The column of a mirrored matrix points the other way: flip it before measuring its angle.
    let skew = (-m.c * sign).atan2(m.d * sign) - rot;
    Parts { x: m.tx, y: m.ty, sx, sy, rot: degrees(rot), skew: degrees(skew) }
}

pub fn compose(p: &Parts) -> M {
    let r = p.rot.to_radians();
    let k = r + p.skew.to_radians();
    M { a: p.sx * r.cos(), b: p.sx * r.sin(), c: -p.sy * k.sin(), d: p.sy * k.cos(), tx: p.x, ty: p.y }
}

/// Twips are the file's unit: keep pixel values on the 1/20 grid.
pub fn twips(v: f64) -> f64 {
    (v * 20.0).round() / 20.0
}

/// A number as the editor shows it: up to `d` decimals, no trailing zeros.
pub fn fmt(v: f64, d: usize) -> String {
    if v.abs() < 1e-9 {
        return "0".into();
    }
    let s = format!("{v:.d$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &M, b: &M) -> bool {
        [a.a - b.a, a.b - b.b, a.c - b.c, a.d - b.d, a.tx - b.tx, a.ty - b.ty].iter().all(|v| v.abs() < 1e-9)
    }

    #[test]
    fn decompose_round_trips() {
        for p in [Parts { x: 12.5, y: -3.0, sx: 1.5, sy: 0.75, rot: 30.0, skew: 0.0 }, Parts { x: 0.0, y: 0.0, sx: 1.0, sy: -2.0, rot: -45.0, skew: 10.0 }] {
            let m = compose(&p);
            let q = decompose(&m);
            assert!(close(&compose(&q), &m), "{p:?} → {q:?}");
        }
        assert_eq!(decompose(&IDENTITY), Parts { x: 0.0, y: 0.0, sx: 1.0, sy: 1.0, rot: 0.0, skew: 0.0 });
    }

    #[test]
    fn mul_and_invert() {
        let m = M::translate(10.0, 5.0).mul(&M::rotation(0.3)).mul(&M::scaling(2.0, 3.0));
        let i = m.invert().unwrap();
        assert!(close(&m.mul(&i), &IDENTITY));
        let (x, y) = m.apply(1.0, 1.0);
        let (u, v) = i.apply(x, y);
        assert!((u - 1.0).abs() < 1e-9 && (v - 1.0).abs() < 1e-9);
    }

    #[test]
    fn formats() {
        assert_eq!(fmt(1.5, 2), "1.5");
        assert_eq!(fmt(2.0, 2), "2");
        assert_eq!(fmt(-0.0000000001, 2), "0");
        assert_eq!(twips(1.026), 1.05);
    }
}
