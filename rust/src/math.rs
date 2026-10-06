//! Minimal vector / matrix helpers (right-handed, Z-up like Blender).

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct V3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

pub const fn v3(x: f32, y: f32, z: f32) -> V3 {
    V3 { x, y, z }
}

impl V3 {
    pub fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
    pub fn sub(self, o: V3) -> V3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
    pub fn scale(self, s: f32) -> V3 {
        v3(self.x * s, self.y * s, self.z * s)
    }
    pub fn dot(self, o: V3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: V3) -> V3 {
        v3(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn len(self) -> f32 {
        self.dot(self).sqrt()
    }
    pub fn norm(self) -> V3 {
        let l = self.len();
        if l < 1e-9 {
            self
        } else {
            self.scale(1.0 / l)
        }
    }
}

/// Row-major 4x4 matrix; points are column vectors (`m * p`).
#[derive(Clone, Copy, Debug)]
pub struct M4(pub [[f32; 4]; 4]);

impl M4 {
    pub fn mul(&self, o: &M4) -> M4 {
        let mut r = [[0.0f32; 4]; 4];
        for (i, row) in r.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = (0..4).map(|k| self.0[i][k] * o.0[k][j]).sum();
            }
        }
        M4(r)
    }

    pub fn apply(&self, p: V3) -> [f32; 4] {
        let m = &self.0;
        [
            m[0][0] * p.x + m[0][1] * p.y + m[0][2] * p.z + m[0][3],
            m[1][0] * p.x + m[1][1] * p.y + m[1][2] * p.z + m[1][3],
            m[2][0] * p.x + m[2][1] * p.y + m[2][2] * p.z + m[2][3],
            m[3][0] * p.x + m[3][1] * p.y + m[3][2] * p.z + m[3][3],
        ]
    }

    pub fn look_at(eye: V3, target: V3, up: V3) -> M4 {
        let f = target.sub(eye).norm();
        let s = f.cross(up).norm();
        let u = s.cross(f);
        M4([
            [s.x, s.y, s.z, -s.dot(eye)],
            [u.x, u.y, u.z, -u.dot(eye)],
            [-f.x, -f.y, -f.z, f.dot(eye)],
            [0.0, 0.0, 0.0, 1.0],
        ])
    }

    /// OpenGL-style perspective; clip z in [-w, w].
    pub fn perspective(fovy: f32, aspect: f32, near: f32, far: f32) -> M4 {
        let t = 1.0 / (fovy * 0.5).tan();
        M4([
            [t / aspect, 0.0, 0.0, 0.0],
            [0.0, t, 0.0, 0.0],
            [0.0, 0.0, (far + near) / (near - far), 2.0 * far * near / (near - far)],
            [0.0, 0.0, -1.0, 0.0],
        ])
    }
}
