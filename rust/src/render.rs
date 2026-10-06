//! Small CPU rasterizer: z-buffered flat-shaded polygons + depth-tested lines.
//! It sits behind `render_scene` so a wgpu backend can replace it later.

use crate::camera::Camera;
use crate::edit::{edge_faces, face_front_facing, Selection, SelectMode};
use crate::math::{v3, M4, V3};
use crate::mesh::Mesh;

const BG: [f32; 3] = [30.0, 30.0, 33.0]; // #1E1E21

pub struct Frame<'a> {
    pub w: usize,
    pub h: usize,
    pub rgba: &'a mut [u8],
    depth: Vec<f32>,
}

#[derive(Clone, Copy)]
struct Clip([f32; 4]);

impl<'a> Frame<'a> {
    fn new(w: usize, h: usize, rgba: &'a mut [u8]) -> Self {
        let mut f = Frame { w, h, rgba, depth: vec![f32::INFINITY; w * h] };
        f.clear();
        f
    }

    fn clear(&mut self) {
        // Soft vignette so the grid fades out toward the edges.
        let (cx, cy) = (self.w as f32 * 0.5, self.h as f32 * 0.5);
        let inv_r2 = 1.0 / (cx * cx + cy * cy);
        for y in 0..self.h {
            let dy2 = (y as f32 - cy).powi(2);
            let row = &mut self.rgba[y * self.w * 4..(y + 1) * self.w * 4];
            for (x, px) in row.chunks_exact_mut(4).enumerate() {
                let d2 = ((x as f32 - cx).powi(2) + dy2) * inv_r2;
                let k = 1.0 + 0.10 * (1.0 - d2.min(1.0));
                px[0] = (BG[0] * k) as u8;
                px[1] = (BG[1] * k) as u8;
                px[2] = (BG[2] * k) as u8;
                px[3] = 255;
            }
        }
    }

    fn to_screen(&self, c: Clip) -> (f32, f32, f32) {
        let iw = 1.0 / c.0[3];
        (
            (c.0[0] * iw * 0.5 + 0.5) * self.w as f32,
            (1.0 - (c.0[1] * iw * 0.5 + 0.5)) * self.h as f32,
            c.0[2] * iw,
        )
    }

    fn blend(&mut self, x: i32, y: i32, z: f32, col: [f32; 3], alpha: f32, write_depth: bool) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        let idx = y as usize * self.w + x as usize;
        if z > self.depth[idx] {
            return;
        }
        if write_depth {
            self.depth[idx] = z;
        }
        let i = idx * 4;
        for c in 0..3 {
            let dst = self.rgba[i + c] as f32;
            self.rgba[i + c] = (dst + (col[c] - dst) * alpha) as u8;
        }
    }

    fn triangle(&mut self, a: Clip, b: Clip, c: Clip, col: [f32; 3]) {
        let (ax, ay, az) = self.to_screen(a);
        let (bx, by, bz) = self.to_screen(b);
        let (cx, cy, cz) = self.to_screen(c);
        let area = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
        if area.abs() < 1e-6 {
            return;
        }
        let minx = ax.min(bx).min(cx).floor().max(0.0) as i32;
        let maxx = ax.max(bx).max(cx).ceil().min(self.w as f32 - 1.0) as i32;
        let miny = ay.min(by).min(cy).floor().max(0.0) as i32;
        let maxy = ay.max(by).max(cy).ceil().min(self.h as f32 - 1.0) as i32;
        let inv = 1.0 / area;
        for y in miny..=maxy {
            for x in minx..=maxx {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = ((bx - px) * (cy - py) - (by - py) * (cx - px)) * inv;
                let w1 = ((cx - px) * (ay - py) - (cy - py) * (ax - px)) * inv;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * az + w1 * bz + w2 * cz;
                self.blend(x, y, z, col, 1.0, true);
            }
        }
    }

    fn line(&mut self, a: Clip, b: Clip, col: [f32; 3], alpha: f32, width: f32, bias: f32) {
        // Clip against the near plane in clip space (w > eps).
        const EPS: f32 = 0.05;
        let (mut a, mut b) = (a, b);
        if a.0[3] < EPS && b.0[3] < EPS {
            return;
        }
        let lerp = |p: Clip, q: Clip, t: f32| {
            let mut r = [0.0; 4];
            for i in 0..4 {
                r[i] = p.0[i] + (q.0[i] - p.0[i]) * t;
            }
            Clip(r)
        };
        if a.0[3] < EPS {
            a = lerp(a, b, (EPS - a.0[3]) / (b.0[3] - a.0[3]));
        } else if b.0[3] < EPS {
            b = lerp(b, a, (EPS - b.0[3]) / (a.0[3] - b.0[3]));
        }
        let (x0, y0, z0) = self.to_screen(a);
        let (x1, y1, z1) = self.to_screen(b);
        let steps = ((x1 - x0).abs().max((y1 - y0).abs()).ceil() as i32).clamp(1, 8000);
        let r = (width * 0.5).max(0.5);
        let ri = r.ceil() as i32;
        for s in 0..=steps {
            let t = s as f32 / steps as f32;
            let (x, y, z) = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t, z0 + (z1 - z0) * t - bias);
            for oy in -ri..=ri {
                for ox in -ri..=ri {
                    let d = ((ox * ox + oy * oy) as f32).sqrt();
                    if d <= r {
                        let fall = if width <= 1.2 { 1.0 } else { (r + 0.5 - d).clamp(0.0, 1.0) };
                        self.blend(x as i32 + ox, y as i32 + oy, z, col, alpha * fall, false);
                    }
                }
            }
        }
    }

    fn dot(&mut self, c: Clip, r: f32, col: [f32; 3], alpha: f32, bias: f32) {
        if c.0[3] < 0.05 {
            return;
        }
        let (x, y, z) = self.to_screen(c);
        let ri = r.ceil() as i32 + 1;
        for oy in -ri..=ri {
            for ox in -ri..=ri {
                let d = ((ox * ox + oy * oy) as f32).sqrt();
                let a = (r + 0.5 - d).clamp(0.0, 1.0) * alpha;
                if a > 0.0 {
                    self.blend(x as i32 + ox, y as i32 + oy, z - bias, col, a, false);
                }
            }
        }
    }
}

fn clip(m: &M4, p: V3) -> Clip {
    Clip(m.apply(p))
}

/// Edit-mode overlay: selection highlight, vertex dots and move handles.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandleStyle {
    Move,
    Rotate,
    Scale,
}

pub struct EditOverlay<'a> {
    pub sel: &'a Selection,
    /// Transform handles: centre, world length and style, if they should be drawn.
    pub handles: Option<(V3, f32, HandleStyle)>,
}

pub struct Scene<'a> {
    pub mesh: &'a Mesh,
    pub edit: Option<EditOverlay<'a>>,
}

const ACCENT: [f32; 3] = [242.0, 163.0, 94.0]; // #F2A35E
const AXIS_COL: [[f32; 3]; 3] = [[229.0, 83.0, 75.0], [123.0, 200.0, 108.0], [91.0, 141.0, 239.0]];

/// Renders into `rgba` (w*h*4, tightly packed). `scale` = device pixel ratio (line widths).
pub fn render_scene(scene: &Scene, cam: &Camera, w: usize, h: usize, scale: f32, rgba: &mut [u8]) {
    assert!(rgba.len() >= w * h * 4);
    let mut f = Frame::new(w, h, rgba);
    let aspect = w as f32 / h.max(1) as f32;
    let view = cam.view();
    let vp = cam.proj(aspect).mul(&view);
    let eye = cam.eye();

    // Ground grid (XY plane) + axes.
    let n = 10i32;
    for i in -n..=n {
        if i == 0 {
            continue;
        }
        let a = i as f32;
        let alpha = if i % 5 == 0 { 0.14 } else { 0.07 };
        let g = [255.0, 255.0, 255.0];
        let (p0, p1) = (clip(&vp, v3(a, -n as f32, 0.0)), clip(&vp, v3(a, n as f32, 0.0)));
        f.line(p0, p1, g, alpha, 1.0 * scale.min(1.5), 0.0);
        let (q0, q1) = (clip(&vp, v3(-n as f32, a, 0.0)), clip(&vp, v3(n as f32, a, 0.0)));
        f.line(q0, q1, g, alpha, 1.0 * scale.min(1.5), 0.0);
    }
    let big = n as f32;
    f.line(clip(&vp, v3(-big, 0.0, 0.0)), clip(&vp, v3(big, 0.0, 0.0)), [229.0, 83.0, 75.0], 0.55, 1.5 * scale, 0.0);
    f.line(clip(&vp, v3(0.0, -big, 0.0)), clip(&vp, v3(0.0, big, 0.0)), [123.0, 200.0, 108.0], 0.55, 1.5 * scale, 0.0);

    // Faces: back-face cull, soft headlight + hemisphere shading (clay look).
    let mesh = scene.mesh;
    let light = eye.sub(cam.target).norm();
    for (fi, face) in mesh.faces.iter().enumerate() {
        if face.len() < 3 {
            continue;
        }
        let nrm = mesh.face_normal(face);
        let centre = face
            .iter()
            .fold(v3(0.0, 0.0, 0.0), |s, &i| s.add(mesh.verts[i as usize]))
            .scale(1.0 / face.len() as f32);
        if nrm.dot(eye.sub(centre)) <= 0.0 {
            continue;
        }
        let head = nrm.dot(light).max(0.0);
        let sky = nrm.z * 0.5 + 0.5;
        let k = 0.30 + 0.45 * head + 0.25 * sky;
        let base = [148.0, 148.0, 155.0]; // #94949B
        let mut col = [base[0] * k * 1.15, base[1] * k * 1.15, base[2] * k * 1.15].map(|c: f32| c.min(255.0));
        if let Some(e) = &scene.edit {
            if e.sel.mode == SelectMode::Face && e.sel.faces.contains(&fi) {
                for c in 0..3 {
                    col[c] = col[c] * 0.45 + ACCENT[c] * k.min(1.0) * 0.55;
                }
            }
        }
        let c0 = clip(&vp, mesh.verts[face[0] as usize]);
        let mut prev = clip(&vp, mesh.verts[face[1] as usize]);
        for &i in &face[2..] {
            let cur = clip(&vp, mesh.verts[i as usize]);
            tri_clipped(&mut f, c0, prev, cur, col);
            prev = cur;
        }
    }

    // Edges on top (slight depth bias so they sit on the surface).
    for face in &mesh.faces {
        let nrm = mesh.face_normal(face);
        let centre = face
            .iter()
            .fold(v3(0.0, 0.0, 0.0), |s, &i| s.add(mesh.verts[i as usize]))
            .scale(1.0 / face.len() as f32);
        if nrm.dot(eye.sub(centre)) <= 0.0 {
            continue;
        }
        for k in 0..face.len() {
            let a = clip(&vp, mesh.verts[face[k] as usize]);
            let b = clip(&vp, mesh.verts[face[(k + 1) % face.len()] as usize]);
            f.line(a, b, [42.0, 42.0, 46.0], 0.9, 1.4 * scale, 0.0005);
        }
    }

    if let Some(e) = &scene.edit {
        draw_edit_overlay(&mut f, scene, e, &vp, eye, scale);
        if let Some((centre, len, style)) = e.handles {
            let c0 = clip(&vp, centre);
            match style {
                HandleStyle::Rotate => {
                    for (axis, col) in AXIS_COL.iter().enumerate() {
                        let mut prev = clip(&vp, crate::edit::ring_point(centre, axis, len * 0.9, 0));
                        for i in 1..=crate::edit::RING_SEGMENTS {
                            let cur = clip(&vp, crate::edit::ring_point(centre, axis, len * 0.9, i));
                            f.line(prev, cur, *col, 0.9, 2.6 * scale, 4.0);
                            prev = cur;
                        }
                    }
                }
                HandleStyle::Move | HandleStyle::Scale => {
                    for (axis, col) in AXIS_COL.iter().enumerate() {
                        let tip = clip(&vp, centre.add(crate::edit::axis_vec(axis).scale(len)));
                        f.line(c0, tip, *col, 0.95, 3.0 * scale, 4.0);
                        f.dot(tip, 8.0 * scale, *col, 1.0, 4.0);
                    }
                    if style == HandleStyle::Scale {
                        f.dot(c0, 7.0 * scale, [235.0, 235.0, 240.0], 1.0, 4.0);
                    }
                }
            }
        }
    }
}

fn draw_edit_overlay(f: &mut Frame, scene: &Scene, e: &EditOverlay, vp: &M4, eye: V3, scale: f32) {
    let mesh = scene.mesh;
    let visible_face: Vec<bool> = (0..mesh.faces.len()).map(|i| face_front_facing(mesh, i, eye)).collect();

    // Selected edges: explicit ones, or the outline of selected faces.
    let mut hot: Vec<(u32, u32)> = Vec::new();
    match e.sel.mode {
        SelectMode::Edge => hot.extend(e.sel.edges.iter().copied()),
        SelectMode::Face => {
            for &fi in &e.sel.faces {
                if let Some(face) = mesh.faces.get(fi) {
                    for k in 0..face.len() {
                        hot.push(crate::edit::edge_key(face[k], face[(k + 1) % face.len()]));
                    }
                }
            }
        }
        SelectMode::Vertex => {}
    }
    let edges = edge_faces(mesh);
    for (a, b) in hot {
        let vis = edges.get(&(a, b)).map_or(false, |fs| fs.iter().any(|&fi| visible_face[fi]));
        if vis {
            f.line(clip(vp, mesh.verts[a as usize]), clip(vp, mesh.verts[b as usize]), ACCENT, 1.0, 2.8 * scale, 0.002);
        }
    }

    // Vertex dots (only on front-facing geometry).
    let mut shown = std::collections::BTreeSet::new();
    for (fi, face) in mesh.faces.iter().enumerate() {
        if visible_face[fi] {
            shown.extend(face.iter().copied());
        }
    }
    let affected = e.sel.affected_verts(mesh);
    for vi in shown {
        let sel = e.sel.mode == SelectMode::Vertex && e.sel.verts.contains(&vi);
        let (r, col) = if sel {
            (5.5 * scale, ACCENT)
        } else if affected.contains(&vi) {
            (4.0 * scale, ACCENT)
        } else {
            (3.2 * scale, [210.0, 210.0, 216.0])
        };
        f.dot(clip(vp, mesh.verts[vi as usize]), r, col, 1.0, 0.002);
    }
}

/// Triangle with simple near-plane rejection (all vertices must be in front).
fn tri_clipped(f: &mut Frame, a: Clip, b: Clip, c: Clip, col: [f32; 3]) {
    const EPS: f32 = 0.05;
    if a.0[3] < EPS || b.0[3] < EPS || c.0[3] < EPS {
        return;
    }
    f.triangle(a, b, c, col);
}
