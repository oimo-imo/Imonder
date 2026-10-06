//! Edit-mode state: selection, picking, axis-constrained move and undo history.

use std::collections::{BTreeMap, BTreeSet};

use crate::camera::Camera;
use crate::math::{v3, M4, V3};
use crate::mesh::Mesh;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectMode {
    Vertex,
    Edge,
    Face,
}

impl SelectMode {
    pub fn from_i32(i: i32) -> SelectMode {
        match i {
            0 => SelectMode::Vertex,
            1 => SelectMode::Edge,
            _ => SelectMode::Face,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Selection {
    pub mode: SelectMode,
    pub verts: BTreeSet<u32>,
    pub edges: BTreeSet<(u32, u32)>,
    pub faces: BTreeSet<usize>,
}

impl Selection {
    pub fn new(mode: SelectMode) -> Selection {
        Selection { mode, verts: BTreeSet::new(), edges: BTreeSet::new(), faces: BTreeSet::new() }
    }

    pub fn clear(&mut self) {
        self.verts.clear();
        self.edges.clear();
        self.faces.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.verts.is_empty() && self.edges.is_empty() && self.faces.is_empty()
    }

    /// Vertices that a transform acts on, according to the current select mode.
    pub fn affected_verts(&self, mesh: &Mesh) -> BTreeSet<u32> {
        match self.mode {
            SelectMode::Vertex => self.verts.clone(),
            SelectMode::Edge => self.edges.iter().flat_map(|&(a, b)| [a, b]).collect(),
            SelectMode::Face => self
                .faces
                .iter()
                .filter_map(|&f| mesh.faces.get(f))
                .flat_map(|f| f.iter().copied())
                .collect(),
        }
    }
}

pub fn edge_key(a: u32, b: u32) -> (u32, u32) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Unique edges with the faces that use them.
pub fn edge_faces(mesh: &Mesh) -> BTreeMap<(u32, u32), Vec<usize>> {
    let mut m: BTreeMap<(u32, u32), Vec<usize>> = BTreeMap::new();
    for (fi, f) in mesh.faces.iter().enumerate() {
        for k in 0..f.len() {
            m.entry(edge_key(f[k], f[(k + 1) % f.len()])).or_default().push(fi);
        }
    }
    m
}

pub fn face_front_facing(mesh: &Mesh, fi: usize, eye: V3) -> bool {
    let f = &mesh.faces[fi];
    let n = mesh.face_normal(f);
    let c = f
        .iter()
        .fold(v3(0.0, 0.0, 0.0), |s, &i| s.add(mesh.verts[i as usize]))
        .scale(1.0 / f.len() as f32);
    n.dot(eye.sub(c)) > 0.0
}

/// Projection into "height units": x in [0, aspect], y in [0, 1] (top = 0), depth in NDC.
pub struct View {
    pub vp: M4,
    pub eye: V3,
    pub aspect: f32,
}

impl View {
    pub fn new(cam: &Camera, aspect: f32) -> View {
        View { vp: cam.proj(aspect).mul(&cam.view()), eye: cam.eye(), aspect }
    }

    pub fn project(&self, p: V3) -> Option<(f32, f32, f32)> {
        let c = self.vp.apply(p);
        if c[3] < 0.05 {
            return None;
        }
        let (nx, ny, nz) = (c[0] / c[3], c[1] / c[3], c[2] / c[3]);
        Some(((nx * 0.5 + 0.5) * self.aspect, 1.0 - (ny * 0.5 + 0.5), nz))
    }
}

fn dist_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let len2 = abx * abx + aby * aby;
    let t = if len2 < 1e-12 { 0.0 } else { (((p.0 - a.0) * abx + (p.1 - a.1) * aby) / len2).clamp(0.0, 1.0) };
    let (qx, qy) = (a.0 + abx * t, a.1 + aby * t);
    ((p.0 - qx).powi(2) + (p.1 - qy).powi(2)).sqrt()
}

pub fn pick_vertex(mesh: &Mesh, view: &View, p: (f32, f32), radius: f32) -> Option<u32> {
    let mut visible = BTreeSet::new();
    for (fi, f) in mesh.faces.iter().enumerate() {
        if face_front_facing(mesh, fi, view.eye) {
            visible.extend(f.iter().copied());
        }
    }
    visible
        .into_iter()
        .filter_map(|vi| {
            let (x, y, _) = view.project(mesh.verts[vi as usize])?;
            let d = ((x - p.0).powi(2) + (y - p.1).powi(2)).sqrt();
            (d <= radius).then_some((vi, d))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(vi, _)| vi)
}

pub fn pick_edge(mesh: &Mesh, view: &View, p: (f32, f32), radius: f32) -> Option<(u32, u32)> {
    edge_faces(mesh)
        .into_iter()
        .filter(|(_, fs)| fs.iter().any(|&f| face_front_facing(mesh, f, view.eye)))
        .filter_map(|((a, b), _)| {
            let pa = view.project(mesh.verts[a as usize])?;
            let pb = view.project(mesh.verts[b as usize])?;
            let d = dist_to_segment(p, (pa.0, pa.1), (pb.0, pb.1));
            (d <= radius).then_some(((a, b), d))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(e, _)| e)
}

pub fn pick_face(mesh: &Mesh, view: &View, p: (f32, f32)) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (fi, f) in mesh.faces.iter().enumerate() {
        if !face_front_facing(mesh, fi, view.eye) {
            continue;
        }
        let pts: Option<Vec<_>> = f.iter().map(|&i| view.project(mesh.verts[i as usize])).collect();
        let Some(pts) = pts else { continue };
        for k in 1..pts.len() - 1 {
            if let Some(z) = tri_depth(p, pts[0], pts[k], pts[k + 1]) {
                if best.map_or(true, |(_, bz)| z < bz) {
                    best = Some((fi, z));
                }
            }
        }
    }
    best.map(|(fi, _)| fi)
}

/// Depth at `p` if it lies inside the triangle (screen space).
fn tri_depth(p: (f32, f32), a: (f32, f32, f32), b: (f32, f32, f32), c: (f32, f32, f32)) -> Option<f32> {
    let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
    if area.abs() < 1e-12 {
        return None;
    }
    let w0 = ((b.0 - p.0) * (c.1 - p.1) - (b.1 - p.1) * (c.0 - p.0)) / area;
    let w1 = ((c.0 - p.0) * (a.1 - p.1) - (c.1 - p.1) * (a.0 - p.0)) / area;
    let w2 = 1.0 - w0 - w1;
    (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0).then(|| w0 * a.2 + w1 * b.2 + w2 * c.2)
}

#[derive(Clone)]
pub struct Snapshot {
    pub mesh: Mesh,
    pub sel: Selection,
}

/// Which of the three move handles (X/Y/Z) lies under `p`, if any.
pub fn pick_handle(view: &View, centre: V3, len: f32, p: (f32, f32), radius: f32) -> Option<usize> {
    let c = view.project(centre)?;
    (0..3)
        .filter_map(|axis| {
            let mut dir = [0.0; 3];
            dir[axis] = len;
            let tip = view.project(centre.add(v3(dir[0], dir[1], dir[2])))?;
            let d = dist_to_segment(p, (c.0, c.1), (tip.0, tip.1));
            (d <= radius).then_some((axis, d))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(a, _)| a)
}

/// Unit vector for axis 0/1/2.
pub fn axis_vec(axis: usize) -> V3 {
    match axis {
        0 => v3(1.0, 0.0, 0.0),
        1 => v3(0.0, 1.0, 0.0),
        _ => v3(0.0, 0.0, 1.0),
    }
}

/// Two unit vectors spanning the plane perpendicular to `axis`.
pub fn ring_basis(axis: usize) -> (V3, V3) {
    let k = axis_vec(axis);
    let u = if axis == 2 { v3(1.0, 0.0, 0.0) } else { v3(0.0, 0.0, 1.0) };
    let u = u.sub(k.scale(u.dot(k))).norm();
    (u, k.cross(u))
}

pub const RING_SEGMENTS: usize = 48;

pub fn ring_point(centre: V3, axis: usize, radius: f32, i: usize) -> V3 {
    let (u, w) = ring_basis(axis);
    let a = i as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
    centre.add(u.scale(a.cos() * radius)).add(w.scale(a.sin() * radius))
}

/// Which rotation ring lies under `p`.
pub fn pick_ring(view: &View, centre: V3, radius: f32, p: (f32, f32), tol: f32) -> Option<usize> {
    (0..3)
        .filter_map(|axis| {
            let mut best = f32::MAX;
            let mut prev = view.project(ring_point(centre, axis, radius, 0))?;
            for i in 1..=RING_SEGMENTS {
                let cur = view.project(ring_point(centre, axis, radius, i))?;
                best = best.min(dist_to_segment(p, (prev.0, prev.1), (cur.0, cur.1)));
                prev = cur;
            }
            (best <= tol).then_some((axis, best))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(a, _)| a)
}

/// Rodrigues rotation of `p` around the line through `c` along `axis`.
pub fn rotate_about(p: V3, c: V3, axis: V3, angle: f32) -> V3 {
    let v = p.sub(c);
    let (s, co) = angle.sin_cos();
    c.add(v.scale(co)).add(axis.cross(v).scale(s)).add(axis.scale(axis.dot(v) * (1.0 - co)))
}
