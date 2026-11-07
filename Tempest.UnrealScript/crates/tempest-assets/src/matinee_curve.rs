//! Matinee evaluation as the engine does it: `FInterpCurve::Eval` and
//! `FInterpCurve::AutoSetTangents` (UnMath.h, `InterpMethod` =
//! `IMT_UseFixedTangentEvalAndNewAutoTangents`, the only one cooked maps
//! use: no curve stores `InterpMethod`), and the move track's
//! `UInterpTrackMove::GetKeyTransformAtTime` / `ComputeWorldSpaceKeyTransform`
//! / `GetMoveRefFrame` / `UInterpTrackInstMove::CalcInitialTransform`
//! (UnInterpolation.cpp). Values are component lists (a float curve has one,
//! vectors three, linear colours four), computed in `f64`.
//!
//! This is the reference for the Studio's port (`matinee-math.ts`): both
//! give the same numbers for the curves in the tests below.

use crate::map::{mul, Mat, IDENTITY};

/// `EInterpCurveMode`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Linear,
    CurveAuto,
    Constant,
    CurveUser,
    CurveBreak,
    CurveAutoClamped,
}

impl Mode {
    pub fn parse(s: &str) -> Mode {
        match s.to_ascii_lowercase().as_str() {
            "cim_curveauto" => Mode::CurveAuto,
            "cim_constant" => Mode::Constant,
            "cim_curveuser" => Mode::CurveUser,
            "cim_curvebreak" => Mode::CurveBreak,
            "cim_curveautoclamped" => Mode::CurveAutoClamped,
            _ => Mode::Linear,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::Linear => "CIM_Linear",
            Mode::CurveAuto => "CIM_CurveAuto",
            Mode::Constant => "CIM_Constant",
            Mode::CurveUser => "CIM_CurveUser",
            Mode::CurveBreak => "CIM_CurveBreak",
            Mode::CurveAutoClamped => "CIM_CurveAutoClamped",
        }
    }

    /// `FInterpCurvePoint::IsCurveKey`.
    pub fn is_curve(self) -> bool {
        matches!(self, Mode::CurveAuto | Mode::CurveAutoClamped | Mode::CurveUser | Mode::CurveBreak)
    }

    fn is_auto(self) -> bool {
        matches!(self, Mode::CurveAuto | Mode::CurveAutoClamped)
    }
}

/// `FInterpCurvePoint<T>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    pub time: f64,
    pub out: Vec<f64>,
    pub arrive: Vec<f64>,
    pub leave: Vec<f64>,
    pub mode: Mode,
}

const KINDA_SMALL_NUMBER: f64 = 1e-4;

/// `CubicInterp` (UnTemplate.h): Hermite with tangents.
fn cubic(p0: f64, t0: f64, p1: f64, t1: f64, a: f64) -> f64 {
    let a2 = a * a;
    let a3 = a2 * a;
    (2.0 * a3 - 3.0 * a2 + 1.0) * p0 + (a3 - 2.0 * a2 + a) * t0 + (a3 - a2) * t1 + (-2.0 * a3 + 3.0 * a2) * p1
}

fn at(v: &[f64], k: usize) -> f64 {
    v.get(k).copied().unwrap_or(0.0)
}

/// `FInterpCurve::Eval`: the value at `t` (the first/last key's outside
/// the keys, `default` without keys).
pub fn eval(points: &[Point], t: f64, default: &[f64]) -> Vec<f64> {
    let n = points.len();
    if n == 0 {
        return default.to_vec();
    }
    if n < 2 || t <= points[0].time {
        return points[0].out.clone();
    }
    if t >= points[n - 1].time {
        return points[n - 1].out.clone();
    }
    for i in 1..n {
        if t < points[i].time {
            let (a, b) = (&points[i - 1], &points[i]);
            let diff = b.time - a.time;
            if diff > 0.0 && a.mode != Mode::Constant {
                let alpha = (t - a.time) / diff;
                return (0..a.out.len())
                    .map(|k| if a.mode == Mode::Linear { at(&a.out, k) + (at(&b.out, k) - at(&a.out, k)) * alpha } else { cubic(at(&a.out, k), at(&a.leave, k) * diff, at(&b.out, k), at(&b.arrive, k) * diff, alpha) })
                    .collect();
            }
            return a.out.clone();
        }
    }
    points[n - 1].out.clone()
}

/// `FClampFloatTangent` (UnMath.cpp).
pub fn clamp_float_tangent(prev: f64, prev_t: f64, cur: f64, cur_t: f64, next: f64, next_t: f64) -> f64 {
    let prev_to_next_t = (next_t - prev_t).max(KINDA_SMALL_NUMBER);
    let prev_to_cur_t = (cur_t - prev_t).max(KINDA_SMALL_NUMBER);
    let cur_to_next_t = (next_t - cur_t).max(KINDA_SMALL_NUMBER);
    let prev_to_next_h = next - prev;
    let prev_to_cur_h = cur - prev;
    let cur_to_next_h = next - cur;
    // A crest: both neighbours on the same side.
    if (prev_to_cur_h >= 0.0 && cur_to_next_h <= 0.0) || (prev_to_cur_h <= 0.0 && cur_to_next_h >= 0.0) {
        return 0.0;
    }
    const CLAMP_THRESHOLD: f64 = 0.333;
    let cur_to_next_tan = cur_to_next_h / cur_to_next_t;
    let prev_to_cur_tan = prev_to_cur_h / prev_to_cur_t;
    let prev_to_next_tan = prev_to_next_h / prev_to_next_t;
    let mut clamped = prev_to_next_tan;
    let lower = CLAMP_THRESHOLD;
    let upper = 1.0 - CLAMP_THRESHOLD;
    let alpha = prev_to_cur_h / prev_to_next_h;
    let lerp = |a: f64, b: f64, x: f64| a + (b - a) * x;
    if prev_to_next_h > 0.0 {
        if alpha < lower {
            clamped = clamped.min(lerp(prev_to_next_tan, prev_to_cur_tan, 1.0 - alpha / CLAMP_THRESHOLD));
        }
        if alpha > upper {
            clamped = clamped.min(lerp(prev_to_next_tan, cur_to_next_tan, (alpha - upper) / CLAMP_THRESHOLD));
        }
    } else {
        if alpha < lower {
            clamped = clamped.max(lerp(prev_to_next_tan, prev_to_cur_tan, 1.0 - alpha / CLAMP_THRESHOLD));
        }
        if alpha > upper {
            clamped = clamped.max(lerp(prev_to_next_tan, cur_to_next_tan, (alpha - upper) / CLAMP_THRESHOLD));
        }
    }
    clamped
}

/// `ComputeCurveTangent`: per component; `clampable` is false for the types
/// the engine has no clamping overload for (`FLinearColor`, `FTwoVectors`
/// … go through the general template, which ignores `bWantClamping`).
fn curve_tangent(prev: &Point, cur: &Point, next: &Point, tension: f64, clamp: bool) -> Vec<f64> {
    let diff = (next.time - prev.time).max(KINDA_SMALL_NUMBER);
    (0..cur.out.len())
        .map(|k| {
            if clamp {
                (1.0 - tension) * clamp_float_tangent(at(&prev.out, k), prev.time, at(&cur.out, k), cur.time, at(&next.out, k), next.time)
            } else {
                // AutoCalcTangent, divided by the time span.
                (1.0 - tension) * ((at(&cur.out, k) - at(&prev.out, k)) + (at(&next.out, k) - at(&cur.out, k))) / diff
            }
        })
        .collect()
}

/// `FInterpCurve::AutoSetTangents(Tension)`: recompute the tangents of
/// every auto key (`CIM_CurveAuto`, `CIM_CurveAutoClamped`); user and break
/// keys keep theirs.
pub fn auto_set_tangents(points: &mut [Point], tension: f64, clampable: bool) {
    let n = points.len();
    for i in 0..n {
        let width = points[i].out.len();
        let zero = vec![0.0; width];
        let mut arrive = points[i].arrive.clone();
        let mut leave = points[i].leave.clone();
        let mode = points[i].mode;
        if i == 0 {
            // Start point (or only point).
            if n == 1 || mode.is_auto() {
                leave = zero;
            }
        } else if i < n - 1 {
            if mode.is_auto() {
                if points[i - 1].mode.is_curve() && mode.is_curve() {
                    arrive = curve_tangent(&points[i - 1], &points[i], &points[i + 1], tension, clampable && mode == Mode::CurveAutoClamped);
                    leave = arrive.clone();
                } else if points[i - 1].mode == Mode::Constant || mode == Mode::Constant {
                    arrive = zero.clone();
                    leave = zero;
                }
            }
        } else if mode.is_auto() {
            // End point.
            arrive = zero;
        }
        points[i].arrive = arrive;
        points[i].leave = leave;
    }
}

// ----- move tracks -------------------------------------------------------------

/// `FRotator::MakeFromEuler`: Euler degrees (X roll, Y pitch, Z yaw) to a
/// rotator (pitch, yaw, roll in 65536ths of a turn, truncated).
pub fn rotator_from_euler(e: &[f64]) -> [f64; 3] {
    let k = 32768.0 / 180.0;
    [(at(e, 1) * k).trunc(), (at(e, 2) * k).trunc(), (at(e, 0) * k).trunc()]
}

/// `UInterpTrackMove::GetKeyTransformAtTime` for a track without lookup
/// keys and without quaternion interpolation (no cooked track sets
/// `bUseQuatInterpolation` or a lookup group): the relative position and
/// rotator. `axes` (`SubTracks`: translation X Y Z, rotation X Y Z) win over
/// `pos`/`euler` when the track is split.
pub fn key_transform(pos: &[Point], euler: &[Point], axes: Option<&[Vec<Point>; 6]>, t: f64) -> ([f64; 3], [f64; 3]) {
    let (p, e) = match axes {
        Some(a) => {
            let v = |k: usize| eval(&a[k], t, &[0.0]).first().copied().unwrap_or(0.0);
            (vec![v(0), v(1), v(2)], vec![v(3), v(4), v(5)])
        }
        None => (eval(pos, t, &[0.0; 3]), eval(euler, t, &[0.0; 3])),
    };
    ([at(&p, 0), at(&p, 1), at(&p, 2)], rotator_from_euler(&e))
}

/// `FRotationTranslationMatrix`.
pub fn rotation_translation(rot: [f64; 3], pos: [f64; 3]) -> Mat {
    let mut m = crate::map::rotation(rot[0], rot[1], rot[2]);
    m[3] = [pos[0], pos[1], pos[2], 1.0];
    m
}

/// `FMatrix::RemoveScaling`.
fn remove_scaling(m: &mut Mat) {
    for row in m.iter_mut().take(3) {
        let len = (row[0] * row[0] + row[1] * row[1] + row[2] * row[2]).sqrt();
        if len > 1e-8 {
            for x in row.iter_mut().take(3) {
                *x /= len;
            }
        }
    }
}

/// `UInterpTrackInstMove::CalcInitialTransform` for an actor without a
/// base: the actor's `Rotation`/`Location` frame, less the track's own
/// transform at `start` (the action's position when it starts, 0 unless
/// `bForceStartPos`) unless `raw` (`bUseRawActorTMforRelativeToInitial`).
pub fn initial_tm(actor_rot: [f64; 3], actor_loc: [f64; 3], start: ([f64; 3], [f64; 3]), raw: bool) -> Mat {
    let actor = rotation_translation(actor_rot, actor_loc);
    let mut m = if raw { actor } else { mul(&crate::map::inverse(&rotation_translation(start.1, start.0)), &actor) };
    remove_scaling(&mut m);
    m
}

/// `GetMoveRefFrame` (no base) then `ComputeWorldSpaceKeyTransform`, as a
/// matrix: the key's rotation and position in the reference frame
/// (`IMF_World`: the world; `IMF_RelativeToInitial`: `initial`). The
/// engine splits the rotator into winding and remainder to keep the
/// winding in the actor's `Rotation`; the orientation is the same.
pub fn world_transform(rel_pos: [f64; 3], rel_rot: [f64; 3], initial: Option<&Mat>) -> Mat {
    let local = rotation_translation(rel_rot, rel_pos);
    mul(&local, initial.unwrap_or(&IDENTITY))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(time: f64, out: &[f64], mode: Mode) -> Point {
        Point { time, out: out.to_vec(), arrive: vec![0.0; out.len()], leave: vec![0.0; out.len()], mode }
    }

    /// The curves `matinee-math.ts` is checked against (same numbers).
    pub fn fixtures() -> Vec<(Vec<Point>, f64)> {
        vec![
            (vec![pt(0.0, &[0.0], Mode::CurveAutoClamped), pt(1.0, &[10.0], Mode::CurveAutoClamped), pt(2.5, &[4.0], Mode::CurveAutoClamped), pt(4.0, &[5.0], Mode::CurveAutoClamped)], 0.0),
            (vec![pt(0.0, &[0.0, 0.0, 0.0], Mode::CurveAuto), pt(0.4, &[0.0, 0.0, -216.0], Mode::CurveAuto), pt(1.0, &[50.0, 20.0, -100.0], Mode::CurveAuto)], 0.25),
            (vec![pt(0.0, &[1.0], Mode::Linear), pt(1.0, &[3.0], Mode::Constant), pt(2.0, &[0.0], Mode::CurveAutoClamped), pt(3.0, &[2.0], Mode::CurveAuto), pt(4.0, &[1.0], Mode::Linear)], 0.0),
            (vec![pt(0.0, &[1.0, 0.5, 0.0, 1.0], Mode::CurveAutoClamped), pt(2.0, &[0.0, 1.0, 1.0, 1.0], Mode::CurveAutoClamped), pt(3.0, &[1.0, 1.0, 0.0, 0.5], Mode::CurveAutoClamped)], 0.0),
        ]
    }

    #[test]
    fn auto_tangents_and_eval() {
        let mut out = Vec::new();
        for (k, (mut pts, tension)) in fixtures().into_iter().enumerate() {
            // Linear colours have no clamping overload.
            auto_set_tangents(&mut pts, tension, k != 3);
            let samples: Vec<Vec<f64>> = (0..=20).map(|i| eval(&pts, -0.5 + i as f64 * 0.25, &[0.0])).collect();
            out.push(samples);
            if k == 0 {
                // A crest (10 between 0 and 4) gets a flat tangent when clamped.
                assert_eq!(pts[1].arrive, vec![0.0]);
                // 4 → 5 continues the slope from 10: clamped toward the next segment.
                assert!(pts[2].arrive[0] == 0.0 || pts[2].arrive[0].abs() < 4.0);
            }
            if k == 2 {
                // A key after a constant key has zero tangents.
                assert_eq!(pts[2].arrive, vec![0.0]);
                assert_eq!(eval(&pts, 1.5, &[0.0]), vec![3.0]);
            }
        }
        // Printed for the TypeScript comparison (`cargo test -- --nocapture`).
        println!("MATINEE_FIXTURES {}", serde_json::to_string(&out).unwrap());
        // Hermite at the middle of a linear-free segment: ends match the keys.
        let mut pts = fixtures().remove(1).0;
        auto_set_tangents(&mut pts, 0.25, true);
        assert_eq!(eval(&pts, 0.4, &[0.0; 3]), vec![0.0, 0.0, -216.0]);
    }

    #[test]
    fn relative_to_initial() {
        let pos = vec![pt(0.0, &[0.0, 0.0, 0.0], Mode::Linear), pt(1.0, &[0.0, 0.0, -216.0], Mode::Linear)];
        let euler = vec![pt(0.0, &[0.0, 0.0, 0.0], Mode::Linear), pt(1.0, &[0.0, 0.0, 90.0], Mode::Linear)];
        let start = key_transform(&pos, &euler, None, 0.0);
        let init = initial_tm([0.0, 16384.0, 0.0], [100.0, 200.0, 300.0], start, false);
        let (p, r) = key_transform(&pos, &euler, None, 1.0);
        assert_eq!(r, [0.0, 16384.0, 0.0]);
        let w = world_transform(p, r, Some(&init));
        // Down 216 from the actor's location, turned another quarter.
        assert!((w[3][2] - 84.0).abs() < 1e-6 && (w[3][0] - 100.0).abs() < 1e-6);
        assert!((w[0][0] + 1.0).abs() < 1e-6);
    }
}
