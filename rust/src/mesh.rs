use crate::math::{v3, V3};

/// Indexed polygon mesh. Faces are CCW when seen from outside.
/// (Stage 2 of the roadmap replaces this with a half-edge structure.)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub verts: Vec<V3>,
    pub faces: Vec<Vec<u32>>,
}

impl Mesh {
    pub fn cube(half: f32) -> Mesh {
        let h = half;
        let verts = vec![
            v3(-h, -h, -h),
            v3(h, -h, -h),
            v3(h, h, -h),
            v3(-h, h, -h),
            v3(-h, -h, h),
            v3(h, -h, h),
            v3(h, h, h),
            v3(-h, h, h),
        ];
        let faces = vec![
            vec![0, 3, 2, 1], // -Z
            vec![4, 5, 6, 7], // +Z
            vec![0, 1, 5, 4], // -Y
            vec![2, 3, 7, 6], // +Y
            vec![1, 2, 6, 5], // +X
            vec![3, 0, 4, 7], // -X
        ];
        Mesh { verts, faces }
    }

    /// Flat square in the XY plane, facing +Z.
    pub fn plane(half: f32) -> Mesh {
        let h = half;
        Mesh {
            verts: vec![v3(-h, -h, 0.0), v3(h, -h, 0.0), v3(h, h, 0.0), v3(-h, h, 0.0)],
            faces: vec![vec![0, 1, 2, 3]],
        }
    }

    /// Z-axis cylinder of radius `r`, height `2*half_h`, with n-gon caps.
    pub fn cylinder(r: f32, half_h: f32, segments: usize) -> Mesh {
        let n = segments.max(3) as u32;
        let mut verts = Vec::new();
        for z in [-half_h, half_h] {
            for i in 0..n {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                verts.push(v3(a.cos() * r, a.sin() * r, z));
            }
        }
        let mut faces = Vec::new();
        for i in 0..n {
            let j = (i + 1) % n;
            faces.push(vec![i, j, n + j, n + i]);
        }
        faces.push((0..n).rev().collect()); // bottom, faces -Z
        faces.push((n..2 * n).collect()); // top, faces +Z
        Mesh { verts, faces }
    }

    /// Z-axis cone: base circle at -half_h, apex at +half_h.
    pub fn cone(r: f32, half_h: f32, segments: usize) -> Mesh {
        let n = segments.max(3) as u32;
        let mut verts = Vec::new();
        for i in 0..n {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            verts.push(v3(a.cos() * r, a.sin() * r, -half_h));
        }
        verts.push(v3(0.0, 0.0, half_h));
        let apex = n;
        let mut faces = Vec::new();
        for i in 0..n {
            faces.push(vec![i, (i + 1) % n, apex]);
        }
        faces.push((0..n).rev().collect());
        Mesh { verts, faces }
    }

    /// UV sphere: quads between rings, triangles at the poles.
    pub fn sphere(r: f32, segments: usize, rings: usize) -> Mesh {
        let (n, m) = (segments.max(3) as u32, rings.max(2) as u32);
        let mut verts = vec![v3(0.0, 0.0, -r)];
        for k in 1..m {
            let phi = std::f32::consts::PI * k as f32 / m as f32 - std::f32::consts::FRAC_PI_2;
            for i in 0..n {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                verts.push(v3(a.cos() * phi.cos() * r, a.sin() * phi.cos() * r, phi.sin() * r));
            }
        }
        verts.push(v3(0.0, 0.0, r));
        let top = verts.len() as u32 - 1;
        let ring = |k: u32, i: u32| 1 + (k - 1) * n + (i % n);
        let mut faces = Vec::new();
        for i in 0..n {
            faces.push(vec![0, ring(1, i + 1), ring(1, i)]);
        }
        for k in 1..m - 1 {
            for i in 0..n {
                faces.push(vec![ring(k, i), ring(k, i + 1), ring(k + 1, i + 1), ring(k + 1, i)]);
            }
        }
        for i in 0..n {
            faces.push(vec![ring(m - 1, i), ring(m - 1, i + 1), top]);
        }
        Mesh { verts, faces }
    }

    /// Torus around the Z axis: `major` = ring radius, `minor` = tube radius.
    pub fn torus(major: f32, minor: f32, segments: usize, sides: usize) -> Mesh {
        let (n, m) = (segments.max(3) as u32, sides.max(3) as u32);
        let mut verts = Vec::new();
        for i in 0..n {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            for j in 0..m {
                let b = j as f32 / m as f32 * std::f32::consts::TAU;
                let rr = major + minor * b.cos();
                verts.push(v3(a.cos() * rr, a.sin() * rr, minor * b.sin()));
            }
        }
        let id = |i: u32, j: u32| (i % n) * m + (j % m);
        let mut faces = Vec::new();
        for i in 0..n {
            for j in 0..m {
                faces.push(vec![id(i, j), id(i + 1, j), id(i + 1, j + 1), id(i, j + 1)]);
            }
        }
        Mesh { verts, faces }
    }

    /// Axis-aligned bounds (min, max); `None` for an empty mesh.
    pub fn bounds(&self) -> Option<(V3, V3)> {
        let first = *self.verts.first()?;
        Some(self.verts.iter().fold((first, first), |(lo, hi), v| {
            (v3(lo.x.min(v.x), lo.y.min(v.y), lo.z.min(v.z)), v3(hi.x.max(v.x), hi.y.max(v.y), hi.z.max(v.z)))
        }))
    }

    pub fn face_normal(&self, f: &[u32]) -> V3 {
        // Newell's method, robust for any planar-ish n-gon.
        let mut n = v3(0.0, 0.0, 0.0);
        for i in 0..f.len() {
            let a = self.verts[f[i] as usize];
            let b = self.verts[f[(i + 1) % f.len()] as usize];
            n.x += (a.y - b.y) * (a.z + b.z);
            n.y += (a.z - b.z) * (a.x + b.x);
            n.z += (a.x - b.x) * (a.y + b.y);
        }
        n.norm()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

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

    #[test]
    fn primitives_are_watertight_and_outward() {
        let pi = std::f32::consts::PI;
        let cases: [(&str, Mesh, f32); 4] = [
            ("cylinder", Mesh::cylinder(1.0, 1.0, 64), pi * 2.0),
            ("cone", Mesh::cone(1.0, 1.0, 64), pi * 2.0 / 3.0),
            ("sphere", Mesh::sphere(1.0, 64, 32), 4.0 / 3.0 * pi),
            ("torus", Mesh::torus(1.0, 0.25, 64, 32), 2.0 * pi * pi * 1.0 * 0.0625),
        ];
        for (name, m, want) in cases {
            assert!(closed(&m), "{name} is closed");
            let v = volume(&m);
            assert!((v - want).abs() / want < 0.03, "{name} volume {v} vs {want}");
        }
    }

    #[test]
    fn plane_and_bounds() {
        let p = Mesh::plane(1.0);
        assert_eq!(p.faces.len(), 1);
        assert!(p.face_normal(&p.faces[0]).z > 0.99);
        let (lo, hi) = Mesh::cube(2.0).bounds().unwrap();
        assert_eq!((lo.x, hi.z), (-2.0, 2.0));
        assert!(Mesh::default().bounds().is_none());
    }
}
