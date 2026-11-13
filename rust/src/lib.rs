pub mod camera;
pub mod edit;
pub mod export;
pub mod math;
pub mod mesh;
pub mod ops;
pub mod render;
pub mod scene;
pub mod snap;

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
    /// `free`: how far the finger has dragged in total (world units along the axis).
    Move { axis: usize, free: f32 },
    /// `axis` 0..3, 3 = uniform; `free`: the total factor dragged so far.
    Scale { axis: usize, free: f32 },
    /// `free`: the total angle dragged so far.
    Rotate { axis: usize, last: (f32, f32), free: f32 },
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

/// What a captured image shows besides the models.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaptureOpts {
    pub grid: bool,
    pub edges: bool,
}

/// Camera presets for capturing: (orthographic, yaw, pitch).
/// 0 diorama (isometric, looking down at 35°), 1 three-quarter perspective, 2 front, 3 top, 4 side.
pub fn capture_preset(preset: i32) -> Option<(bool, f32, f32)> {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};
    Some(match preset {
        0 => (true, -FRAC_PI_4, (1.0f32 / 2.0f32.sqrt()).atan()),
        1 => (false, -FRAC_PI_4 - 0.3, 0.42),
        2 => (true, -FRAC_PI_2, 0.0),
        3 => (true, -FRAC_PI_2, 1.55),
        4 => (true, 0.0, 0.0),
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
    /// Capture mode: the view shows exactly what an exported image will look like.
    capture: Option<CaptureOpts>,
    /// Snap transforms to other objects' surfaces / the grid / angle and scale steps.
    pub snap: bool,
    /// The editing camera, put back when capture mode ends.
    camera_before_capture: Option<Camera>,
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
            capture: None,
            snap: false,
            camera_before_capture: None,
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
    /// What a transform moves: (object index, its vertices). The element selection of the
    /// active object in edit mode; every selected object, whole, in object mode.
    fn transform_targets(&self) -> Vec<(usize, std::collections::BTreeSet<u32>)> {
        if self.edit_mode {
            if !self.scene.has_active() {
                return Vec::new();
            }
            let v = self.sel.affected_verts(self.mesh());
            if v.is_empty() {
                return Vec::new();
            }
            vec![(self.scene.active, v)]
        } else {
            self.scene
                .selected_indices()
                .into_iter()
                .map(|i| (i, (0..self.scene.objects[i].mesh.verts.len() as u32).collect()))
                .filter(|(_, v): &(usize, std::collections::BTreeSet<u32>)| !v.is_empty())
                .collect()
        }
    }

    /// Pivot of the targets in `scene`: the centroid of the selected elements in edit mode,
    /// the centre of the selected objects' bounds in object mode.
    fn pivot_in(&self, scene: &Scene, targets: &[(usize, std::collections::BTreeSet<u32>)]) -> Option<V3> {
        if self.edit_mode {
            let (oi, verts) = targets.first()?;
            let mesh = &scene.objects.get(*oi)?.mesh;
            let sum = verts.iter().fold(math::v3(0.0, 0.0, 0.0), |s, &i| s.add(mesh.verts[i as usize]));
            Some(sum.scale(1.0 / verts.len() as f32))
        } else {
            let (lo, hi) = scene.selection_bounds()?;
            Some(lo.add(hi).scale(0.5))
        }
    }

    fn handle_geometry(&self) -> Option<(V3, f32, render::HandleStyle)> {
        let style = self.handle_style()?;
        let targets = self.transform_targets();
        if targets.is_empty() {
            return None;
        }
        let centre = self.pivot_in(&self.scene, &targets)?;
        Some((centre, self.camera.distance * 0.18, style))
    }

    pub fn render(&self, w: usize, h: usize, scale: f32, out: &mut [u8]) {
        self.aspect.set(w as f32 / h.max(1) as f32);
        if let Some(c) = self.capture {
            let opts = render::RenderOpts { grid: c.grid, edges: c.edges, vignette: false, ..Default::default() };
            render::render_scene(&self.clean_scene(), &self.camera, w, h, scale, out, &opts);
            return;
        }
        let scene = render::Scene {
            objects: &self.scene.objects,
            active: self.scene.active,
            edit: self.edit_mode.then(|| render::EditOverlay { sel: &self.sel }),
            handles: self.handle_geometry(),
            highlight: !self.edit_mode,
        };
        render::render_scene(&scene, &self.camera, w, h, scale, out, &render::RenderOpts::default());
    }

    /// The models only: no selection, handles or highlight.
    fn clean_scene(&self) -> render::Scene<'_> {
        render::Scene { objects: &self.scene.objects, active: usize::MAX, edit: None, handles: None, highlight: false }
    }

    // ------------------------------------------------------------------ capture --

    pub fn set_ortho(&mut self, on: bool) {
        self.camera.ortho = on;
    }

    /// Enters (Some) or leaves (None) capture mode, or changes its options. Selection tools are
    /// off while capturing. Leaving restores the camera the editor had before.
    pub fn set_capture(&mut self, opts: Option<CaptureOpts>) {
        match (self.capture.is_some(), opts.is_some()) {
            (false, true) => self.camera_before_capture = Some(self.camera),
            (true, false) => {
                if let Some(cam) = self.camera_before_capture.take() {
                    self.camera = cam;
                }
            }
            _ => {}
        }
        self.capture = opts;
        self.drag = None;
        self.last_op = None;
    }

    /// Bounds of everything visible.
    fn visible_bounds(&self) -> Option<(V3, V3)> {
        let mut out: Option<(V3, V3)> = None;
        for o in self.scene.objects.iter().filter(|o| o.visible) {
            if let Some((a, b)) = o.mesh.bounds() {
                out = Some(match out {
                    None => (a, b),
                    Some((lo, hi)) => (
                        math::v3(lo.x.min(a.x), lo.y.min(a.y), lo.z.min(a.z)),
                        math::v3(hi.x.max(b.x), hi.y.max(b.y), hi.z.max(b.z)),
                    ),
                });
            }
        }
        out
    }

    /// Points the camera from a preset direction and zooms so every visible object fits inside
    /// `rect` (fractions of a view with `view_aspect`), with a small margin.
    pub fn apply_preset(&mut self, preset: i32, view_aspect: f32, rect: [f32; 4]) -> bool {
        let Some((ortho, yaw, pitch)) = capture_preset(preset) else { return false };
        self.camera.ortho = ortho;
        self.camera.yaw = yaw;
        self.camera.pitch = pitch;
        self.frame_all(view_aspect, rect);
        true
    }

    /// Zooms and pans so every visible object fits in `rect`, centred in it.
    pub fn frame_all(&mut self, view_aspect: f32, rect: [f32; 4]) {
        let Some((lo, hi)) = self.visible_bounds() else { return };
        let centre = lo.add(hi).scale(0.5);
        let corners: Vec<V3> = (0..8)
            .map(|i| math::v3(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z }))
            .collect();
        // The frame's centre in clip space; the camera is panned so the model's centre lands there.
        let (fx, fy) = (rect[0] + rect[2] - 1.0, 1.0 - (rect[1] + rect[3]));
        let (right, up) = self.camera.basis();
        let place = |cam: &mut Camera, d: f32| {
            cam.distance = d;
            let (hw, hh) = cam.half_extents(view_aspect);
            cam.target = centre.sub(right.scale(fx * hw)).sub(up.scale(fy * hh));
        };
        const MARGIN: f32 = 0.82; // keep the model within 82% of the frame
        let crop = math::M4::crop(rect);
        let fits = |cam: &Camera| {
            let m = crop.mul(&cam.proj(view_aspect)).mul(&cam.view());
            corners.iter().all(|&p| {
                let c = m.apply(p);
                c[3] > 0.05 && (c[0] / c[3]).abs() <= MARGIN && (c[1] / c[3]).abs() <= MARGIN
            })
        };
        // Fitting gets easier the further away (or zoomed out) the camera is: bisect.
        let (mut near, mut far) = (0.5f32, 200.0f32);
        let mut cam = self.camera;
        place(&mut cam, far);
        if fits(&cam) {
            for _ in 0..40 {
                let mid = (near * far).sqrt();
                place(&mut cam, mid);
                if fits(&cam) {
                    far = mid;
                } else {
                    near = mid;
                }
            }
        }
        place(&mut cam, far);
        self.camera = cam;
    }

    /// Renders the part of the view inside `rect` (as framed on screen) at `w` x `h`.
    pub fn render_export(&self, w: usize, h: usize, scale: f32, view_aspect: f32, rect: [f32; 4], transparent: bool, out: &mut [u8]) {
        let c = self.capture.unwrap_or(CaptureOpts { grid: true, edges: true });
        let opts = render::RenderOpts { grid: c.grid, edges: c.edges, transparent, crop: Some((view_aspect, rect)), vignette: false };
        render::render_scene(&self.clean_scene(), &self.camera, w, h, scale, out, &opts);
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
        render::render_scene(&self.clean_scene(), &cam, w, h, 1.0, out, &render::RenderOpts::default());
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
        if self.capture.is_some() {
            return false;
        }
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
            match (hit, add) {
                (Some((i, _)), true) => {
                    self.scene.toggle(i);
                }
                (Some((i, _)), false) => {
                    self.scene.select_only(i);
                }
                (None, true) => {}
                (None, false) => self.scene.clear_selection(),
            }
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
    /// bit 6: edit mode, bit 7: an object is active, bit 8: orthographic view, bit 9: capture mode, bit 10: snapping, bit 11: two or more objects selected,
    /// bit 12: a selected object is in a group.
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
            | (self.camera.ortho as i32) << 8
            | (self.capture.is_some() as i32) << 9
            | (self.snap as i32) << 10
            | ((self.scene.selected_indices().len() >= 2) as i32) << 11
            | (self.scene.objects.iter().any(|o| o.selected && o.group != 0) as i32) << 12
    }

    // -------------------------------------------------------------------- objects --

    /// Adds a primitive at the point the camera looks at and makes it active.
    pub fn add_object(&mut self, kind: i32) -> bool {
        let Some((name, mut mesh)) = primitive(kind) else { return false };
        self.push_undo();
        for v in &mut mesh.verts {
            *v = v.add(self.camera.target);
        }
        let i = self.scene.add(name, mesh);
        self.scene.select_only(i);
        self.sel = Selection::new(self.sel.mode);
        self.edit_mode = false;
        self.last_op = None;
        true
    }

    /// Copies every selected object in place; the copies become the selection.
    pub fn duplicate_object(&mut self) -> bool {
        if self.scene.selected_indices().is_empty() {
            return false;
        }
        self.push_undo();
        self.sel = Selection::new(self.sel.mode);
        self.edit_mode = false;
        self.last_op = None;
        self.scene.duplicate_selected()
    }

    /// Deletes every selected object.
    pub fn delete_object(&mut self) -> bool {
        if self.scene.selected_indices().is_empty() {
            return false;
        }
        self.push_undo();
        self.edit_mode = false;
        self.sel = Selection::new(self.sel.mode);
        self.last_op = None;
        self.scene.remove_selected()
    }

    /// Selects only this object (and its group), e.g. from the scene list.
    pub fn select_object(&mut self, index: usize) -> bool {
        if index >= self.scene.objects.len() {
            return false;
        }
        if self.scene.active != index {
            self.sel = Selection::new(self.sel.mode);
            self.edit_mode = false;
            self.last_op = None;
        }
        self.scene.select_only(index)
    }

    /// Adds the object (and its group) to the selection or takes it out.
    pub fn toggle_object(&mut self, index: usize) -> bool {
        if index >= self.scene.objects.len() {
            return false;
        }
        self.sel = Selection::new(self.sel.mode);
        self.edit_mode = false;
        self.last_op = None;
        self.scene.toggle(index)
    }

    pub fn select_all_objects(&mut self) {
        self.edit_mode = false;
        self.last_op = None;
        self.scene.select_all_visible();
    }

    /// Groups (`on`) or ungroups the selected objects. One undo step.
    pub fn group_objects(&mut self, on: bool) -> bool {
        let mut probe = self.scene.clone();
        let changed = if on { probe.group_selected() } else { probe.ungroup_selected() };
        if !changed {
            return false;
        }
        self.push_undo();
        self.scene = probe;
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
        if self.capture.is_some() {
            return false;
        }
        let Some((centre, len, style)) = self.handle_geometry() else { return false };
        let view = self.view();
        let p = (nx * view.aspect, ny);
        let drag = match style {
            render::HandleStyle::Move => edit::pick_handle(&view, centre, len, p, HANDLE_RADIUS).map(|axis| Drag::Move { axis, free: 0.0 }),
            render::HandleStyle::Scale => {
                let near_centre = view
                    .project(centre)
                    .map_or(false, |c| ((c.0 - p.0).powi(2) + (c.1 - p.1).powi(2)).sqrt() < HANDLE_RADIUS * 0.8);
                if near_centre {
                    Some(Drag::Scale { axis: 3, free: 1.0 })
                } else {
                    edit::pick_handle(&view, centre, len, p, HANDLE_RADIUS).map(|axis| Drag::Scale { axis, free: 1.0 })
                }
            }
            render::HandleStyle::Rotate => {
                edit::pick_ring(&view, centre, len * 0.9, p, HANDLE_RADIUS * 0.7).map(|axis| Drag::Rotate { axis, last: p, free: 0.0 })
            }
        };
        let Some(drag) = drag else { return false };
        self.begin_transform(drag);
        true
    }

    /// Records the undo step the drag works from: every update recomputes the result from
    /// that starting state, so snapping never accumulates rounding.
    fn begin_transform(&mut self, drag: Drag) {
        self.last_op = None;
        self.push_undo();
        self.drag = Some(drag);
    }

    /// Applies a drag step. Deltas are fractions of width / height.
    pub fn drag_update(&mut self, dnx: f32, dny: f32) {
        let (Some(drag), Some((centre, len, _))) = (self.drag, self.handle_geometry()) else { return };
        let view = self.view();
        let d = (dnx * view.aspect, dny);
        let next = match drag {
            Drag::Move { axis, free } => {
                let Some(t) = axis_drag(&view, centre, axis, d) else { return };
                Drag::Move { axis, free: free + t }
            }
            Drag::Scale { axis, free } => {
                let f = if axis == 3 {
                    1.0 + (d.0 - d.1) * 2.0
                } else {
                    let Some(t) = axis_drag(&view, centre, axis, d) else { return };
                    1.0 + t / len
                }
                .max(0.05);
                Drag::Scale { axis, free: (free * f).max(0.01) }
            }
            Drag::Rotate { axis, last, free } => {
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
                if view.eye.sub(centre).dot(edit::axis_vec(axis)) < 0.0 {
                    da = -da; // seen from the negative side the rotation looks reversed
                }
                Drag::Rotate { axis, last: now, free: free + da }
            }
        };
        self.drag = Some(next);
        self.apply_transform(next);
    }

    /// Sets the moving vertices to their starting positions transformed by the drag so far
    /// (snapped when snapping is on).
    fn apply_transform(&mut self, drag: Drag) {
        let Some(base) = self.undo.last().map(|s| s.scene.clone()) else { return };
        let targets = self.transform_targets();
        if targets.is_empty()
            || targets.iter().any(|(oi, _)| base.objects.get(*oi).map(|o| o.mesh.verts.len()) != self.scene.objects.get(*oi).map(|o| o.mesh.verts.len()))
        {
            return;
        }
        let Some(pivot) = self.pivot_in(&base, &targets) else { return };
        let apply: Box<dyn Fn(V3) -> V3> = match drag {
            Drag::Move { axis, free } => {
                let t = if self.snap { self.snapped_move(&base, &targets, axis, free) } else { free };
                let dir = edit::axis_vec(axis);
                Box::new(move |v| v.add(dir.scale(t)))
            }
            Drag::Scale { axis, free } => {
                let f = if self.snap { snap::snap_scale(free) } else { free };
                let k = edit::axis_vec(axis.min(2));
                if axis == 3 {
                    Box::new(move |v| pivot.add(v.sub(pivot).scale(f)))
                } else {
                    Box::new(move |v| {
                        let rel = v.sub(pivot);
                        pivot.add(rel.add(k.scale(k.dot(rel) * (f - 1.0))))
                    })
                }
            }
            Drag::Rotate { axis, free, .. } => {
                let a = if self.snap { snap::snap_angle(free) } else { free };
                let k = edit::axis_vec(axis);
                Box::new(move |v| edit::rotate_about(v, pivot, k, a))
            }
        };
        for (oi, verts) in &targets {
            let from = &base.objects[*oi].mesh;
            let to = &mut self.scene.objects[*oi].mesh;
            for &vi in verts {
                to.verts[vi as usize] = apply(from.verts[vi as usize]);
            }
        }
        self.rev += 1;
    }

    /// The translation to apply for a move dragged `free` along `axis`, snapped to the
    /// surfaces of the visible objects that are not moving, or to the grid.
    fn snapped_move(&self, base: &Scene, targets: &[(usize, std::collections::BTreeSet<u32>)], axis: usize, free: f32) -> f32 {
        let mut moved: Vec<V3> = Vec::new();
        let mut moved_faces: Vec<Vec<V3>> = Vec::new();
        for (oi, verts) in targets {
            let mesh = &base.objects[*oi].mesh;
            moved.extend(verts.iter().map(|&i| mesh.verts[i as usize]));
            moved_faces.extend(
                mesh.faces
                    .iter()
                    .filter(|f| f.iter().all(|i| verts.contains(i)))
                    .map(|f| f.iter().map(|&i| mesh.verts[i as usize]).collect()),
            );
        }
        let moving: std::collections::BTreeSet<usize> = targets.iter().map(|(i, _)| *i).collect();
        let others: Vec<&Mesh> = base
            .objects
            .iter()
            .enumerate()
            .filter(|(i, o)| !moving.contains(i) && o.visible)
            .map(|(_, o)| &o.mesh)
            .collect();
        let d = self.camera.distance;
        snap::snap_move(&moved, &moved_faces, &others, axis, free, d * 0.04, snap::grid_step(d))
    }

    pub fn set_snap(&mut self, on: bool) {
        self.snap = on;
    }

    // ------------------------------------------------------------------- export --

    /// Visible objects (only the selected ones if `selected_only`) as `.glb` (format 0) or
    /// `.obj` (format 1).
    pub fn export(&self, format: i32, selected_only: bool) -> Vec<u8> {
        let objects: Vec<&scene::Object> =
            self.scene.objects.iter().filter(|o| o.visible && (!selected_only || o.selected)).collect();
        if format == 1 {
            export::to_obj(&objects).into_bytes()
        } else {
            export::to_glb(&objects)
        }
    }

    // ------------------------------------------------------------------ presets --

    /// The visible objects as a work file (for saving a preset).
    pub fn visible_to_bytes(&self) -> Vec<u8> {
        let objects = self
            .scene
            .objects
            .iter()
            .filter(|o| o.visible)
            .map(|o| scene::Object { selected: false, group: 0, ..o.clone() })
            .collect();
        Scene { objects, active: 0 }.to_bytes(&self.camera)
    }

    /// Adds every object of a saved work (a preset) to this one, where they were saved.
    pub fn insert(&mut self, data: &[u8]) -> bool {
        let Some((incoming, _)) = Scene::from_bytes(data) else { return false };
        if incoming.objects.is_empty() {
            return false;
        }
        self.push_undo();
        self.scene.clear_selection();
        for o in incoming.objects {
            let i = self.scene.add(&o.name, o.mesh);
            self.scene.objects[i].visible = o.visible;
            self.scene.objects[i].selected = true;
        }
        // The pieces of a preset move together.
        self.scene.group_selected();
        self.sel = Selection::new(self.sel.mode);
        self.edit_mode = false;
        self.last_op = None;
        true
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

/// Orthographic (1) or perspective (0) view.
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_set_ortho(core: *mut Core, on: i32) {
    if let Some(c) = core.as_mut() {
        c.set_ortho(on != 0);
    }
}

/// Enters (`on` = 1) or leaves capture mode; `grid` / `edges` choose what the image shows.
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_set_capture(core: *mut Core, on: i32, grid: i32, edges: i32) {
    if let Some(c) = core.as_mut() {
        c.set_capture((on != 0).then_some(CaptureOpts { grid: grid != 0, edges: edges != 0 }));
    }
}

/// Applies a capture preset (0 diorama, 1 three-quarter, 2 front, 3 top, 4 side) and frames every
/// visible object inside the rect (x0, y0, x1, y1 as fractions of a view with `view_aspect`).
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_apply_preset(core: *mut Core, preset: i32, view_aspect: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> i32 {
    core.as_mut().map_or(0, |c| c.apply_preset(preset, view_aspect, [x0, y0, x1, y1]) as i32)
}

/// Zooms and centres (keeping the direction) so every visible object fits inside the rect.
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_frame_all(core: *mut Core, view_aspect: f32, x0: f32, y0: f32, x1: f32, y1: f32) {
    if let Some(c) = core.as_mut() {
        c.frame_all(view_aspect, [x0, y0, x1, y1]);
    }
}

/// Renders the framed part of the view into `out` (`w*h*4` RGBA bytes). Returns 1 on success.
///
/// # Safety
/// `core` must be live and `out` must point to at least `w*h*4` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn imonder_render_export(
    core: *const Core,
    w: u32,
    h: u32,
    scale: f32,
    view_aspect: f32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    transparent: i32,
    out: *mut u8,
) -> i32 {
    let (Some(c), false) = (core.as_ref(), out.is_null()) else { return 0 };
    if w == 0 || h == 0 || w > 8192 || h > 8192 || !(x1 > x0 && y1 > y0) {
        return 0;
    }
    let buf = std::slice::from_raw_parts_mut(out, w as usize * h as usize * 4);
    c.render_export(w as usize, h as usize, scale, view_aspect, [x0, y0, x1, y1], transparent != 0, buf);
    1
}

/// Snapping on (1) or off (0).
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_set_snap(core: *mut Core, on: i32) {
    if let Some(c) = core.as_mut() {
        c.set_snap(on != 0);
    }
}

/// The visible objects as a work file, for saving a preset. Free with `imonder_free_bytes`.
///
/// # Safety
/// `core` must be live and `out_len` a writable `u32`.
#[no_mangle]
pub unsafe extern "C" fn imonder_visible_bytes(core: *const Core, out_len: *mut u32) -> *mut u8 {
    let bytes = core.as_ref().map_or_else(Vec::new, |c| c.visible_to_bytes());
    leak_bytes(bytes, out_len)
}

/// Adds the objects of a saved work (a preset) to the current one. Returns 1 on success.
///
/// # Safety
/// `core` must be live and `data` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn imonder_insert(core: *mut Core, data: *const u8, len: u32) -> i32 {
    let (Some(c), false) = (core.as_mut(), data.is_null()) else { return 0 };
    c.insert(std::slice::from_raw_parts(data, len as usize)) as i32
}

/// Adds the object (and its group) to the selection, or takes it out.
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_toggle_object(core: *mut Core, index: i32) -> i32 {
    core.as_mut().map_or(0, |c| (index >= 0 && c.toggle_object(index as usize)) as i32)
}

/// Selects every visible object.
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_select_all_objects(core: *mut Core) {
    if let Some(c) = core.as_mut() {
        c.select_all_objects();
    }
}

/// Groups (1) or ungroups (0) the selected objects. Returns 1 if anything changed.
///
/// # Safety
/// `core` must be a live pointer from `imonder_create`.
#[no_mangle]
pub unsafe extern "C" fn imonder_group_objects(core: *mut Core, on: i32) -> i32 {
    core.as_mut().map_or(0, |c| c.group_objects(on != 0) as i32)
}

/// Exports the visible objects (or only the selected ones) as glTF binary (0) or OBJ (1).
/// Free with `imonder_free_bytes`.
///
/// # Safety
/// `core` must be live and `out_len` a writable `u32`.
#[no_mangle]
pub unsafe extern "C" fn imonder_export(core: *const Core, format: i32, selected_only: i32, out_len: *mut u32) -> *mut u8 {
    let bytes = core.as_ref().map_or_else(Vec::new, |c| c.export(format, selected_only != 0));
    leak_bytes(bytes, out_len)
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
        c.select_object(0);
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
        assert!(!c.delete_object(), "nothing is selected after deleting");
        c.select_object(0);
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
    fn ortho_keeps_working_when_zoomed_into_the_model() {
        let mut c = front_core();
        c.set_ortho(true);
        c.camera.distance = 0.6; // a perspective camera would now sit inside the cube
        let mut buf = vec![0u8; 400 * 300 * 4];
        c.render(400, 300, 1.0, &mut buf);
        assert!(c.tap(0.5, 0.5, false));
        assert_eq!(c.sel.faces.iter().copied().collect::<Vec<_>>(), vec![2], "still picks the front face");
        assert_eq!(c.status() & 256, 256);
        // Parallel projection: the front face is a perfect square on screen.
        let view = c.view();
        let p: Vec<_> = c.mesh().faces[2].iter().map(|&v| view.project(c.mesh().verts[v as usize]).unwrap()).collect();
        let xs: Vec<f32> = p.iter().map(|q| q.0).collect();
        let ys: Vec<f32> = p.iter().map(|q| q.1).collect();
        let w = xs.iter().cloned().fold(f32::MIN, f32::max) - xs.iter().cloned().fold(f32::MAX, f32::min);
        let h = ys.iter().cloned().fold(f32::MIN, f32::max) - ys.iter().cloned().fold(f32::MAX, f32::min);
        assert!((w - h).abs() < 1e-3, "{w} vs {h}");
    }

    #[test]
    fn export_of_the_whole_view_matches_the_screen() {
        let mut c = object_core();
        c.set_capture(Some(CaptureOpts { grid: true, edges: true }));
        let (w, h) = (200, 150);
        let mut screen = vec![0u8; w * h * 4];
        let mut image = vec![0u8; w * h * 4];
        c.render(w, h, 1.0, &mut screen);
        c.render_export(w, h, 1.0, w as f32 / h as f32, [0.0, 0.0, 1.0, 1.0], false, &mut image);
        assert_eq!(screen, image);
    }

    #[test]
    fn export_of_a_square_frame_shows_the_frame_centre() {
        let mut c = object_core();
        c.set_capture(Some(CaptureOpts { grid: false, edges: false }));
        let (vw, vh) = (300usize, 600usize);
        let aspect = vw as f32 / vh as f32;
        let mut screen = vec![0u8; vw * vh * 4];
        c.render(vw, vh, 1.0, &mut screen);
        // A square frame centred on screen: 300 px wide = 0.5 of the 600 px height.
        let rect = [0.0, 0.25, 1.0, 0.75];
        let mut img = vec![0u8; 300 * 300 * 4];
        c.render_export(300, 300, 1.0, aspect, rect, false, &mut img);
        let px = |b: &[u8], w: usize, x: usize, y: usize| b[(y * w + x) * 4..][..3].to_vec();
        for (sx, sy, ix, iy) in [(150, 300, 150, 150), (60, 200, 60, 50), (240, 420, 240, 270)] {
            let (a, b) = (px(&screen, vw, sx, sy), px(&img, 300, ix, iy));
            let close = a.iter().zip(&b).all(|(p, q)| (*p as i32 - *q as i32).abs() <= 12);
            assert!(close, "screen ({sx},{sy}) {a:?} vs image ({ix},{iy}) {b:?}");
        }
    }

    #[test]
    fn transparent_export_has_alpha_only_where_the_model_is() {
        let mut c = object_core();
        c.set_capture(Some(CaptureOpts { grid: false, edges: true }));
        c.apply_preset(0, 1.0, [0.0, 0.0, 1.0, 1.0]);
        let mut img = vec![0u8; 128 * 128 * 4];
        c.render_export(128, 128, 1.0, 1.0, [0.0, 0.0, 1.0, 1.0], true, &mut img);
        assert_eq!(img[3], 0, "corner is see-through");
        assert_eq!(img[(64 * 128 + 64) * 4 + 3], 255, "the cube is opaque");
    }

    #[test]
    fn diorama_preset_frames_everything_inside_the_rect() {
        let mut c = object_core();
        c.add_object(4);
        for v in &mut c.scene.mesh_mut().unwrap().verts {
            v.x += 6.0;
            v.z += 2.0;
        }
        let aspect = 390.0 / 844.0;
        let rect = [0.05, 0.3, 0.95, 0.716]; // a square on a phone screen
        assert!(c.apply_preset(0, aspect, rect));
        assert!(c.camera.ortho);
        assert!((c.camera.pitch - 0.6155).abs() < 1e-3, "isometric angle");
        let m = math::M4::crop(rect).mul(&c.camera.proj(aspect)).mul(&c.camera.view());
        let mut max = 0.0f32;
        for o in &c.scene.objects {
            for &v in &o.mesh.verts {
                let p = m.apply(v);
                max = max.max((p[0] / p[3]).abs()).max((p[1] / p[3]).abs());
            }
        }
        assert!(max <= 0.83 && max > 0.75, "fills the frame without spilling over: {max}");
        // The model's centre sits in the middle of the frame, not of the screen.
        let (lo, hi) = c.visible_bounds().unwrap();
        let p = m.apply(lo.add(hi).scale(0.5));
        assert!((p[0] / p[3]).abs() < 0.02 && (p[1] / p[3]).abs() < 0.02, "centred: {p:?}");
        assert!(!c.apply_preset(42, aspect, rect));
    }

    #[test]
    fn capture_mode_disables_selection_and_handles() {
        let mut c = front_core();
        c.set_capture(Some(CaptureOpts { grid: true, edges: true }));
        assert!(!c.tap(0.5, 0.5, false));
        assert!(!c.drag_begin(0.5, 0.5));
        assert_eq!(c.status() & 512, 512);
        c.apply_preset(3, 1.0, [0.0, 0.0, 1.0, 1.0]);
        assert!(c.camera.ortho);
        c.set_capture(Some(CaptureOpts { grid: false, edges: true })); // changing options keeps the shot
        assert!(c.camera.ortho);
        c.set_capture(None);
        assert!(!c.camera.ortho && (c.camera.yaw + std::f32::consts::FRAC_PI_2).abs() < 1e-6, "editing view restored");
        assert!(c.tap(0.5, 0.5, false));
    }

    /// A cube with another one tucked 0.15 behind it, offset so the back cube's left edge
    /// (x = 0) runs right behind the front cube's face.
    fn tucked_scene(ortho: bool) -> (Core, Core) {
        let mut back = Core::new();
        back.add_object(0);
        for v in &mut back.scene.mesh_mut().unwrap().verts {
            v.x += 1.0;
            v.y += 0.15;
        }
        let mut alone = Core::new();
        for c in [&mut back, &mut alone] {
            c.camera.snap_axis(3);
            c.camera.target = math::v3(0.5, 0.0, 0.3); // slightly off-axis so the edge is not hidden by symmetry
            c.camera.yaw += 0.15;
            c.camera.pitch = 0.2;
            c.set_ortho(ortho);
            c.set_capture(Some(CaptureOpts { grid: false, edges: true }));
        }
        (back, alone)
    }

    #[test]
    fn hidden_edges_of_objects_behind_do_not_show_through() {
        for ortho in [true, false] {
            let (back, alone) = tucked_scene(ortho);
            let (w, h) = (400usize, 300usize);
            let (mut a, mut b) = (vec![0u8; w * h * 4], vec![0u8; w * h * 4]);
            back.render(w, h, 1.0, &mut a);
            alone.render(w, h, 1.0, &mut b);
            // Inside the front cube's face, the picture must not change because of the hidden cube.
            let view = alone.view();
            let mesh = alone.mesh();
            let face = &mesh.faces[2]; // -Y, facing the camera
            let pts: Vec<_> = face.iter().map(|&v| view.project(mesh.verts[v as usize]).unwrap()).collect();
            let (x0, x1) = pts.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
            let (y0, y1) = pts.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
            let to_px = |x: f32, y: f32| ((x / view.aspect * w as f32) as usize, (y * h as f32) as usize);
            let (px0, py0) = to_px(x0, y0);
            let (px1, py1) = to_px(x1, y1);
            let mut diff = 0;
            for y in py0 + 6..py1 - 6 {
                for x in px0 + 6..px1 - 6 {
                    let i = (y * w + x) * 4;
                    if (0..3).any(|c| (a[i + c] as i32 - b[i + c] as i32).abs() > 6) {
                        diff += 1;
                    }
                }
            }
            assert_eq!(diff, 0, "ortho={ortho}: {diff} pixels of hidden edges show through the front face");
        }
    }

    #[test]
    fn move_handles_end_in_arrowheads_and_scale_handles_in_squares() {
        let probe = |tool: Tool| {
            let mut c = object_core();
            c.camera.yaw = -0.7;
            c.camera.pitch = 0.5;
            c.set_tool(tool);
            let (w, h) = (400usize, 300usize);
            let mut buf = vec![0u8; w * h * 4];
            c.render(w, h, 1.0, &mut buf);
            let view = c.view();
            let (centre, len, _) = c.handle_geometry().unwrap();
            let a = view.project(centre).unwrap();
            let b = view.project(centre.add(edit::axis_vec(0).scale(len))).unwrap();
            let to_px = |p: (f32, f32)| (p.0 / view.aspect * w as f32, p.1 * h as f32);
            let (pa, pb) = (to_px((a.0, a.1)), to_px((b.0, b.1)));
            let (dx, dy) = (pb.0 - pa.0, pb.1 - pa.1);
            let l = (dx * dx + dy * dy).sqrt();
            // 12 px past the end of the X shaft: inside an arrowhead, outside a 6.5 px square.
            let (x, y) = ((pb.0 + dx / l * 12.0) as usize, (pb.1 + dy / l * 12.0) as usize);
            let i = (y * w + x) * 4;
            buf[i] > 200 && buf[i + 1] < 120
        };
        assert!(probe(Tool::Move), "red arrowhead beyond the X shaft");
        assert!(!probe(Tool::Scale), "the scale handle stops at its square");
    }

    /// A small cube hovering above a wide table; the cube is active.
    fn table_scene() -> Core {
        let mut c = object_core();
        for v in &mut c.scene.mesh_mut().unwrap().verts {
            v.x *= 4.0;
            v.y *= 4.0;
            v.z = v.z * 0.5 + 0.5; // table: z 0..1
        }
        c.add_object(0);
        for v in &mut c.scene.mesh_mut().unwrap().verts {
            v.x *= 0.5;
            v.y *= 0.5;
            v.z = v.z * 0.5 + 4.5; // cube: z 4..5
        }
        c
    }

    fn lowest_z(c: &Core) -> f32 {
        c.mesh().verts.iter().map(|v| v.z).fold(f32::MAX, f32::min)
    }

    #[test]
    fn snapping_drops_an_object_onto_another_and_undo_restores_it() {
        let mut c = table_scene();
        c.set_snap(true);
        c.begin_transform(Drag::Move { axis: 2, free: 0.0 });
        c.apply_transform(Drag::Move { axis: 2, free: -2.85 }); // bottom would be at 1.15
        assert!((lowest_z(&c) - 1.0).abs() < 1e-4, "rests on the table: {}", lowest_z(&c));
        c.apply_transform(Drag::Move { axis: 2, free: -1.0 }); // far from anything: grid
        let z = lowest_z(&c);
        assert!(((z / 0.1).round() * 0.1 - z).abs() < 1e-4, "on the 0.1 grid: {z}");
        c.drag_end();
        assert!(c.undo());
        assert!((lowest_z(&c) - 4.0).abs() < 1e-5);
        assert_eq!(c.status() & 1024, 1024, "undo does not change the snap setting");
    }

    #[test]
    fn without_snapping_the_drag_is_free() {
        let mut c = table_scene();
        c.begin_transform(Drag::Move { axis: 2, free: 0.0 });
        c.apply_transform(Drag::Move { axis: 2, free: -2.85 });
        assert!((lowest_z(&c) - 1.15).abs() < 1e-4);
    }

    #[test]
    fn snapping_rotation_and_scale_use_steps() {
        let mut c = table_scene();
        c.set_snap(true);
        assert_eq!(c.status() & 1024, 1024);
        c.begin_transform(Drag::Rotate { axis: 2, last: (0.0, 0.0), free: 0.0 });
        c.apply_transform(Drag::Rotate { axis: 2, last: (0.0, 0.0), free: 0.3 }); // ≈17° -> 15°
        let v = c.mesh().verts[1].sub(c.mesh().verts[0]); // the cube's bottom edge along +X
        let angle = v.y.atan2(v.x);
        assert!((angle - snap::ANGLE_STEP).abs() < 1e-4, "{angle}");
        c.drag_end();
        c.begin_transform(Drag::Scale { axis: 3, free: 1.0 });
        c.apply_transform(Drag::Scale { axis: 3, free: 1.47 });
        let (lo, hi) = c.mesh().bounds().unwrap();
        assert!(((hi.z - lo.z) - 1.5).abs() < 1e-4, "1.47x snaps to 1.5x: {}", hi.z - lo.z);
    }

    #[test]
    fn presets_save_visible_objects_and_insert_where_they_were() {
        let mut c = table_scene();
        c.add_object(4);
        c.set_visible(2, false); // the sphere stays out of the preset
        let preset = c.visible_to_bytes();
        let mut d = object_core();
        assert!(d.insert(&preset));
        let names: Vec<_> = d.scene.objects.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, ["立方体", "立方体.001", "立方体.002"]);
        assert_eq!(d.scene.objects[1].mesh, c.scene.objects[0].mesh, "same place as when saved");
        assert!(d.undo(), "inserting is one undo step");
        assert_eq!(d.scene.objects.len(), 1);
        assert!(!d.insert(b"junk"));
        assert_eq!(d.status() & 2, 2, "a failed insert records nothing (redo still there)");
    }

    /// Cube at the origin, a sphere to its right and a cone further right, front view.
    fn three_objects() -> Core {
        let mut c = object_core();
        c.camera.distance = 14.0;
        for (kind, dx) in [(4, 3.0), (3, 6.0)] {
            c.add_object(kind);
            for v in &mut c.scene.mesh_mut().unwrap().verts {
                v.x += dx;
            }
        }
        c.camera.target = math::v3(3.0, 0.0, 0.0);
        let mut buf = vec![0u8; 400 * 300 * 4];
        c.render(400, 300, 1.0, &mut buf);
        c
    }

    fn tap_object(c: &mut Core, i: usize, add: bool) -> bool {
        let view = c.view();
        let (lo, hi) = c.scene.objects[i].mesh.bounds().unwrap();
        let p = view.project(lo.add(hi).scale(0.5)).unwrap();
        c.tap(p.0 / view.aspect, p.1, add)
    }

    #[test]
    fn add_select_by_tap_then_move_all_selected_together() {
        let mut c = three_objects();
        assert!(tap_object(&mut c, 0, false));
        assert!(tap_object(&mut c, 2, true));
        assert_eq!(c.scene.selected_indices(), vec![0, 2]);
        assert_eq!(c.status() & 2048, 2048, "two or more selected");
        let before: Vec<_> = c.scene.objects.iter().map(|o| o.mesh.verts.clone()).collect();
        c.begin_transform(Drag::Move { axis: 2, free: 0.0 });
        c.apply_transform(Drag::Move { axis: 2, free: 1.5 });
        c.drag_end();
        for i in [0, 2] {
            assert!(c.scene.objects[i].mesh.verts.iter().zip(&before[i]).all(|(a, b)| (a.z - b.z - 1.5).abs() < 1e-5));
        }
        assert_eq!(c.scene.objects[1].mesh.verts, before[1], "the unselected sphere stays");
        // Tapping a selected object with "add" takes it out again.
        assert!(tap_object(&mut c, 2, true));
        assert_eq!(c.scene.selected_indices(), vec![0]);
        // Tapping empty space with "add" keeps the selection; without it clears it.
        assert!(!c.tap(0.01, 0.01, true));
        assert_eq!(c.scene.selected_indices(), vec![0]);
        assert!(!c.tap(0.01, 0.01, false));
        assert!(c.scene.selected_indices().is_empty());
    }

    #[test]
    fn rotation_of_a_selection_turns_around_its_common_centre() {
        let mut c = three_objects();
        c.select_all_objects();
        let (lo, hi) = c.scene.selection_bounds().unwrap();
        let centre = lo.add(hi).scale(0.5);
        c.begin_transform(Drag::Rotate { axis: 2, last: (0.0, 0.0), free: 0.0 });
        c.apply_transform(Drag::Rotate { axis: 2, last: (0.0, 0.0), free: std::f32::consts::PI });
        let (lo2, hi2) = c.scene.selection_bounds().unwrap();
        assert!(lo2.add(hi2).scale(0.5).sub(centre).len() < 1e-3, "half a turn keeps the centre");
        let (sl, sh) = c.scene.objects[1].mesh.bounds().unwrap(); // the sphere in the middle stays in the middle
        assert!((sl.add(sh).scale(0.5).x - centre.x).abs() < 1e-3);
    }

    #[test]
    fn groups_select_as_one_and_duplicate_and_delete_together() {
        let mut c = three_objects();
        c.select_object(0);
        c.toggle_object(1);
        assert!(c.group_objects(true));
        assert_eq!(c.status() & 4096, 4096);
        assert!(tap_object(&mut c, 2, false));
        assert!(tap_object(&mut c, 1, false));
        assert_eq!(c.scene.selected_indices(), vec![0, 1], "one tap selects the whole group");
        assert!(c.duplicate_object());
        assert_eq!(c.scene.objects.len(), 5);
        assert_eq!(c.scene.selected_indices(), vec![3, 4]);
        assert!(c.delete_object());
        assert_eq!(c.scene.objects.len(), 3);
        assert!(c.undo() && c.undo());
        assert_eq!(c.scene.objects.len(), 3);
        assert!(c.scene.objects.iter().all(|o| o.group == 1 || o.name == "円錐"), "back to grouped");
        c.select_object(0);
        assert!(c.group_objects(false));
        c.select_object(0);
        assert_eq!(c.scene.selected_indices(), vec![0]);
        assert!(!c.group_objects(false), "nothing to ungroup");
    }

    #[test]
    fn an_inserted_preset_arrives_selected_as_one_group() {
        let src = three_objects();
        let preset = src.visible_to_bytes();
        let mut c = object_core();
        assert!(c.insert(&preset));
        assert_eq!(c.scene.selected_indices(), vec![1, 2, 3]);
        let g = c.scene.objects[1].group;
        assert!(g != 0 && c.scene.objects[1..].iter().all(|o| o.group == g));
        assert_eq!(c.scene.objects[0].group, 0);
    }

    #[test]
    fn selection_and_groups_survive_saving() {
        let mut c = three_objects();
        c.select_object(0);
        c.toggle_object(2);
        c.group_objects(true);
        let mut d = Core::new();
        assert!(d.load(&c.to_bytes()));
        assert_eq!(d.scene.selected_indices(), vec![0, 2]);
        assert_eq!(d.scene.objects.iter().map(|o| o.group).collect::<Vec<_>>(), vec![1, 0, 1]);
    }

    #[test]
    fn export_respects_visibility_and_selection() {
        let mut c = three_objects();
        c.set_visible(1, false);
        c.select_object(2);
        let all = String::from_utf8(c.export(1, false)).unwrap();
        assert!(all.contains("o 立方体") && !all.contains("o 球") && all.contains("o 円錐"));
        let sel = String::from_utf8(c.export(1, true)).unwrap();
        assert!(!sel.contains("o 立方体") && sel.contains("o 円錐"));
        assert_eq!(&c.export(0, false)[0..4], b"glTF");
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
