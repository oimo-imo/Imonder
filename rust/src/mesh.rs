use crate::math::{v3, V3};

/// Indexed polygon mesh. Faces are CCW when seen from outside.
/// (Stage 2 of the roadmap replaces this with a half-edge structure.)
#[derive(Clone, Debug, Default)]
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
