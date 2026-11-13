//! Non-destructive modifiers: the object keeps its base mesh, and these are applied on top
//! (in stack order) whenever the result is needed — for display, picking, export.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use crate::math::{v3, V3};
use crate::mesh::Mesh;
use crate::ops::clean_faces;

#[derive(Clone, Debug, PartialEq)]
pub enum Modifier {
    /// Reflects across the planes through the object's origin, for each axis switched on.
    Mirror { enabled: bool, axes: [bool; 3], merge: bool },
    /// `count` copies, each shifted along `axis` by `spacing` times the input's size on that axis.
    Array { enabled: bool, count: u32, spacing: f32, axis: u8, merge: bool },
}

pub const KIND_MIRROR: i32 = 0;
pub const KIND_ARRAY: i32 = 1;
pub const MAX_ARRAY: u32 = 64;

impl Modifier {
    pub fn new(kind: i32) -> Option<Modifier> {
        Some(match kind {
            KIND_MIRROR => Modifier::Mirror { enabled: true, axes: [true, false, false], merge: true },
            KIND_ARRAY => Modifier::Array { enabled: true, count: 3, spacing: 1.0, axis: 0, merge: false },
            _ => return None,
        })
    }

    pub fn enabled(&self) -> bool {
        match self {
            Modifier::Mirror { enabled, .. } | Modifier::Array { enabled, .. } => *enabled,
        }
    }

    /// Sets one parameter. Mirror: 0 enabled, 1-3 X/Y/Z, 4 merge.
    /// Array: 0 enabled, 1 count, 2 spacing, 3 axis (0-2), 4 merge. Returns false if unknown.
    pub fn set(&mut self, param: i32, value: f32) -> bool {
        let on = value != 0.0;
        match self {
            Modifier::Mirror { enabled, axes, merge } => match param {
                0 => *enabled = on,
                1..=3 => axes[(param - 1) as usize] = on,
                4 => *merge = on,
                _ => return false,
            },
            Modifier::Array { enabled, count, spacing, axis, merge } => match param {
                0 => *enabled = on,
                1 => *count = (value.round().max(1.0) as u32).min(MAX_ARRAY),
                2 => *spacing = value.clamp(-10.0, 10.0),
                3 => *axis = (value.round().clamp(0.0, 2.0)) as u8,
                4 => *merge = on,
                _ => return false,
            },
        }
        true
    }

    pub fn to_json(&self) -> String {
        match self {
            Modifier::Mirror { enabled, axes, merge } => format!(
                "{{\"kind\":\"mirror\",\"enabled\":{enabled},\"axes\":[{},{},{}],\"merge\":{merge}}}",
                axes[0], axes[1], axes[2]
            ),
            Modifier::Array { enabled, count, spacing, axis, merge } => format!(
                "{{\"kind\":\"array\",\"enabled\":{enabled},\"count\":{count},\"spacing\":{spacing},\"axis\":{axis},\"merge\":{merge}}}"
            ),
        }
    }
}

/// The result of the whole stack. Borrows the base mesh when nothing changes it.
pub fn evaluate<'a>(base: &'a Mesh, origin: V3, mods: &[Modifier]) -> Cow<'a, Mesh> {
    let mut out = Cow::Borrowed(base);
    for m in mods.iter().filter(|m| m.enabled()) {
        out = Cow::Owned(match m {
            Modifier::Mirror { axes, merge, .. } => {
                let mut cur = out.into_owned();
                for (axis, _) in axes.iter().enumerate().filter(|(_, on)| **on) {
                    cur = mirror(&cur, origin, axis, *merge);
                }
                cur
            }
            Modifier::Array { count, spacing, axis, merge, .. } => array(&out, *count, *spacing, *axis as usize, *merge),
        });
    }
    out
}

fn get(p: V3, axis: usize) -> f32 {
    [p.x, p.y, p.z][axis]
}

fn with(p: V3, axis: usize, value: f32) -> V3 {
    match axis {
        0 => v3(value, p.y, p.z),
        1 => v3(p.x, value, p.z),
        _ => v3(p.x, p.y, value),
    }
}

/// Size-aware tolerance for "on the plane" / "the same point".
fn tolerance(m: &Mesh) -> f32 {
    m.bounds().map_or(1e-4, |(lo, hi)| (hi.sub(lo).len() * 1e-4).max(1e-5))
}

fn mirror(m: &Mesh, origin: V3, axis: usize, merge: bool) -> Mesh {
    let eps = tolerance(m);
    let o = get(origin, axis);
    let n = m.verts.len() as u32;
    let mut verts = m.verts.clone();
    let mut map = Vec::with_capacity(m.verts.len());
    for (i, &p) in m.verts.iter().enumerate() {
        if merge && (get(p, axis) - o).abs() <= eps {
            map.push(i as u32); // on the plane: shared by both halves
        } else {
            map.push(verts.len() as u32);
            verts.push(with(p, axis, 2.0 * o - get(p, axis)));
        }
    }
    let mut faces = m.faces.clone();
    // Reflection flips handedness, so reverse each mirrored face to keep it facing outwards.
    faces.extend(m.faces.iter().map(|f| f.iter().rev().map(|&i| map[i as usize]).collect::<Vec<u32>>()));
    let _ = n;
    finish(verts, faces, merge)
}

fn array(m: &Mesh, count: u32, spacing: f32, axis: usize, merge: bool) -> Mesh {
    let count = count.clamp(1, MAX_ARRAY);
    let size = m.bounds().map_or(1.0, |(lo, hi)| get(hi.sub(lo), axis)).max(1e-6);
    let step = size * spacing;
    let n = m.verts.len() as u32;
    let mut verts = Vec::with_capacity(m.verts.len() * count as usize);
    let mut faces = Vec::with_capacity(m.faces.len() * count as usize);
    for k in 0..count {
        let shift = with(v3(0.0, 0.0, 0.0), axis, step * k as f32);
        verts.extend(m.verts.iter().map(|p| p.add(shift)));
        faces.extend(m.faces.iter().map(|f| f.iter().map(|&i| i + k * n).collect::<Vec<u32>>()));
    }
    if merge {
        let eps = tolerance(m);
        let (verts, faces) = weld(verts, faces, eps);
        finish(verts, faces, true)
    } else {
        Mesh { verts, faces }
    }
}

/// Merges points closer than `eps`.
fn weld(verts: Vec<V3>, faces: Vec<Vec<u32>>, eps: f32) -> (Vec<V3>, Vec<Vec<u32>>) {
    let q = |x: f32| (x / eps).round() as i64;
    let mut keys: BTreeMap<(i64, i64, i64), u32> = BTreeMap::new();
    let mut out = Vec::new();
    let remap: Vec<u32> = verts
        .iter()
        .map(|p| {
            *keys.entry((q(p.x), q(p.y), q(p.z))).or_insert_with(|| {
                out.push(*p);
                out.len() as u32 - 1
            })
        })
        .collect();
    let faces = faces.into_iter().map(|f| f.into_iter().map(|i| remap[i as usize]).collect()).collect();
    (out, faces)
}

/// Drops collapsed faces, faces that now sit back to back inside the solid (both copies of a
/// face on the mirror plane, the touching ends of merged array copies), and unused points.
fn finish(verts: Vec<V3>, faces: Vec<Vec<u32>>, remove_internal: bool) -> Mesh {
    let mut faces = clean_faces(faces);
    if remove_internal {
        let key = |f: &Vec<u32>| {
            let mut k = f.clone();
            k.sort_unstable();
            k
        };
        let mut count: BTreeMap<Vec<u32>, usize> = BTreeMap::new();
        for f in &faces {
            *count.entry(key(f)).or_default() += 1;
        }
        faces.retain(|f| count[&key(f)] == 1);
    }
    let used: BTreeSet<u32> = faces.iter().flatten().copied().collect();
    let mut remap = vec![u32::MAX; verts.len()];
    let mut kept = Vec::with_capacity(used.len());
    for (i, p) in verts.iter().enumerate() {
        if used.contains(&(i as u32)) {
            remap[i] = kept.len() as u32;
            kept.push(*p);
        }
    }
    for f in &mut faces {
        for i in f.iter_mut() {
            *i = remap[*i as usize];
        }
    }
    Mesh { verts: kept, faces }
}

/// Bakes the stack into a plain mesh (for "apply").
pub fn apply_all(base: &Mesh, origin: V3, mods: &[Modifier]) -> Mesh {
    evaluate(base, origin, mods).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed(m: &Mesh) -> bool {
        let mut dir: BTreeMap<(u32, u32), i32> = BTreeMap::new();
        for f in &m.faces {
            for k in 0..f.len() {
                *dir.entry((f[k], f[(k + 1) % f.len()])).or_default() += 1;
            }
        }
        dir.iter().all(|(&(a, b), &c)| c == 1 && dir.get(&(b, a)) == Some(&1))
    }

    fn volume(m: &Mesh) -> f32 {
        let mut v = 0.0;
        for f in &m.faces {
            for k in 1..f.len() - 1 {
                let (a, b, c) = (m.verts[f[0] as usize], m.verts[f[k] as usize], m.verts[f[k + 1] as usize]);
                v += a.dot(b.cross(c)) / 6.0;
            }
        }
        v
    }

    /// The +X half of a 2x2x2 cube: open where it was cut at x = 0.
    fn half_cube() -> Mesh {
        let mut m = Mesh::cube(1.0);
        for v in &mut m.verts {
            v.x = v.x.max(0.0);
        }
        m.faces.retain(|f| f.iter().any(|&i| m.verts[i as usize].x > 0.5));
        m
    }

    fn mirror_x(merge: bool) -> Modifier {
        Modifier::Mirror { enabled: true, axes: [true, false, false], merge }
    }

    #[test]
    fn mirror_closes_a_half_model_across_the_origin() {
        let half = half_cube();
        assert!(!closed(&half));
        let m = evaluate(&half, v3(0.0, 0.0, 0.0), &[mirror_x(true)]);
        assert!(closed(&m), "the seam is welded");
        assert_eq!((m.verts.len(), m.faces.len()), (12, 10));
        assert!((volume(&m) - 8.0).abs() < 1e-4, "outward and complete: {}", volume(&m));
        let unmerged = evaluate(&half, v3(0.0, 0.0, 0.0), &[mirror_x(false)]);
        assert_eq!(unmerged.verts.len(), 16, "without merging the seam stays open");
    }

    #[test]
    fn mirror_uses_the_origin_and_drops_faces_on_the_plane() {
        // A whole cube mirrored around x = 1 (its right face sits on the plane).
        let m = apply_all(&Mesh::cube(1.0), v3(1.0, 0.0, 0.0), &[mirror_x(true)]);
        assert!(closed(&m));
        let (lo, hi) = m.bounds().unwrap();
        assert_eq!((lo.x, hi.x), (-1.0, 3.0));
        assert!((volume(&m) - 16.0).abs() < 1e-4, "the shared face is gone from the inside");
    }

    #[test]
    fn mirror_on_two_axes_makes_four_quarters() {
        let mut m = half_cube();
        for v in &mut m.verts {
            v.y = v.y.max(0.0);
        }
        m.faces.retain(|f| f.iter().any(|&i| m.verts[i as usize].y > 0.5));
        let r = evaluate(&m, v3(0.0, 0.0, 0.0), &[Modifier::Mirror { enabled: true, axes: [true, true, false], merge: true }]);
        assert!(closed(&r));
        assert!((volume(&r) - 8.0).abs() < 1e-4);
    }

    #[test]
    fn array_copies_side_by_side_and_merges_into_one_bar() {
        let arr = |merge| Modifier::Array { enabled: true, count: 3, spacing: 1.0, axis: 0, merge };
        let m = apply_all(&Mesh::cube(1.0), v3(0.0, 0.0, 0.0), &[arr(false)]);
        assert_eq!((m.verts.len(), m.faces.len()), (24, 18));
        let (lo, hi) = m.bounds().unwrap();
        assert_eq!((lo.x, hi.x), (-1.0, 5.0), "touching copies with spacing 1");
        let bar = apply_all(&Mesh::cube(1.0), v3(0.0, 0.0, 0.0), &[arr(true)]);
        assert!(closed(&bar));
        assert_eq!((bar.verts.len(), bar.faces.len()), (16, 14));
        assert!((volume(&bar) - 24.0).abs() < 1e-3);
        let gap = apply_all(&Mesh::cube(1.0), v3(0.0, 0.0, 0.0), &[Modifier::Array { enabled: true, count: 2, spacing: -1.5, axis: 2, merge: false }]);
        let (lo, _) = gap.bounds().unwrap();
        assert_eq!(lo.z, -4.0, "negative spacing goes the other way");
    }

    #[test]
    fn stack_order_disabled_and_borrowed() {
        let base = Mesh::cube(1.0);
        let off = Modifier::Array { enabled: false, count: 5, spacing: 1.0, axis: 0, merge: false };
        assert!(matches!(evaluate(&base, v3(0.0, 0.0, 0.0), &[off]), Cow::Borrowed(_)), "nothing to do, no copy");
        // Array then mirror: 2 copies to +X, then mirrored around x = 0 → 4 cubes across -5..5.
        let mods = [Modifier::Array { enabled: true, count: 2, spacing: 1.5, axis: 0, merge: false }, mirror_x(true)];
        let r = evaluate(&base, v3(0.0, 0.0, 0.0), &mods);
        let (lo, hi) = r.bounds().unwrap();
        assert_eq!((lo.x, hi.x), (-4.0, 4.0));
    }

    #[test]
    fn parameters_are_clamped() {
        let mut a = Modifier::new(KIND_ARRAY).unwrap();
        assert!(a.set(1, 1000.0) && a.set(3, 7.0) && a.set(2, -99.0));
        assert_eq!(a, Modifier::Array { enabled: true, count: MAX_ARRAY, spacing: -10.0, axis: 2, merge: false });
        assert!(!a.set(9, 1.0));
        let mut m = Modifier::new(KIND_MIRROR).unwrap();
        assert!(m.set(2, 1.0) && m.set(1, 0.0) && m.set(4, 0.0));
        assert_eq!(m, Modifier::Mirror { enabled: true, axes: [false, true, false], merge: false });
        assert!(Modifier::new(5).is_none());
    }
}
