pub mod camera;
pub mod edit;
pub mod math;
pub mod mesh;
pub mod ops;
pub mod render;
pub mod scene;

use std::cell::Cell;

use camera::Camera;
use edit::{SelectMode, Selection, Snapshot, View};
use scene::Scene;
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

/// Objects that can be added: 0 cube, 1 plane, 2 cylinder, 3 cone, 4 sphere, 5 torus.
pub fn primitive(kind: i32) -> Option<(&'static str, Mesh)> {
    Some(match kind {
        0 => ("立方体", Mesh::cube(1.0)),
        1 => ("平面", Mesh::plane(1.0)),
        2 => ("円柱", Mesh::cylinder(1.0, 1.0, 32)),
        3 => ("円錐", Mesh::cone(1.0, 1.0, 32)),
        4 => ("球", Mesh::sphere(1.0, 32, 16)),
        5 => ("トーラス", Mesh::torus(1.0, 0.3, 32, 16)),
        _ => return None,
    })
}

pub struct Core {
    pub camera: Camera,
    pub scene: Scene,
    pub edit_mode: bool,
    pub tool: Tool,
    pub sel: Selection,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    drag: Option<Drag>,
    last_op: Option<LastOp>,
    aspect: Cell<f32>,
    rev: u64,
}

impl Core {
    pub fn new() -> Core {
        Core {
            camera: Camera::default(),
            scene: Scene::starter(),
            edit_mode: false,
            tool: Tool::Move,
            sel: Selection::new(SelectMode::Face),
            undo: Vec::new(),
            redo: Vec::new(),
            drag: None,
            last_op: None,
            aspect: Cell::new(1.0),
            rev: 0,
        }
    }

    pub fn mesh(&self) -> &Mesh {
        self.scene.mesh()
    }

    /// Counts every change to the work; used to decide when to autosave.
    pub fn revision(&self) -> u64 {
        self.rev
    }

    fn view(&self) -> View {
        View::new(&self.camera, self.aspect.get())
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot { scene: self.scene.clone(), sel: self.sel.clone() }
    }

    fn restore(&mut self, s: Snapshot) {
        self.scene = s.scene;
        self.sel = s.sel;
        self.drag = None;
        self.last_op = None;
        self.rev += 1;
    }

    fn handle_style(&self) -> Option<render::HandleStyle> {
        match self.tool {
            Tool::Move => Some(render::HandleStyle::Move),
            Tool::Rotate => Some(render::HandleStyle::Rotate),
            Tool::Scale => Some(render::HandleStyle::Scale),
            _ => None,
        }
    }

    /// Vertices a transform acts on: the selection in edit mode, the whole active object otherwise.
    fn transform_verts(&self) -> std::collections::BTreeSet<u32> {
        if self.edit_mode {
            self.sel.affected_verts(self.mesh())
        } else {
            (0..self.mesh().verts.len() as u32).collect()
        }
    }

    fn handle_geometry(&self) -> Option<(V3, f32, render::HandleStyle)> {
        let style = self.handle_style()?;
        if !self.scene.has_active() {
            return None;
        }
        let verts = self.transform_verts();
        if verts.is_empty() {
            return None;
        }
        let mesh = self.mesh();
        let centre = if self.edit_mode {
            let sum = verts.iter().fold(math::v3(0.0, 0.0, 0.0), |s, &i| s.add(mesh.verts[i as usize]));
            sum.scale(1.0 / verts.len() as f32)
        } else {
            self.scene.bounds_centre()?
        };
        Some((centre, self.camera.distance * 0.18, style))
    }

    pub fn render(&self, w: usize, h: usize, scale: f32, out: &mut [u8]) {
        self.aspect.set(w as f32 / h.max(1) as f32);
        let scene = render::Scene {
            objects: &self.scene.objects,
            active: self.scene.active,
            edit: self.edit_mode.then(|| render::EditOverlay { sel: &self.sel }),
            handles: self.handle_geometry(),
        };
        render::render_scene(&scene, &self.camera, w, h, scale, out);
    }

    /// Renders the whole work from a fixed three-quarter view (for gallery thumbnails).
    pub fn render_thumbnail(&self, w: usize, h: usize, out: &mut [u8]) {
        let mut cam = Camera::default();
        let mut lo = math::v3(f32::MAX, f32::MAX, f32::MAX);
        let mut hi = math::v3(f32::MIN, f32::MIN, f32::MIN);
        for o in self.scene.objects.iter().filter(|o| o.visible) {
            if let Some((a, b)) = o.mesh.bounds() {
                lo = math::v3(lo.x.min(a.x), lo.y.min(a.y), lo.z.min(a.z));
                hi = math::v3(hi.x.max(b.x), hi.y.max(b.y), hi.z.max(b.z));
            }
        }
        if lo.x <= hi.x {
            cam.target = lo.add(hi).scale(0.5);
            let radius = hi.sub(lo).len() * 0.5;
            cam.distance = (radius / (cam.fovy * 0.5).tan() * 1.15).max(1.0);
        }
        let scene = render::Scene { objects: &self.scene.objects, active: usize::MAX, edit: None, handles: None };
        render::render_scene(&scene, &cam, w, h, 1.0, out);
    }

    pub fn set_edit_mode(&mut self, on: bool) {
        self.edit_mode = on && self.scene.has_active();
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
        self.sel = ops::select_all(self.mesh(), self.sel.mode);
        self.last_op = None;
    }

    /// Edit mode: selects the element under (`nx`,`ny`) (fractions of width / height).
    /// Object mode: selects the object under the point. Returns whether anything was hit.
    /// With the Loop Cut tool, tapping an edge cuts along its ring right away.
    pub fn tap(&mut self, nx: f32, ny: f32, add: bool) -> bool {
        self.last_op = None;
        let view = self.view();
        let p = (nx * view.aspect, ny);
        if !self.edit_mode {
            let hit = self
                .scene
                .objects
                .iter()
                .enumerate()
                .filter(|(_, o)| o.visible)
                .filter_map(|(i, o)| edit::pick_face(&o.mesh, &view, p).map(|(_, z)| (i, z)))
                .min_by(|a, b| a.1.total_cmp(&b.1));
            self.sel = Selection::new(self.sel.mode);
            self.scene.active = hit.map_or(self.scene.objects.len(), |(i, _)| i);
            return hit.is_some();
        }
        if self.tool == Tool::LoopCut {
            self.set_select_mode(SelectMode::Edge);
            let Some(e) = edit::pick_edge(self.mesh(), &view, p, PICK_RADIUS) else { return false };
            self.sel.clear();
            self.sel.edges.insert(e);
            return self.op_begin(ops::OpKind::LoopCut);
        }
        let hit = match self.sel.mode {
            SelectMode::Vertex => edit::pick_vertex(self.mesh(), &view, p, PICK_RADIUS).map(Hit::Vert),
            SelectMode::Edge => edit::pick_edge(self.mesh(), &view, p, PICK_RADIUS).map(Hit::Edge),
            SelectMode::Face => edit::pick_face(self.mesh(), &view, p).map(|(f, _)| Hit::Face(f)),
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
        let s = self.snapshot();
        self.undo.push(s);
        if self.undo.len() > MAX_HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.rev += 1;
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo.pop() else { return false };
        let now = self.snapshot();
        self.redo.push(now);
        self.restore(prev);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        let now = self.snapshot();
        self.undo.push(now);
        self.restore(next);
        true
    }

    /// Bit 0: can undo, bit 1: can redo, bit 2: something is selected,
    /// bit 3: an adjustable operation is active, bits 4-5: select mode (0 vertex, 1 edge, 2 face),
    /// bit 6: edit mode, bit 7: an object is active.
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
            | (self.edit_mode as i32) << 6
            | (self.scene.has_active() as i32) << 7
    }

    // -------------------------------------------------------------------- objects --

    /// Adds a primitive at the point the camera looks at and makes it active.
    pub fn add_object(&mut self, kind: i32) -> bool {
        let Some((name, mut mesh)) = primitive(kind) else { return false };
        self.push_undo();
        for v in &mut mesh.verts {
            *v = v.add(self.camera.target);
        }
        self.scene.add(name, mesh);
        self.sel = Selection::new(self.sel.mode);
        self.edit_mode = false;
        self.last_op = None;
        true
    }

    pub fn duplicate_object(&mut self) -> bool {
        if !self.scene.has_active() {
            return false;
        }
        self.push_undo();
        self.sel = Selection::new(self.sel.mode);
        self.last_op = None;
        self.scene.duplicate_active()
    }

    pub fn delete_object(&mut self) -> bool {
        if !self.scene.has_active() {
            return false;
        }
        self.push_undo();
        self.edit_mode = false;
        self.sel = Selection::new(self.sel.mode);
        self.last_op = None;
        self.scene.remove_active();
        true
    }

    pub fn select_object(&mut self, index: usize) -> bool {
        if index >= self.scene.objects.len() {
            return false;
        }
        if self.scene.active != index {
            self.scene.active = index;
            self.sel = Selection::new(self.sel.mode);
            self.edit_mode = false;
            self.last_op = None;
        }
        true
    }

    pub fn set_visible(&mut self, index: usize, visible: bool) -> bool {
        let Some(o) = self.scene.objects.get_mut(index) else { return false };
        if o.visible != visible {
            o.visible = visible;
            self.rev += 1;
        }
        true
    }

    // ---------------------------------------------------------------------- files --

    pub fn to_bytes(&self) -> Vec<u8> {
        self.scene.to_bytes(&self.camera)
    }

    /// Replaces the work with a saved one. Leaves everything untouched if the data is invalid.
    pub fn load(&mut self, data: &[u8]) -> bool {
        let Some((scene, camera)) = Scene::from_bytes(data) else { return false };
        self.reset(scene, camera);
        true
    }

    pub fn new_work(&mut self) {
        self.reset(Scene::starter(), Camera::default());
    }

    fn reset(&mut self, scene: Scene, camera: Camera) {
        self.scene = scene;
        self.camera = camera;
        self.sel = Selection::new(SelectMode::Face);
        self.undo.clear();
        self.redo.clear();
        self.drag = None;
        self.last_op = None;
        self.edit_mode = false;
        self.rev += 1;
    }

    // ------------------------------------------------------------------ operations --

    /// Runs an operation on the selection with its default value. Returns false if it does not apply.
    pub fn op_begin(&mut self, kind: ops::OpKind) -> bool {
        if !self.edit_mode {
            return false;
        }
        let param = kind.range().map_or(0.0, |r| r.2);
        let Some((mesh, sel)) = ops::apply(kind, self.mesh(), &self.sel, param) else { return false };
        let base = self.snapshot();
        self.push_undo();
        self.scene.set_mesh(mesh);
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
        let Some((mesh, sel)) = ops::apply(l.kind, l.base.scene.mesh(), &l.base.sel, l.param) else { return false };
        self.scene.set_mesh(mesh);
        self.sel = sel;
        self.rev += 1;
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
        let verts = self.transform_verts();
        let mut next_drag = drag;
        let apply: Box<dyn Fn(V3) -> V3>;
        match drag {
            Drag::Move(axis) => {
                let Some(t) = axis_drag(&view, centre, axis, d) else { return };
                let dir = edit::axis_vec(axis);
                apply = Box::new(move |v| v.add(dir.scale(t)));
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
                apply = if axis == 3 {
                    Box::new(move |v| centre.add(v.sub(centre).scale(f)))
                } else {
                    Box::new(move |v| {
                        let rel = v.sub(centre);
                        centre.add(rel.add(k.scale(k.dot(rel) * (f - 1.0))))
                    })
                };
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
                apply = Box::new(move |v| edit::rotate_about(v, centre, k, da));
                next_drag = Drag::Rotate { axis, last: now };
            }
        }
        if let Some(mesh) = self.scene.mesh_mut() {
            for vi in verts {
                let v = &mut mesh.verts[vi as usize];
                *v = apply(*v);
            }
        }
        self.drag = Some(next_drag);
        self.rev += 1;
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

/// Adds a primitive (0 cube, 1 plane, 2 cylinder, 3 cone, 4 sphere, 5 torus). Returns 1 on success.
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_add_object(core: *mut Core, kind: i32) -> i32 {
    core.as_mut().map_or(0, |c| c.add_object(kind) as i32)
}

/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_duplicate_object(core: *mut Core) -> i32 {
    core.as_mut().map_or(0, |c| c.duplicate_object() as i32)
}

/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_delete_object(core: *mut Core) -> i32 {
    core.as_mut().map_or(0, |c| c.delete_object() as i32)
}

/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_select_object(core: *mut Core, index: i32) -> i32 {
    core.as_mut().map_or(0, |c| (index >= 0 && c.select_object(index as usize)) as i32)
}

/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_set_visible(core: *mut Core, index: i32, visible: i32) -> i32 {
    core.as_mut().map_or(0, |c| (index >= 0 && c.set_visible(index as usize, visible != 0)) as i32)
}

fn leak_bytes(v: Vec<u8>, out_len: *mut u32) -> *mut u8 {
    let b = v.into_boxed_slice();
    if !out_len.is_null() {
        // SAFETY: the caller passes a valid pointer to a u32.
        unsafe { *out_len = b.len() as u32 };
    }
    Box::into_raw(b) as *mut u8
}

/// Frees a buffer returned by `imonder_scene_json` / `imonder_save`.
///
/// # Safety
/// `ptr`/`len` must be exactly what one of those functions returned, and be freed once.
#[no_mangle]
pub unsafe extern "C" fn imonder_free_bytes(ptr: *mut u8, len: u32) {
    if !ptr.is_null() {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len as usize)));
    }
}

/// UTF-8 JSON list of the objects: `[{"name":..,"visible":..,"active":..}]`. Free with `imonder_free_bytes`.
///
/// # Safety
/// `core` must be live and `out_len` a writable `u32`.
#[no_mangle]
pub unsafe extern "C" fn imonder_scene_json(core: *const Core, out_len: *mut u32) -> *mut u8 {
    let json = core.as_ref().map_or_else(|| "[]".to_string(), |c| c.scene.to_json());
    leak_bytes(json.into_bytes(), out_len)
}

/// Serialises the work (`.imnd`). Free with `imonder_free_bytes`.
///
/// # Safety
/// `core` must be live and `out_len` a writable `u32`.
#[no_mangle]
pub unsafe extern "C" fn imonder_save(core: *const Core, out_len: *mut u32) -> *mut u8 {
    let bytes = core.as_ref().map_or_else(Vec::new, |c| c.to_bytes());
    leak_bytes(bytes, out_len)
}

/// Loads a work. Returns 1 on success; the current work is untouched on failure.
///
/// # Safety
/// `core` must be live and `data` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn imonder_load(core: *mut Core, data: *const u8, len: u32) -> i32 {
    let (Some(c), false) = (core.as_mut(), data.is_null()) else { return 0 };
    c.load(std::slice::from_raw_parts(data, len as usize)) as i32
}

/// Starts a new work (one cube).
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_new_work(core: *mut Core) {
    if let Some(c) = core.as_mut() {
        c.new_work();
    }
}

/// Changes every time the work changes.
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_revision(core: *const Core) -> u64 {
    core.as_ref().map_or(0, |c| c.revision())
}

/// Renders the whole work from a fixed view into `out` (`w*h*4` bytes). Returns 1 on success.
///
/// # Safety
/// `core` must be live and `out` must point to at least `w*h*4` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn imonder_render_thumbnail(core: *const Core, w: u32, h: u32, out: *mut u8) -> i32 {
    let (Some(c), false) = (core.as_ref(), out.is_null()) else { return 0 };
    if w == 0 || h == 0 || w > 2048 || h > 2048 {
        return 0;
    }
    c.render_thumbnail(w as usize, h as usize, std::slice::from_raw_parts_mut(out, w as usize * h as usize * 4));
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
        let (vx, vy, _) = view.project(c.mesh().verts[0]).unwrap(); // (-1,-1,-1)
        c.set_select_mode(SelectMode::Vertex);
        assert!(c.tap(vx / view.aspect, vy, false));
        assert!(c.sel.verts.contains(&0));
        // Midpoint of the front-bottom edge (0,1).
        let mid = c.mesh().verts[0].add(c.mesh().verts[1]).scale(0.5);
        let (ex, ey, _) = view.project(mid).unwrap();
        c.set_select_mode(SelectMode::Edge);
        assert!(c.tap(ex / view.aspect, ey, false));
        assert!(c.sel.edges.contains(&(0, 1)));
    }

    #[test]
    fn move_along_x_and_undo_redo() {
        let mut c = front_core();
        c.tap(0.5, 0.5, false);
        let before = c.mesh().verts.clone();
        let view = c.view();
        let (centre, len, _) = c.handle_geometry().unwrap();
        let tip = view.project(centre.add(math::v3(len, 0.0, 0.0))).unwrap();
        // Grab the X handle near its tip, drag 0.1 of the width to the right.
        assert!(c.drag_begin(tip.0 / view.aspect, tip.1));
        c.drag_update(0.1, 0.0);
        c.drag_end();
        let moved: Vec<usize> = (0..8).filter(|&i| (c.mesh().verts[i].x - before[i].x).abs() > 1e-4).collect();
        assert_eq!(moved.len(), 4, "only the selected face moves");
        assert!(moved.iter().all(|&i| c.mesh().verts[i].x > before[i].x));
        assert!(c.mesh().verts.iter().zip(&before).all(|(a, b)| (a.y - b.y).abs() < 1e-5 && (a.z - b.z).abs() < 1e-5));
        assert_eq!(c.status() & 3, 1);
        assert!(c.undo());
        assert_eq!(c.mesh().verts, before);
        assert_eq!(c.status() & 3, 2);
        assert!(c.redo());
        assert!(c.mesh().verts != before);
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
        let v = c.sel.affected_verts(c.mesh());
        v.iter().fold(math::v3(0.0, 0.0, 0.0), |s, &i| s.add(c.mesh().verts[i as usize])).scale(1.0 / v.len() as f32)
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
        let xs: Vec<f32> = c.sel.affected_verts(c.mesh()).iter().map(|&i| c.mesh().verts[i as usize].x).collect();
        let width = xs.iter().cloned().fold(f32::MIN, f32::max) - xs.iter().cloned().fold(f32::MAX, f32::min);
        assert!(width > 2.05, "selected face got wider: {width}");
        assert!(c.undo());
        assert_eq!(c.mesh().verts, Mesh::cube(1.0).verts);
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
        let moved = c.mesh().verts.iter().zip(&Mesh::cube(1.0).verts).filter(|(a, b)| a.sub(**b).len() > 1e-3).count();
        assert_eq!(moved, 4);
        let after = selected_centre(&c);
        assert!(after.sub(centre).len() < 1e-4, "rotation is about the selection centre");
        // Distances to the centre are preserved.
        for &i in &c.sel.affected_verts(c.mesh()) {
            let d = c.mesh().verts[i as usize].sub(centre).len();
            assert!((d - 2.0f32.sqrt()).abs() < 1e-3);
        }
    }

    #[test]
    fn operation_can_be_adjusted_and_undone() {
        let mut c = front_core();
        c.tap(0.5, 0.5, false); // -Y face
        assert!(c.op_begin(ops::OpKind::Extrude));
        assert_eq!(c.mesh().faces.len(), 10);
        assert_eq!(c.status() & 8, 8);
        let (lo, hi, cur, int) = c.op_range().unwrap();
        assert_eq!((lo, hi, cur, int), (-2.0, 2.0, 0.5, false));
        let y = |c: &Core| c.mesh().verts.iter().map(|v| v.y).fold(f32::MAX, f32::min);
        assert!((y(&c) + 1.5).abs() < 1e-4);
        assert!(c.op_adjust(1.0));
        assert!((y(&c) + 2.0).abs() < 1e-4, "extruded further");
        assert_eq!(c.mesh().faces.len(), 10, "adjusting recomputes, it does not stack");
        assert!(c.undo(), "one undo removes the whole operation");
        assert_eq!(c.mesh().faces.len(), 6);
        assert_eq!(c.status() & 8, 0);
    }

    #[test]
    fn loop_cut_tool_cuts_on_tap() {
        let mut c = front_core();
        c.set_tool(Tool::LoopCut);
        let view = c.view();
        let mid = c.mesh().verts[0].add(c.mesh().verts[4]).scale(0.5); // a vertical edge at the front-left
        let p = view.project(mid).unwrap();
        assert!(c.tap(p.0 / view.aspect, p.1, false));
        assert_eq!(c.mesh().faces.len(), 10);
        assert_eq!(c.sel.mode, SelectMode::Edge);
        assert!(c.op_adjust(3.0));
        assert_eq!(c.mesh().faces.len(), 18);
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
        assert!(c.mesh().faces.is_empty());
        assert!(c.undo());
        assert_eq!(c.mesh().faces.len(), 6);
    }

    fn object_core() -> Core {
        let mut c = Core::new();
        c.camera.snap_axis(3);
        let mut buf = vec![0u8; 400 * 300 * 4];
        c.render(400, 300, 1.0, &mut buf);
        c
    }

    #[test]
    fn adding_objects_places_them_at_the_view_centre_and_undo_removes_them() {
        let mut c = object_core();
        c.camera.target = math::v3(5.0, 0.0, 0.0);
        for kind in 0..6 {
            assert!(c.add_object(kind), "kind {kind}");
        }
        assert!(!c.add_object(99));
        assert_eq!(c.scene.objects.len(), 7);
        assert_eq!(c.scene.active, 6);
        let (lo, hi) = c.mesh().bounds().unwrap();
        assert!((lo.x + hi.x) * 0.5 > 4.9, "torus sits at the view centre: {lo:?} {hi:?}");
        assert!(c.undo());
        assert_eq!(c.scene.objects.len(), 6);
        while c.undo() {}
        assert_eq!(c.scene.objects.len(), 1);
    }

    #[test]
    fn tapping_selects_the_front_object_and_empty_space_deselects() {
        let mut c = object_core();
        c.camera.target = math::v3(0.0, 0.0, 0.0);
        // A big sphere in front of the cube (closer to the camera at -Y).
        c.add_object(4);
        for v in &mut c.scene.mesh_mut().unwrap().verts {
            v.y -= 3.0;
        }
        c.scene.active = 0;
        assert!(c.tap(0.5, 0.5, false));
        assert_eq!(c.scene.active, 1, "the sphere hides the cube");
        assert!(!c.tap(0.01, 0.01, false));
        assert!(!c.scene.has_active());
        assert_eq!(c.status() & 128, 0);
    }

    #[test]
    fn object_mode_transform_moves_only_the_active_object() {
        let mut c = object_core();
        c.add_object(4);
        c.scene.active = 0;
        let before: Vec<_> = c.scene.objects.iter().map(|o| o.mesh.verts.clone()).collect();
        let view = c.view();
        let (centre, len, _) = c.handle_geometry().unwrap();
        let tip = view.project(centre.add(edit::axis_vec(0).scale(len))).unwrap();
        assert!(c.drag_begin(tip.0 / view.aspect, tip.1));
        c.drag_update(0.1, 0.0);
        c.drag_end();
        assert!(c.scene.objects[0].mesh.verts.iter().zip(&before[0]).all(|(a, b)| a.x > b.x + 1e-3));
        assert_eq!(c.scene.objects[1].mesh.verts, before[1], "other objects stay put");
        assert!(c.undo());
        assert_eq!(c.scene.objects[0].mesh.verts, before[0]);
    }

    #[test]
    fn duplicate_delete_visibility_and_selection_by_index() {
        let mut c = object_core();
        assert!(c.duplicate_object());
        assert_eq!(c.scene.objects.len(), 2);
        assert!(c.select_object(0) && !c.select_object(5));
        assert!(c.set_visible(1, false));
        assert!(!c.scene.objects[1].visible);
        assert!(c.delete_object());
        assert_eq!(c.scene.objects.len(), 1);
        assert!(c.delete_object() && !c.delete_object());
        assert!(!c.scene.has_active());
        c.set_edit_mode(true);
        assert!(!c.edit_mode, "nothing to edit");
        assert!(c.undo() && c.undo());
        assert_eq!(c.scene.objects.len(), 2);
    }

    #[test]
    fn save_load_roundtrip_and_bad_data() {
        let mut c = object_core();
        c.add_object(2);
        c.set_visible(0, false);
        let bytes = c.to_bytes();
        let mut d = Core::new();
        d.camera.yaw = 2.0;
        assert!(d.load(&bytes));
        assert_eq!(d.scene.objects, c.scene.objects);
        assert_eq!(d.scene.active, c.scene.active);
        assert_eq!(d.camera.yaw, c.camera.yaw);
        assert_eq!(d.status() & 3, 0, "history starts empty");
        let before = d.scene.objects.clone();
        assert!(!d.load(b"nope"));
        assert_eq!(d.scene.objects, before, "failed load changes nothing");
        d.new_work();
        assert_eq!(d.scene.objects.len(), 1);
    }

    #[test]
    fn revision_changes_on_every_edit_but_not_on_view_changes() {
        let mut c = object_core();
        let r0 = c.revision();
        c.camera.orbit(0.3, 0.1);
        let mut buf = vec![0u8; 100 * 100 * 4];
        c.render(100, 100, 1.0, &mut buf);
        assert_eq!(c.revision(), r0, "camera moves are not edits");
        c.add_object(0);
        assert!(c.revision() > r0);
        let r1 = c.revision();
        c.undo();
        assert!(c.revision() > r1);
    }

    #[test]
    fn thumbnail_shows_the_work_regardless_of_the_current_view() {
        let mut c = object_core();
        c.camera.target = math::v3(50.0, 50.0, 50.0); // looking at nothing
        let (w, h) = (96, 96);
        let mut buf = vec![0u8; w * h * 4];
        c.render_thumbnail(w, h, &mut buf);
        let centre = &buf[(h / 2 * w + w / 2) * 4..][..3];
        let corner = &buf[..3];
        assert_ne!(centre, corner, "the cube is in the middle of the thumbnail");
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
