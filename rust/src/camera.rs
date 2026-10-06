use crate::math::{v3, M4, V3};

/// Orbit camera, Z-up. `yaw` rotates around Z, `pitch` lifts above the ground.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: V3,
    pub fovy: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            yaw: -0.7,
            pitch: 0.5,
            distance: 7.0,
            target: v3(0.0, 0.0, 0.0),
            fovy: 0.8,
        }
    }
}

const MAX_PITCH: f32 = 1.55;

impl Camera {
    pub fn eye(&self) -> V3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target
            .add(v3(cy * cp, sy * cp, sp).scale(self.distance))
    }

    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx;
        self.pitch = (self.pitch + dy).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// `dx`/`dy` are fractions of the viewport height.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        let eye = self.eye();
        let f = self.target.sub(eye).norm();
        let right = f.cross(v3(0.0, 0.0, 1.0)).norm();
        let up = right.cross(f);
        let h = 2.0 * self.distance * (self.fovy * 0.5).tan();
        self.target = self
            .target
            .add(right.scale(-dx * h))
            .add(up.scale(dy * h));
    }

    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance / factor.max(0.01)).clamp(0.5, 200.0);
    }

    pub fn view(&self) -> M4 {
        M4::look_at(self.eye(), self.target, v3(0.0, 0.0, 1.0))
    }

    pub fn proj(&self, aspect: f32) -> M4 {
        // Keep the horizontal field of view from shrinking on portrait screens.
        let fovy = if aspect < 1.0 {
            2.0 * ((self.fovy * 0.5).tan() / aspect).atan()
        } else {
            self.fovy
        };
        M4::perspective(fovy, aspect, 0.05, 500.0)
    }

    /// Look at the target from along a world axis.
    /// 0 +X (right), 1 -X (left), 2 +Y (back), 3 -Y (front), 4 +Z (top), 5 -Z (bottom).
    pub fn snap_axis(&mut self, axis: i32) {
        use std::f32::consts::{FRAC_PI_2, PI};
        let (yaw, pitch) = match axis {
            0 => (0.0, 0.0),
            1 => (PI, 0.0),
            2 => (FRAC_PI_2, 0.0),
            3 => (-FRAC_PI_2, 0.0),
            4 => (-FRAC_PI_2, MAX_PITCH),
            5 => (-FRAC_PI_2, -MAX_PITCH),
            _ => return,
        };
        self.yaw = yaw;
        self.pitch = pitch;
    }

    /// Snap to a preset view (Blender numpad style): 0 front, 1 right, 2 top, 3 perspective.
    pub fn snap(&mut self, preset: i32) {
        match preset {
            0 => {
                self.yaw = -std::f32::consts::FRAC_PI_2;
                self.pitch = 0.0;
            }
            1 => {
                self.yaw = 0.0;
                self.pitch = 0.0;
            }
            2 => {
                self.yaw = -std::f32::consts::FRAC_PI_2;
                self.pitch = MAX_PITCH;
            }
            _ => *self = Camera::default(),
        }
    }
}
