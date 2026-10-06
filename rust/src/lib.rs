pub mod camera;
pub mod edit;
pub mod math;
pub mod mesh;
pub mod render;

use std::cell::Cell;

use camera::Camera;
use edit::{SelectMode, Selection, Snapshot, View};
use math::V3;
use mesh::Mesh;

const MAX_HISTORY: usize = 100;
/// Tap radius for vertices / edges, as a fraction of the viewport height.
const PICK_RADIUS: f32 = 0.035;
const HANDLE_RADIUS: f32 = 0.045;

pub struct Core {
    pub camera: Camera,
    pub mesh: Mesh,
    pub edit_mode: bool,
    pub show_handles: bool,
    pub sel: Selection,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    drag_axis: Option<usize>,
    aspect: Cell<f32>,
}

impl Core {
    pub fn new() -> Core {
        Core {
            camera: Camera::default(),
            mesh: Mesh::cube(1.0),
            edit_mode: false,
            show_handles: false,
            sel: Selection::new(SelectMode::Face),
            undo: Vec::new(),
            redo: Vec::new(),
            drag_axis: None,
            aspect: Cell::new(1.0),
        }
    }

    fn view(&self) -> View {
        View::new(&self.camera, self.aspect.get())
    }

    fn handle_geometry(&self) -> Option<(V3, f32)> {
        if !(self.edit_mode && self.show_handles) {
            return None;
        }
        let verts = self.sel.affected_verts(&self.mesh);
        if verts.is_empty() {
            return None;
        }
        let sum = verts.iter().fold(math::v3(0.0, 0.0, 0.0), |s, &i| s.add(self.mesh.verts[i as usize]));
        Some((sum.scale(1.0 / verts.len() as f32), self.camera.distance * 0.18))
    }

    pub fn render(&self, w: usize, h: usize, scale: f32, out: &mut [u8]) {
        self.aspect.set(w as f32 / h.max(1) as f32);
        let edit = self
            .edit_mode
            .then(|| render::EditOverlay { sel: &self.sel, handles: self.handle_geometry() });
        render::render_scene(&render::Scene { mesh: &self.mesh, edit }, &self.camera, w, h, scale, out);
    }

    pub fn set_edit_mode(&mut self, on: bool) {
        self.edit_mode = on;
        self.drag_axis = None;
    }

    pub fn set_select_mode(&mut self, mode: SelectMode) {
        if self.sel.mode != mode {
            self.sel = Selection::new(mode);
        }
    }

    /// Selects the element under (`nx`,`ny`) (fractions of width / height). Returns whether anything was hit.
    pub fn tap(&mut self, nx: f32, ny: f32, add: bool) -> bool {
        if !self.edit_mode {
            return false;
        }
        let view = self.view();
        let p = (nx * view.aspect, ny);
        let hit = match self.sel.mode {
            SelectMode::Vertex => edit::pick_vertex(&self.mesh, &view, p, PICK_RADIUS).map(Hit::Vert),
            SelectMode::Edge => edit::pick_edge(&self.mesh, &view, p, PICK_RADIUS).map(Hit::Edge),
            SelectMode::Face => edit::pick_face(&self.mesh, &view, p).map(Hit::Face),
        };
        if !add {
            let keep = match &hit {
                Some(Hit::Vert(v)) => self.sel.verts.len() == 1 && self.sel.verts.contains(v),
                Some(Hit::Edge(e)) => self.sel.edges.len() == 1 && self.sel.edges.contains(e),
                Some(Hit::Face(f)) => self.sel.faces.len() == 1 && self.sel.faces.contains(f),
                None => false,
            };
            if !keep {
                self.sel.clear();
            }
        }
        match hit {
            Some(Hit::Vert(v)) => toggle(&mut self.sel.verts, v, add),
            Some(Hit::Edge(e)) => toggle(&mut self.sel.edges, e, add),
            Some(Hit::Face(f)) => toggle(&mut self.sel.faces, f, add),
            None => return false,
        }
        true
    }

    fn push_undo(&mut self) {
        self.undo.push(Snapshot { mesh: self.mesh.clone(), sel: self.sel.clone() });
        if self.undo.len() > MAX_HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo.pop() else { return false };
        self.redo.push(Snapshot { mesh: self.mesh.clone(), sel: self.sel.clone() });
        self.mesh = prev.mesh;
        self.sel = prev.sel;
        self.drag_axis = None;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(Snapshot { mesh: self.mesh.clone(), sel: self.sel.clone() });
        self.mesh = next.mesh;
        self.sel = next.sel;
        self.drag_axis = None;
        true
    }

    /// Bit 0: can undo, bit 1: can redo, bit 2: something is selected.
    pub fn status(&self) -> i32 {
        (!self.undo.is_empty()) as i32 | ((!self.redo.is_empty()) as i32) << 1 | ((!self.sel.is_empty()) as i32) << 2
    }

    /// Starts dragging a move handle if one is under the point. Records an undo step.
    pub fn drag_begin(&mut self, nx: f32, ny: f32) -> bool {
        let Some((centre, len)) = self.handle_geometry() else { return false };
        let view = self.view();
        let Some(axis) = edit::pick_handle(&view, centre, len, (nx * view.aspect, ny), HANDLE_RADIUS) else {
            return false;
        };
        self.push_undo();
        self.drag_axis = Some(axis);
        true
    }

    /// Moves the selection along the grabbed axis. Deltas are fractions of width / height.
    pub fn drag_update(&mut self, dnx: f32, dny: f32) {
        let (Some(axis), Some((centre, _))) = (self.drag_axis, self.handle_geometry()) else { return };
        let view = self.view();
        let mut unit = [0.0f32; 3];
        unit[axis] = 1.0;
        let dir = math::v3(unit[0], unit[1], unit[2]);
        let (Some(a), Some(b)) = (view.project(centre), view.project(centre.add(dir))) else { return };
        let s = (b.0 - a.0, b.1 - a.1);
        let s2 = s.0 * s.0 + s.1 * s.1;
        if s2 < 1e-5 {
            return; // axis points at the camera: no meaningful screen direction
        }
        let d = (dnx * view.aspect, dny);
        let t = (d.0 * s.0 + d.1 * s.1) / s2;
        for vi in self.sel.affected_verts(&self.mesh) {
            let v = &mut self.mesh.verts[vi as usize];
            *v = v.add(dir.scale(t));
        }
    }

    pub fn drag_end(&mut self) {
        self.drag_axis = None;
    }
}

enum Hit {
    Vert(u32),
    Edge((u32, u32)),
    Face(usize),
}

fn toggle<T: Ord>(set: &mut std::collections::BTreeSet<T>, v: T, add: bool) {
    if add && set.contains(&v) {
        set.remove(&v);
    } else {
        set.insert(v);
    }
}

#[cfg(test)]
impl Core {
    fn set_move_tool_for_test(&mut self) {
        self.show_handles = true;
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

/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_set_edit_mode(core: *mut Core, on: i32) {
    if let Some(c) = core.as_mut() {
        c.set_edit_mode(on != 0);
    }
}

/// 0 vertex, 1 edge, 2 face.
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_set_select_mode(core: *mut Core, mode: i32) {
    if let Some(c) = core.as_mut() {
        c.set_select_mode(SelectMode::from_i32(mode));
    }
}

/// Shows the move handles on the selection (the Move tool is active).
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_set_move_tool(core: *mut Core, on: i32) {
    if let Some(c) = core.as_mut() {
        c.show_handles = on != 0;
    }
}

/// Tap-select at (`nx`,`ny`) given as fractions of the viewport. Returns 1 if an element was hit.
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_tap(core: *mut Core, nx: f32, ny: f32, add: i32) -> i32 {
    core.as_mut().map_or(0, |c| c.tap(nx, ny, add != 0) as i32)
}

/// Returns 1 if a move handle was grabbed at the point.
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_drag_begin(core: *mut Core, nx: f32, ny: f32) -> i32 {
    core.as_mut().map_or(0, |c| c.drag_begin(nx, ny) as i32)
}

/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_drag_update(core: *mut Core, dnx: f32, dny: f32) {
    if let Some(c) = core.as_mut() {
        c.drag_update(dnx, dny);
    }
}

/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_drag_end(core: *mut Core) {
    if let Some(c) = core.as_mut() {
        c.drag_end();
    }
}

/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_undo(core: *mut Core) -> i32 {
    core.as_mut().map_or(0, |c| c.undo() as i32)
}

/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_redo(core: *mut Core) -> i32 {
    core.as_mut().map_or(0, |c| c.redo() as i32)
}

/// Bit 0: can undo, bit 1: can redo, bit 2: something is selected.
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_status(core: *const Core) -> i32 {
    core.as_ref().map_or(0, |c| c.status())
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

    fn front_core() -> Core {
        let mut c = Core::new();
        c.camera.snap_axis(3); // looking along +Y from the front
        c.set_edit_mode(true);
        let mut buf = vec![0u8; 400 * 300 * 4];
        c.render(400, 300, 1.0, &mut buf); // records the aspect ratio
        c
    }

    #[test]
    fn tap_selects_front_face_then_toggles() {
        let mut c = front_core();
        assert!(c.tap(0.5, 0.5, false));
        assert_eq!(c.sel.faces.iter().copied().collect::<Vec<_>>(), vec![2]); // -Y face
        assert!(c.tap(0.5, 0.5, true)); // add-tap on a selected face deselects it
        assert!(c.sel.faces.is_empty());
        assert!(!c.tap(0.02, 0.02, false)); // empty space
        assert!(c.sel.is_empty());
    }

    #[test]
    fn tap_selects_vertex_and_edge() {
        let mut c = front_core();
        let view = c.view();
        let (vx, vy, _) = view.project(c.mesh.verts[0]).unwrap(); // (-1,-1,-1)
        c.set_select_mode(SelectMode::Vertex);
        assert!(c.tap(vx / view.aspect, vy, false));
        assert!(c.sel.verts.contains(&0));
        // Midpoint of the front-bottom edge (0,1).
        let mid = c.mesh.verts[0].add(c.mesh.verts[1]).scale(0.5);
        let (ex, ey, _) = view.project(mid).unwrap();
        c.set_select_mode(SelectMode::Edge);
        assert!(c.tap(ex / view.aspect, ey, false));
        assert!(c.sel.edges.contains(&(0, 1)));
    }

    #[test]
    fn move_along_x_and_undo_redo() {
        let mut c = front_core();
        c.set_move_tool_for_test();
        c.tap(0.5, 0.5, false);
        let before = c.mesh.verts.clone();
        let view = c.view();
        let (centre, len) = c.handle_geometry().unwrap();
        let tip = view.project(centre.add(math::v3(len, 0.0, 0.0))).unwrap();
        // Grab the X handle near its tip, drag 0.1 of the width to the right.
        assert!(c.drag_begin(tip.0 / view.aspect, tip.1));
        c.drag_update(0.1, 0.0);
        c.drag_end();
        let moved: Vec<usize> = (0..8).filter(|&i| (c.mesh.verts[i].x - before[i].x).abs() > 1e-4).collect();
        assert_eq!(moved.len(), 4, "only the selected face moves");
        assert!(moved.iter().all(|&i| c.mesh.verts[i].x > before[i].x));
        assert!(c.mesh.verts.iter().zip(&before).all(|(a, b)| (a.y - b.y).abs() < 1e-5 && (a.z - b.z).abs() < 1e-5));
        assert_eq!(c.status() & 3, 1);
        assert!(c.undo());
        assert_eq!(c.mesh.verts, before);
        assert_eq!(c.status() & 3, 2);
        assert!(c.redo());
        assert!(c.mesh.verts != before);
        assert!(!c.redo());
    }

    #[test]
    fn drag_without_handle_hit_does_nothing() {
        let mut c = front_core();
        c.set_move_tool_for_test();
        c.tap(0.5, 0.5, false);
        assert!(!c.drag_begin(0.02, 0.02));
        assert_eq!(c.status() & 1, 0);
    }

    #[test]
    fn selection_renders_accent() {
        let mut c = front_core();
        let mut a = vec![0u8; 400 * 300 * 4];
        let mut b = vec![0u8; 400 * 300 * 4];
        c.render(400, 300, 1.0, &mut a);
        c.tap(0.5, 0.5, false);
        c.render(400, 300, 1.0, &mut b);
        let i = (150 * 400 + 200) * 4;
        assert!(b[i] > a[i] + 20 && b[i + 2] < a[i + 2], "selected face turns orange: {:?} -> {:?}", &a[i..i + 3], &b[i..i + 3]);
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
