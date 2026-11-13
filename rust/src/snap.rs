//! Snapping for transform drags: contact with other objects' surfaces along the drag axis,
//! otherwise the grid. All functions are pure so they are easy to test.

use crate::math::{v3, V3};
use crate::mesh::Mesh;

/// Rotation snaps to multiples of this (15°).
pub const ANGLE_STEP: f32 = std::f32::consts::PI / 12.0;
/// Scale snaps to multiples of this factor.
pub const SCALE_STEP: f32 = 0.1;

/// Grid spacing that suits the current zoom: 0.1 up close, 1 further out, ...
pub fn grid_step(view_distance: f32) -> f32 {
    10f32.powf((view_distance * 0.05).max(1e-4).log10().floor())
}

fn comp(p: V3, axis: usize) -> f32 {
    match axis {
        0 => p.x,
        1 => p.y,
        _ => p.z,
    }
}

fn axis_vec(axis: usize) -> V3 {
    match axis {
        0 => v3(1.0, 0.0, 0.0),
        1 => v3(0.0, 1.0, 0.0),
        _ => v3(0.0, 0.0, 1.0),
    }
}

/// Signed distance along `dir` from `o` to triangle (a, b, c), if the line through `o` hits it.
fn ray_triangle(o: V3, dir: V3, a: V3, b: V3, c: V3) -> Option<f32> {
    let (e1, e2) = (b.sub(a), c.sub(a));
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return None; // the axis runs along the triangle
    }
    let inv = 1.0 / det;
    let s = o.sub(a);
    let u = s.dot(p) * inv;
    if !(-1e-5..=1.0 + 1e-5).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) * inv;
    if v < -1e-5 || u + v > 1.0 + 1e-5 {
        return None;
    }
    Some(e2.dot(q) * inv)
}

fn triangles(faces: &[Vec<V3>]) -> impl Iterator<Item = (V3, V3, V3)> + '_ {
    faces.iter().flat_map(|f| (1..f.len().saturating_sub(1)).map(move |k| (f[0], f[k], f[k + 1])))
}

/// Bounds of points on the two axes other than `axis`.
fn footprint(pts: impl Iterator<Item = V3>, axis: usize) -> Option<[f32; 4]> {
    let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
    let mut r: Option<[f32; 4]> = None;
    for p in pts {
        let (x, y) = (comp(p, a), comp(p, b));
        r = Some(match r {
            None => [x, y, x, y],
            Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
        });
    }
    r
}

fn overlaps(a: [f32; 4], b: [f32; 4], slack: f32) -> bool {
    a[0] <= b[2] + slack && b[0] <= a[2] + slack && a[1] <= b[3] + slack && b[1] <= a[3] + slack
}

/// The translation along `axis` to use when the user dragged `free` from the start.
///
/// `moved` are the moving vertices and `moved_faces` the faces that move with them, both at
/// their starting positions; `others` are the meshes to snap against. Returns `free` adjusted
/// so the moving geometry touches another surface when that is within `threshold`, or else
/// lands on the grid (its lowest point, highest point or centre on a grid line).
pub fn snap_move(moved: &[V3], moved_faces: &[Vec<V3>], others: &[&Mesh], axis: usize, free: f32, threshold: f32, grid: f32) -> f32 {
    if moved.is_empty() {
        return free;
    }
    let dir = axis_vec(axis);
    let shift = dir.scale(free);
    let now: Vec<V3> = moved.iter().map(|p| p.add(shift)).collect();
    let now_faces: Vec<Vec<V3>> = moved_faces.iter().map(|f| f.iter().map(|p| p.add(shift)).collect()).collect();
    let mine = footprint(now.iter().copied(), axis);

    // Contact: the smallest extra move that makes a moving vertex touch another surface, or
    // another object's vertex touch a moving face.
    let mut best: Option<f32> = None;
    let mut consider = |s: f32| {
        if s.abs() <= threshold && best.map_or(true, |b: f32| s.abs() < b.abs()) {
            best = Some(s);
        }
    };
    for other in others {
        let Some(theirs) = footprint(other.verts.iter().copied(), axis) else { continue };
        if !mine.map_or(false, |m| overlaps(m, theirs, threshold)) {
            continue;
        }
        let other_faces: Vec<Vec<V3>> = other.faces.iter().map(|f| f.iter().map(|&i| other.verts[i as usize]).collect()).collect();
        for &p in &now {
            for (a, b, c) in triangles(&other_faces) {
                if let Some(s) = ray_triangle(p, dir, a, b, c) {
                    consider(s);
                }
            }
        }
        for &q in &other.verts {
            for (a, b, c) in triangles(&now_faces) {
                // The moving face reaches q after moving s along the axis.
                if let Some(s) = ray_triangle(q, dir.scale(-1.0), a, b, c) {
                    consider(s);
                }
            }
        }
    }
    if let Some(s) = best {
        return free + s;
    }

    // Grid: line up the lowest point, the highest point or the centre with a grid line.
    if grid <= 0.0 {
        return free;
    }
    let lo = now.iter().map(|&p| comp(p, axis)).fold(f32::MAX, f32::min);
    let hi = now.iter().map(|&p| comp(p, axis)).fold(f32::MIN, f32::max);
    [lo, hi, (lo + hi) * 0.5]
        .iter()
        .map(|&v| (v / grid).round() * grid - v)
        .min_by(|a, b| a.abs().total_cmp(&b.abs()))
        .map_or(free, |d| free + d)
}

pub fn snap_angle(total: f32) -> f32 {
    (total / ANGLE_STEP).round() * ANGLE_STEP
}

pub fn snap_scale(total: f32) -> f32 {
    ((total / SCALE_STEP).round() * SCALE_STEP).max(SCALE_STEP)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_mesh(lo: (f32, f32, f32), hi: (f32, f32, f32)) -> Mesh {
        let mut m = Mesh::cube(1.0);
        for v in &mut m.verts {
            v.x = if v.x < 0.0 { lo.0 } else { hi.0 };
            v.y = if v.y < 0.0 { lo.1 } else { hi.1 };
            v.z = if v.z < 0.0 { lo.2 } else { hi.2 };
        }
        m
    }

    fn parts(m: &Mesh) -> (Vec<V3>, Vec<Vec<V3>>) {
        (m.verts.clone(), m.faces.iter().map(|f| f.iter().map(|&i| m.verts[i as usize]).collect()).collect())
    }

    #[test]
    fn drops_onto_a_table_top() {
        let table = box_mesh((-3.0, -3.0, 0.0), (3.0, 3.0, 1.0));
        let cube = box_mesh((-0.5, -0.5, 4.0), (0.5, 0.5, 5.0));
        let (pts, faces) = parts(&cube);
        // Dragged down by 2.9: the cube's bottom (z = 4) would be at 1.1, just above the table.
        let t = snap_move(&pts, &faces, &[&table], 2, -2.9, 0.3, 0.1);
        assert!((t + 3.0).abs() < 1e-4, "bottom lands exactly on the top: {t}");
        // Pushed slightly into the table: pulled back out to touch it.
        let t = snap_move(&pts, &faces, &[&table], 2, -3.15, 0.3, 0.1);
        assert!((t + 3.0).abs() < 1e-4, "{t}");
    }

    #[test]
    fn slides_against_a_wall_but_ignores_walls_it_does_not_face() {
        let wall = box_mesh((2.0, -3.0, 0.0), (2.4, 3.0, 3.0));
        let far_wall = box_mesh((2.0, 10.0, 0.0), (2.4, 12.0, 3.0)); // not in the way
        let cube = box_mesh((-0.5, -0.5, 0.0), (0.5, 0.5, 1.0));
        let (pts, faces) = parts(&cube);
        let t = snap_move(&pts, &faces, &[&wall], 0, 1.37, 0.3, 0.0);
        assert!((t - 1.5).abs() < 1e-4, "right side touches the wall: {t}");
        let t = snap_move(&pts, &faces, &[&far_wall], 0, 1.37, 0.3, 0.0);
        assert!((t - 1.37).abs() < 1e-6, "no contact, no grid: unchanged ({t})");
    }

    #[test]
    fn lands_on_slanted_faces_and_points() {
        // A plate lowered onto a cone's tip: the cone's vertex meets the plate's underside.
        let mut cone = Mesh::cone(1.0, 1.0, 16); // apex at z = 1
        for v in &mut cone.verts {
            v.x += 0.1;
        }
        let plate = box_mesh((-2.0, -2.0, 3.0), (2.0, 2.0, 3.2));
        let (pts, faces) = parts(&plate);
        let t = snap_move(&pts, &faces, &[&cone], 2, -1.9, 0.3, 0.0);
        assert!((t + 2.0).abs() < 1e-4, "plate rests on the tip: {t}");
        // A small cube slid sideways into the cone's slanted side.
        let cube = box_mesh((-3.2, -0.1, -0.6), (-3.0, 0.1, -0.4));
        let (pts, faces) = parts(&cube);
        let t = snap_move(&pts, &faces, &[&cone], 0, 2.25, 0.3, 0.0);
        let right = -3.0 + t;
        // The cone widens downwards: radius 0.8 at the cube's lower edge (z = -0.6), so that
        // corner meets the side first, at x = 0.1 - 0.8 = -0.7 (a 16-gon sits a little inside).
        assert!(right > -0.72 && right < -0.64, "touches the slanted side: {right}");
    }

    #[test]
    fn grid_when_nothing_is_near() {
        let cube = box_mesh((-0.5, -0.5, 0.0), (0.5, 0.5, 1.0));
        let (pts, faces) = parts(&cube);
        let t = snap_move(&pts, &faces, &[], 0, 0.437, 0.3, 0.25);
        // Edges at -0.063 / 0.937, centre 0.437: the closest grid line is 0 for the left side.
        assert!((t - 0.5).abs() < 1e-5, "{t}");
        assert_eq!(grid_step(7.0), 0.1);
        assert_eq!(grid_step(40.0), 1.0);
    }

    #[test]
    fn angle_and_scale_steps() {
        assert!((snap_angle(0.30) - ANGLE_STEP).abs() < 1e-6);
        assert!((snap_angle(-0.1)).abs() < 1e-6);
        assert!((snap_scale(1.26) - 1.3).abs() < 1e-6);
        assert!((snap_scale(0.01) - 0.1).abs() < 1e-6, "never collapses to zero");
    }
}
