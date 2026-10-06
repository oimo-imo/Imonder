//! Imonder core. Exposes a small C ABI consumed from Dart via dart:ffi.

pub mod camera;
pub mod math;
pub mod mesh;
pub mod render;

use camera::Camera;
use mesh::Mesh;

pub struct Core {
    pub camera: Camera,
    pub mesh: Mesh,
}

impl Core {
    pub fn new() -> Core {
        Core { camera: Camera::default(), mesh: Mesh::cube(1.0) }
    }

    pub fn render(&self, w: usize, h: usize, scale: f32, out: &mut [u8]) {
        render::render_scene(&render::Scene { mesh: &self.mesh }, &self.camera, w, h, scale, out);
    }
}

impl Default for Core {
    fn default() -> Self {
        Core::new()
    }
}

// ---------------------------------------------------------------- C ABI --

#[no_mangle]
pub extern "C" fn imonder_create() -> *mut Core {
    Box::into_raw(Box::new(Core::new()))
}

/// # Safety
/// `core` must come from `imonder_create` and not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn imonder_destroy(core: *mut Core) {
    if !core.is_null() {
        drop(Box::from_raw(core));
    }
}

/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_orbit(core: *mut Core, dx: f32, dy: f32) {
    if let Some(c) = core.as_mut() {
        c.camera.orbit(dx, dy);
    }
}

/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_pan(core: *mut Core, dx: f32, dy: f32) {
    if let Some(c) = core.as_mut() {
        c.camera.pan(dx, dy);
    }
}

/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_zoom(core: *mut Core, factor: f32) {
    if let Some(c) = core.as_mut() {
        c.camera.zoom(factor);
    }
}

/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_snap_view(core: *mut Core, preset: i32) {
    if let Some(c) = core.as_mut() {
        c.camera.snap(preset);
    }
}

/// Snaps the view to look along a world axis (0 +X, 1 -X, 2 +Y, 3 -Y, 4 +Z, 5 -Z).
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_snap_axis(core: *mut Core, axis: i32) {
    if let Some(c) = core.as_mut() {
        c.camera.snap_axis(axis);
    }
}

/// Writes the camera yaw and pitch (radians) into `out[0..2]`.
///
/// # Safety
/// `core` must be live and `out` must point to two writable `f32`s.
#[no_mangle]
pub unsafe extern "C" fn imonder_get_angles(core: *const Core, out: *mut f32) {
    if let (Some(c), false) = (core.as_ref(), out.is_null()) {
        *out = c.camera.yaw;
        *out.add(1) = c.camera.pitch;
    }
}

/// Renders RGBA8 into `out` (`w*h*4` bytes). Returns 1 on success.
///
/// # Safety
/// `core` must be live and `out` must point to at least `w*h*4` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn imonder_render(core: *mut Core, w: u32, h: u32, scale: f32, out: *mut u8) -> i32 {
    let (Some(c), false) = (core.as_ref(), out.is_null()) else { return 0 };
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return 0;
    }
    let buf = std::slice::from_raw_parts_mut(out, w as usize * h as usize * 4);
    c.render(w as usize, h as usize, scale, buf);
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(buf: &[u8], w: usize, x: usize, y: usize) -> [u8; 3] {
        let i = (y * w + x) * 4;
        [buf[i], buf[i + 1], buf[i + 2]]
    }

    #[test]
    fn renders_cube_over_background() {
        let core = Core::new();
        let (w, h) = (320, 240);
        let mut buf = vec![0u8; w * h * 4];
        core.render(w, h, 1.0, &mut buf);
        let corner = px(&buf, w, 2, 2);
        let centre = px(&buf, w, w / 2, h / 2);
        assert!(corner[0] < 40 && corner[2] < 45, "background corner {corner:?}");
        assert_ne!(corner, centre, "cube should cover the centre");
        assert!(buf.chunks(4).all(|p| p[3] == 255));
    }

    #[test]
    fn orbit_changes_image() {
        let mut core = Core::new();
        let (w, h) = (200, 150);
        let mut a = vec![0u8; w * h * 4];
        let mut b = vec![0u8; w * h * 4];
        core.render(w, h, 1.0, &mut a);
        core.camera.orbit(0.5, 0.2);
        core.render(w, h, 1.0, &mut b);
        assert_ne!(a, b);
    }

    #[test]
    fn camera_limits() {
        let mut c = Camera::default();
        c.orbit(0.0, 100.0);
        assert!(c.pitch <= 1.55 + 1e-6);
        c.zoom(1e6);
        assert!(c.distance >= 0.5);
        c.zoom(1e-6);
        assert!(c.distance <= 200.0);
    }

    #[test]
    fn snap_axis_looks_along_axis() {
        let mut c = Camera::default();
        for (axis, want) in [
            (0, [1.0, 0.0, 0.0]),
            (1, [-1.0, 0.0, 0.0]),
            (2, [0.0, 1.0, 0.0]),
            (3, [0.0, -1.0, 0.0]),
            (4, [0.0, 0.0, 1.0]),
            (5, [0.0, 0.0, -1.0]),
        ] {
            c.snap_axis(axis);
            let d = c.eye().sub(c.target).norm();
            assert!(
                (d.x - want[0]).abs() < 0.05 && (d.y - want[1]).abs() < 0.05 && (d.z - want[2]).abs() < 0.05,
                "axis {axis}: {d:?}"
            );
        }
    }

    #[test]
    fn ffi_angles() {
        unsafe {
            let c = imonder_create();
            imonder_snap_axis(c, 0);
            let mut a = [9.0f32; 2];
            imonder_get_angles(c, a.as_mut_ptr());
            assert_eq!(a, [0.0, 0.0]);
            imonder_destroy(c);
        }
    }

    #[test]
    fn ffi_roundtrip() {
        unsafe {
            let c = imonder_create();
            imonder_orbit(c, 0.1, 0.1);
            imonder_pan(c, 0.05, 0.0);
            imonder_zoom(c, 1.2);
            imonder_snap_view(c, 2);
            let mut buf = vec![0u8; 64 * 64 * 4];
            assert_eq!(imonder_render(c, 64, 64, 1.0, buf.as_mut_ptr()), 1);
            assert_eq!(imonder_render(c, 0, 64, 1.0, buf.as_mut_ptr()), 0);
            imonder_destroy(c);
        }
    }
}
