pub mod camera;
pub mod edit;
pub mod math;
pub mod mesh;
pub mod ops;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Move,
    Rotate,
    Scale,
    LoopCut,
    None,
}

impl Tool {
    pub fn from_i32(i: i32) -> Tool {
        match i {
            0 => Tool::Move,
            1 => Tool::Rotate,
            2 => Tool::Scale,
            3 => Tool::LoopCut,
            _ => Tool::None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Drag {
    Move(usize),
    Scale(usize), // 0..3 axis, 3 = uniform
    Rotate { axis: usize, last: (f32, f32) },
}

/// The last operation, kept so its value can be adjusted by redoing it from `base`.
struct LastOp {
    kind: ops::OpKind,
    base: Snapshot,
    param: f32,
}

pub struct Core {
    pub camera: Camera,
    pub mesh: Mesh,
    pub edit_mode: bool,
    pub tool: Tool,
    pub sel: Selection,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    drag: Option<Drag>,
    last_op: Option<LastOp>,
    aspect: Cell<f32>,
}

impl Core {
    pub fn new() -> Core {
        Core {
            camera: Camera::default(),
            mesh: Mesh::cube(1.0),
            edit_mode: false,
            tool: Tool::Move,
            sel: Selection::new(SelectMode::Face),
            undo: Vec::new(),
            redo: Vec::new(),
            drag: None,
            last_op: None,
            aspect: Cell::new(1.0),
        }
    }

    fn view(&self) -> View {
        View::new(&self.camera, self.aspect.get())
    }

    fn handle_style(&self) -> Option<render::HandleStyle> {
        match self.tool {
            Tool::Move => Some(render::HandleStyle::Move),
            Tool::Rotate => Some(render::HandleStyle::Rotate),
            Tool::Scale => Some(render::HandleStyle::Scale),
            _ => None,
        }
    }

    fn handle_geometry(&self) -> Option<(V3, f32, render::HandleStyle)> {
        let style = self.handle_style()?;
        if !self.edit_mode {
            return None;
        }
        let verts = self.sel.affected_verts(&self.mesh);
        if verts.is_empty() {
            return None;
        }
        let sum = verts.iter().fold(math::v3(0.0, 0.0, 0.0), |s, &i| s.add(self.mesh.verts[i as usize]));
        Some((sum.scale(1.0 / verts.len() as f32), self.camera.distance * 0.18, style))
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
        self.drag = None;
        self.last_op = None;
    }

    pub fn set_select_mode(&mut self, mode: SelectMode) {
        if self.sel.mode != mode {
            self.sel = Selection::new(mode);
            self.last_op = None;
        }
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.drag = None;
    }

    pub fn select_all(&mut self) {
        self.sel = ops::select_all(&self.mesh, self.sel.mode);
        self.last_op = None;
    }

    /// Selects the element under (`nx`,`ny`) (fractions of width / height). Returns whether anything was hit.
    /// With the Loop Cut tool, tapping an edge cuts along its ring right away.
    pub fn tap(&mut self, nx: f32, ny: f32, add: bool) -> bool {
        if !self.edit_mode {
            return false;
        }
        self.last_op = None;
        let view = self.view();
        let p = (nx * view.aspect, ny);
        if self.tool == Tool::LoopCut {
            self.set_select_mode(SelectMode::Edge);
            let Some(e) = edit::pick_edge(&self.mesh, &view, p, PICK_RADIUS) else { return false };
            self.sel.clear();
            self.sel.edges.insert(e);
            return self.op_begin(ops::OpKind::LoopCut);
        }
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
        self.drag = None;
        self.last_op = None;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(Snapshot { mesh: self.mesh.clone(), sel: self.sel.clone() });
        self.mesh = next.mesh;
        self.sel = next.sel;
        self.drag = None;
        self.last_op = None;
        true
    }

    /// Bit 0: can undo, bit 1: can redo, bit 2: something is selected,
    /// bit 3: an adjustable operation is active, bits 4-5: select mode (0 vertex, 1 edge, 2 face).
    pub fn status(&self) -> i32 {
        (!self.undo.is_empty()) as i32
            | ((!self.redo.is_empty()) as i32) << 1
            | ((!self.sel.is_empty()) as i32) << 2
            | (self.last_op.is_some() as i32) << 3
            | (match self.sel.mode {
                SelectMode::Vertex => 0,
                SelectMode::Edge => 1,
                SelectMode::Face => 2,
            }) << 4
    }

    // ------------------------------------------------------------------ operations --

    /// Runs an operation on the selection with its default value. Returns false if it does not apply.
    pub fn op_begin(&mut self, kind: ops::OpKind) -> bool {
        if !self.edit_mode {
            return false;
        }
        let param = kind.range().map_or(0.0, |r| r.2);
        let Some((mesh, sel)) = ops::apply(kind, &self.mesh, &self.sel, param) else { return false };
        let base = Snapshot { mesh: self.mesh.clone(), sel: self.sel.clone() };
        self.push_undo();
        self.mesh = mesh;
        self.sel = sel;
        self.last_op = kind.range().map(|_| LastOp { kind, base, param });
        true
    }

    /// Re-runs the active operation from its base state with a new value.
    pub fn op_adjust(&mut self, value: f32) -> bool {
        let Some(l) = &mut self.last_op else { return false };
        let (lo, hi, _, int) = l.kind.range().unwrap_or((0.0, 1.0, 0.0, false));
        let v = value.clamp(lo, hi);
        l.param = if int { v.round() } else { v };
        let Some((mesh, sel)) = ops::apply(l.kind, &l.base.mesh, &l.base.sel, l.param) else { return false };
        self.mesh = mesh;
        self.sel = sel;
        true
    }

    /// (min, max, current, is_integer) of the active operation's value.
    pub fn op_range(&self) -> Option<(f32, f32, f32, bool)> {
        let l = self.last_op.as_ref()?;
        let (lo, hi, _, int) = l.kind.range()?;
        Some((lo, hi, l.param, int))
    }

    pub fn op_commit(&mut self) {
        self.last_op = None;
    }

    // ------------------------------------------------------------------- transforms --

    /// Starts dragging a transform handle if one is under the point. Records an undo step.
    pub fn drag_begin(&mut self, nx: f32, ny: f32) -> bool {
        let Some((centre, len, style)) = self.handle_geometry() else { return false };
        let view = self.view();
        let p = (nx * view.aspect, ny);
        let drag = match style {
            render::HandleStyle::Move => edit::pick_handle(&view, centre, len, p, HANDLE_RADIUS).map(Drag::Move),
            render::HandleStyle::Scale => {
                let near_centre = view
                    .project(centre)
                    .map_or(false, |c| ((c.0 - p.0).powi(2) + (c.1 - p.1).powi(2)).sqrt() < HANDLE_RADIUS * 0.8);
                if near_centre {
                    Some(Drag::Scale(3))
                } else {
                    edit::pick_handle(&view, centre, len, p, HANDLE_RADIUS).map(Drag::Scale)
                }
            }
            render::HandleStyle::Rotate => {
                edit::pick_ring(&view, centre, len * 0.9, p, HANDLE_RADIUS * 0.7).map(|axis| Drag::Rotate { axis, last: p })
            }
        };
        let Some(drag) = drag else { return false };
        self.last_op = None;
        self.push_undo();
        self.drag = Some(drag);
        true
    }

    /// Applies a drag step. Deltas are fractions of width / height.
    pub fn drag_update(&mut self, dnx: f32, dny: f32) {
        let (Some(drag), Some((centre, len, _))) = (self.drag, self.handle_geometry()) else { return };
        let view = self.view();
        let d = (dnx * view.aspect, dny);
        let verts = self.sel.affected_verts(&self.mesh);
        match drag {
            Drag::Move(axis) => {
                let Some(t) = axis_drag(&view, centre, axis, d) else { return };
                let dir = edit::axis_vec(axis);
                for vi in verts {
                    let v = &mut self.mesh.verts[vi as usize];
                    *v = v.add(dir.scale(t));
                }
            }
            Drag::Scale(axis) => {
                let f = if axis == 3 {
                    1.0 + (d.0 - d.1) * 2.0
                } else {
                    let Some(t) = axis_drag(&view, centre, axis, d) else { return };
                    1.0 + t / len
                }
                .max(0.05);
                let k = edit::axis_vec(axis.min(2));
                for vi in verts {
                    let v = &mut self.mesh.verts[vi as usize];
                    let rel = v.sub(centre);
                    *v = if axis == 3 { centre.add(rel.scale(f)) } else { centre.add(rel.add(k.scale(k.dot(rel) * (f - 1.0)))) };
                }
            }
            Drag::Rotate { axis, last } => {
                let Some(c) = view.project(centre) else { return };
                let now = (last.0 + d.0, last.1 + d.1);
                // Screen angles with y pointing up, so counter-clockwise is positive.
                let ang = |p: (f32, f32)| (-(p.1 - c.1)).atan2(p.0 - c.0);
                let mut da = ang(now) - ang(last);
                while da > std::f32::consts::PI {
                    da -= std::f32::consts::TAU;
                }
                while da < -std::f32::consts::PI {
                    da += std::f32::consts::TAU;
                }
                let k = edit::axis_vec(axis);
                if view.eye.sub(centre).dot(k) < 0.0 {
                    da = -da; // seen from the negative side the rotation looks reversed
                }
                for vi in verts {
                    let v = &mut self.mesh.verts[vi as usize];
                    *v = edit::rotate_about(*v, centre, k, da);
                }
                self.drag = Some(Drag::Rotate { axis, last: now });
            }
        }
    }

    pub fn drag_end(&mut self) {
        self.drag = None;
    }
}

/// How far (in world units) a screen drag `d` moves along `axis` at `centre`.
fn axis_drag(view: &View, centre: V3, axis: usize, d: (f32, f32)) -> Option<f32> {
    let dir = edit::axis_vec(axis);
    let (a, b) = (view.project(centre)?, view.project(centre.add(dir))?);
    let s = (b.0 - a.0, b.1 - a.1);
    let s2 = s.0 * s.0 + s.1 * s.1;
    if s2 < 1e-5 {
        return None; // axis points at the camera: no meaningful screen direction
    }
    Some((d.0 * s.0 + d.1 * s.1) / s2)
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

/// 0 move, 1 rotate, 2 scale, 3 loop cut, anything else: no tool.
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_set_tool(core: *mut Core, tool: i32) {
    if let Some(c) = core.as_mut() {
        c.set_tool(Tool::from_i32(tool));
    }
}

/// Selects everything of the current select mode.
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_select_all(core: *mut Core) {
    if let Some(c) = core.as_mut() {
        c.select_all();
    }
}

/// Runs an operation (0 extrude, 1 inset, 2 loop cut, 3 bevel, 4 merge, 5 delete) on the selection.
/// Returns 1 on success, 0 if it does not apply to the current selection.
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_op_begin(core: *mut Core, kind: i32) -> i32 {
    let (Some(c), Some(k)) = (core.as_mut(), ops::OpKind::from_i32(kind)) else { return 0 };
    c.op_begin(k) as i32
}

/// Changes the value of the active operation and recomputes it.
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_op_adjust(core: *mut Core, value: f32) -> i32 {
    core.as_mut().map_or(0, |c| c.op_adjust(value) as i32)
}

/// Writes [min, max, current, is_integer] of the active operation into `out[0..4]`. Returns 0 if none.
///
/// # Safety
/// `core` must be live and `out` must point to four writable `f32`s.
#[no_mangle]
pub unsafe extern "C" fn imonder_op_range(core: *const Core, out: *mut f32) -> i32 {
    let (Some(c), false) = (core.as_ref(), out.is_null()) else { return 0 };
    let Some((lo, hi, cur, int)) = c.op_range() else { return 0 };
    *out = lo;
    *out.add(1) = hi;
    *out.add(2) = cur;
    *out.add(3) = int as i32 as f32;
    1
}

/// Finishes the active operation (it can no longer be adjusted).
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_op_commit(core: *mut Core) {
    if let Some(c) = core.as_mut() {
        c.op_commit();
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

/// Bit 0: can undo, bit 1: can redo, bit 2: something is selected, bit 3: adjustable operation
/// active, bits 4-5: select mode.
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
        c.tap(0.5, 0.5, false);
        let before = c.mesh.verts.clone();
        let view = c.view();
        let (centre, len, _) = c.handle_geometry().unwrap();
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

    /// Grabs the handle of `axis` near its tip and drags by (`dx`,`dy`) viewport fractions.
    fn drag_axis_tip(c: &mut Core, axis: usize, dx: f32, dy: f32) {
        let view = c.view();
        let (centre, len, _) = c.handle_geometry().unwrap();
        let tip = view.project(centre.add(edit::axis_vec(axis).scale(len))).unwrap();
        assert!(c.drag_begin(tip.0 / view.aspect, tip.1), "handle {axis} not grabbed");
        c.drag_update(dx, dy);
        c.drag_end();
    }

    fn selected_centre(c: &Core) -> V3 {
        let v = c.sel.affected_verts(&c.mesh);
        v.iter().fold(math::v3(0.0, 0.0, 0.0), |s, &i| s.add(c.mesh.verts[i as usize])).scale(1.0 / v.len() as f32)
    }

    #[test]
    fn scale_along_axis_keeps_centre() {
        let mut c = front_core();
        c.set_tool(Tool::Scale);
        c.tap(0.5, 0.5, false);
        let centre = selected_centre(&c);
        drag_axis_tip(&mut c, 0, 0.1, 0.0);
        let after = selected_centre(&c);
        assert!((after.x - centre.x).abs() < 1e-4 && (after.z - centre.z).abs() < 1e-4);
        let xs: Vec<f32> = c.sel.affected_verts(&c.mesh).iter().map(|&i| c.mesh.verts[i as usize].x).collect();
        let width = xs.iter().cloned().fold(f32::MIN, f32::max) - xs.iter().cloned().fold(f32::MAX, f32::min);
        assert!(width > 2.05, "selected face got wider: {width}");
        assert!(c.undo());
        assert_eq!(c.mesh.verts, Mesh::cube(1.0).verts);
    }

    #[test]
    fn rotate_ring_turns_the_selection() {
        let mut c = front_core();
        c.set_tool(Tool::Rotate);
        c.tap(0.5, 0.5, false);
        let centre = selected_centre(&c);
        let view = c.view();
        let (_, len, _) = c.handle_geometry().unwrap();
        // The Y ring (around the viewing axis) seen face-on: grab it at the top and drag sideways (along its tangent).
        let p = view.project(edit::ring_point(centre, 1, len * 0.9, 0)).unwrap();
        assert!(c.drag_begin(p.0 / view.aspect, p.1));
        c.drag_update(0.08, 0.0);
        c.drag_end();
        let moved = c.mesh.verts.iter().zip(&Mesh::cube(1.0).verts).filter(|(a, b)| a.sub(**b).len() > 1e-3).count();
        assert_eq!(moved, 4);
        let after = selected_centre(&c);
        assert!(after.sub(centre).len() < 1e-4, "rotation is about the selection centre");
        // Distances to the centre are preserved.
        for &i in &c.sel.affected_verts(&c.mesh) {
            let d = c.mesh.verts[i as usize].sub(centre).len();
            assert!((d - 2.0f32.sqrt()).abs() < 1e-3);
        }
    }

    #[test]
    fn operation_can_be_adjusted_and_undone() {
        let mut c = front_core();
        c.tap(0.5, 0.5, false); // -Y face
        assert!(c.op_begin(ops::OpKind::Extrude));
        assert_eq!(c.mesh.faces.len(), 10);
        assert_eq!(c.status() & 8, 8);
        let (lo, hi, cur, int) = c.op_range().unwrap();
        assert_eq!((lo, hi, cur, int), (-2.0, 2.0, 0.5, false));
        let y = |c: &Core| c.mesh.verts.iter().map(|v| v.y).fold(f32::MAX, f32::min);
        assert!((y(&c) + 1.5).abs() < 1e-4);
        assert!(c.op_adjust(1.0));
        assert!((y(&c) + 2.0).abs() < 1e-4, "extruded further");
        assert_eq!(c.mesh.faces.len(), 10, "adjusting recomputes, it does not stack");
        assert!(c.undo(), "one undo removes the whole operation");
        assert_eq!(c.mesh.faces.len(), 6);
        assert_eq!(c.status() & 8, 0);
    }

    #[test]
    fn loop_cut_tool_cuts_on_tap() {
        let mut c = front_core();
        c.set_tool(Tool::LoopCut);
        let view = c.view();
        let mid = c.mesh.verts[0].add(c.mesh.verts[4]).scale(0.5); // a vertical edge at the front-left
        let p = view.project(mid).unwrap();
        assert!(c.tap(p.0 / view.aspect, p.1, false));
        assert_eq!(c.mesh.faces.len(), 10);
        assert_eq!(c.sel.mode, SelectMode::Edge);
        assert!(c.op_adjust(3.0));
        assert_eq!(c.mesh.faces.len(), 18);
    }

    #[test]
    fn inapplicable_operation_changes_nothing() {
        let mut c = front_core();
        c.set_select_mode(SelectMode::Vertex);
        c.tap(0.5, 0.5, false);
        assert!(!c.op_begin(ops::OpKind::Extrude));
        assert_eq!(c.status() & 1, 0, "no undo step recorded");
    }

    #[test]
    fn select_all_and_delete() {
        let mut c = front_core();
        c.select_all();
        assert_eq!(c.sel.faces.len(), 6);
        assert!(c.op_begin(ops::OpKind::Delete));
        assert!(c.mesh.faces.is_empty());
        assert!(c.undo());
        assert_eq!(c.mesh.faces.len(), 6);
    }

    #[test]
    fn drag_without_handle_hit_does_nothing() {
        let mut c = front_core();
        c.tap(0.5, 0.5, false);
        assert!(!c.drag_begin(0.02, 0.02));
        assert_eq!(c.status() & 1, 0);
    }

    #[test]
    fn selection_renders_accent() {
        let mut c = front_core();
        c.set_tool(Tool::None); // handles would cover the sampled pixel
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
