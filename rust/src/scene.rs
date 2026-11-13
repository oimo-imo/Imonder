//! The scene: a list of objects (each a world-space mesh), the active one, and the file format.

use crate::camera::Camera;
use crate::math::{v3, V3};
use crate::mesh::Mesh;

#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    pub name: String,
    pub mesh: Mesh,
    pub visible: bool,
    /// Part of the object selection (several objects can be selected at once).
    pub selected: bool,
    /// Objects sharing a non-zero group id are selected together.
    pub group: u32,
}

impl Object {
    pub fn new(name: String, mesh: Mesh) -> Object {
        Object { name, mesh, visible: true, selected: false, group: 0 }
    }
}

#[derive(Clone, Debug)]
pub struct Scene {
    pub objects: Vec<Object>,
    pub active: usize,
}

static EMPTY: Mesh = Mesh { verts: Vec::new(), faces: Vec::new() };

impl Scene {
    /// A fresh work: one cube.
    pub fn starter() -> Scene {
        let mut cube = Object::new("立方体".into(), Mesh::cube(1.0));
        cube.selected = true;
        Scene { objects: vec![cube], active: 0 }
    }

    pub fn has_active(&self) -> bool {
        self.active < self.objects.len()
    }

    /// The active object's mesh (empty when there is none).
    pub fn mesh(&self) -> &Mesh {
        self.objects.get(self.active).map_or(&EMPTY, |o| &o.mesh)
    }

    pub fn mesh_mut(&mut self) -> Option<&mut Mesh> {
        self.objects.get_mut(self.active).map(|o| &mut o.mesh)
    }

    pub fn set_mesh(&mut self, mesh: Mesh) {
        if let Some(o) = self.objects.get_mut(self.active) {
            o.mesh = mesh;
        }
    }

    /// "立方体", "立方体.001", "立方体.002", ...
    pub fn unique_name(&self, base: &str) -> String {
        let base = base.split('.').next().unwrap_or(base);
        if !self.objects.iter().any(|o| o.name == base) {
            return base.to_string();
        }
        (1..)
            .map(|i| format!("{base}.{i:03}"))
            .find(|n| !self.objects.iter().any(|o| &o.name == n))
            .unwrap()
    }

    /// Adds an object and makes it active (the selection is up to the caller).
    pub fn add(&mut self, name: &str, mesh: Mesh) -> usize {
        let name = self.unique_name(name);
        self.objects.push(Object::new(name, mesh));
        self.active = self.objects.len() - 1;
        self.active
    }

    // ---------------------------------------------------------------- selection --

    pub fn selected_indices(&self) -> Vec<usize> {
        (0..self.objects.len()).filter(|&i| self.objects[i].selected).collect()
    }

    pub fn clear_selection(&mut self) {
        for o in &mut self.objects {
            o.selected = false;
        }
        self.active = self.objects.len();
    }

    fn set_with_group(&mut self, i: usize, on: bool) {
        let group = self.objects[i].group;
        for (k, o) in self.objects.iter_mut().enumerate() {
            if k == i || (group != 0 && o.group == group) {
                o.selected = on;
            }
        }
    }

    /// Selects just this object (and the rest of its group) and makes it active.
    pub fn select_only(&mut self, i: usize) -> bool {
        if i >= self.objects.len() {
            return false;
        }
        self.clear_selection();
        self.set_with_group(i, true);
        self.active = i;
        true
    }

    /// Adds this object (and its group) to the selection, or takes it out if it was in.
    pub fn toggle(&mut self, i: usize) -> bool {
        if i >= self.objects.len() {
            return false;
        }
        let on = !self.objects[i].selected;
        self.set_with_group(i, on);
        if on {
            self.active = i;
        } else if !self.has_active() || !self.objects[self.active].selected {
            self.active = self.selected_indices().last().copied().unwrap_or(self.objects.len());
        }
        true
    }

    pub fn select_all_visible(&mut self) {
        for o in &mut self.objects {
            o.selected = o.visible;
        }
        if !self.has_active() || !self.objects[self.active].selected {
            self.active = self.selected_indices().first().copied().unwrap_or(self.objects.len());
        }
    }

    /// Puts the selected objects into a new group. Needs at least two.
    pub fn group_selected(&mut self) -> bool {
        let sel = self.selected_indices();
        if sel.len() < 2 {
            return false;
        }
        let id = self.objects.iter().map(|o| o.group).max().unwrap_or(0) + 1;
        for i in sel {
            self.objects[i].group = id;
        }
        true
    }

    pub fn ungroup_selected(&mut self) -> bool {
        let mut any = false;
        for o in self.objects.iter_mut().filter(|o| o.selected && o.group != 0) {
            o.group = 0;
            any = true;
        }
        any
    }

    /// Removes every selected object.
    pub fn remove_selected(&mut self) -> bool {
        let before = self.objects.len();
        self.objects.retain(|o| !o.selected);
        self.active = self.objects.len();
        self.objects.len() != before
    }

    /// Copies the selected objects in exactly the same place; the copies become the selection.
    /// Copied groups become new groups.
    pub fn duplicate_selected(&mut self) -> bool {
        let sel = self.selected_indices();
        if sel.is_empty() {
            return false;
        }
        let was_active = self.active;
        let mut next_group = self.objects.iter().map(|o| o.group).max().unwrap_or(0) + 1;
        let mut remap = std::collections::BTreeMap::new();
        let mut new_active = None;
        for &i in &sel {
            let src = self.objects[i].clone();
            self.objects[i].selected = false;
            let k = self.add(&src.name, src.mesh);
            self.objects[k].visible = src.visible;
            self.objects[k].selected = true;
            if src.group != 0 {
                self.objects[k].group = *remap.entry(src.group).or_insert_with(|| {
                    next_group += 1;
                    next_group - 1
                });
            }
            if i == was_active {
                new_active = Some(k);
            }
        }
        self.active = new_active.unwrap_or(self.objects.len() - 1);
        true
    }

    /// Bounds of the selected objects.
    pub fn selection_bounds(&self) -> Option<(V3, V3)> {
        let mut out: Option<(V3, V3)> = None;
        for o in self.objects.iter().filter(|o| o.selected) {
            if let Some((a, b)) = o.mesh.bounds() {
                out = Some(match out {
                    None => (a, b),
                    Some((lo, hi)) => (
                        v3(lo.x.min(a.x), lo.y.min(a.y), lo.z.min(a.z)),
                        v3(hi.x.max(b.x), hi.y.max(b.y), hi.z.max(b.z)),
                    ),
                });
            }
        }
        out
    }

    pub fn bounds_centre(&self) -> Option<V3> {
        let (lo, hi) = self.mesh().bounds()?;
        Some(lo.add(hi).scale(0.5))
    }

    /// `[{"name":"..","visible":true,"active":false,"selected":false,"group":0}, ...]`
    pub fn to_json(&self) -> String {
        let items: Vec<String> = self
            .objects
            .iter()
            .enumerate()
            .map(|(i, o)| {
                format!(
                    "{{\"name\":{},\"visible\":{},\"active\":{},\"selected\":{},\"group\":{}}}",
                    json_string(&o.name),
                    o.visible,
                    i == self.active,
                    o.selected,
                    o.group
                )
            })
            .collect();
        format!("[{}]", items.join(","))
    }

    // ------------------------------------------------------------------ file format --

    const MAGIC: &'static [u8; 4] = b"IMND";
    /// 2 added the selected flag and group id per object; version 1 files still open.
    const VERSION: u32 = 2;

    /// Serialises the scene and the camera into the `.imnd` binary format.
    pub fn to_bytes(&self, cam: &Camera) -> Vec<u8> {
        let mut w = Vec::new();
        w.extend_from_slice(Self::MAGIC);
        put_u32(&mut w, Self::VERSION);
        for f in [cam.yaw, cam.pitch, cam.distance, cam.target.x, cam.target.y, cam.target.z] {
            put_f32(&mut w, f);
        }
        put_u32(&mut w, self.active.min(self.objects.len().saturating_sub(1)) as u32);
        put_u32(&mut w, self.objects.len() as u32);
        for o in &self.objects {
            put_u32(&mut w, o.name.len() as u32);
            w.extend_from_slice(o.name.as_bytes());
            w.push(o.visible as u8);
            w.push(o.selected as u8);
            put_u32(&mut w, o.group);
            put_u32(&mut w, o.mesh.verts.len() as u32);
            for v in &o.mesh.verts {
                put_f32(&mut w, v.x);
                put_f32(&mut w, v.y);
                put_f32(&mut w, v.z);
            }
            put_u32(&mut w, o.mesh.faces.len() as u32);
            for f in &o.mesh.faces {
                put_u32(&mut w, f.len() as u32);
                for &i in f {
                    put_u32(&mut w, i);
                }
            }
        }
        w
    }

    /// Parses a file. Returns `None` for anything that is not a valid, in-range `.imnd`.
    pub fn from_bytes(data: &[u8]) -> Option<(Scene, Camera)> {
        let mut r = Reader { d: data, p: 0 };
        if r.take(4)? != Self::MAGIC {
            return None;
        }
        let version = r.u32()?;
        if version != 1 && version != Self::VERSION {
            return None;
        }
        let cam = Camera {
            yaw: r.f32()?,
            pitch: r.f32()?,
            distance: r.f32()?,
            target: v3(r.f32()?, r.f32()?, r.f32()?),
            ..Camera::default()
        };
        let active = r.u32()? as usize;
        let count = r.u32()? as usize;
        let mut objects = Vec::new();
        for _ in 0..count {
            let name_len = r.u32()? as usize;
            let name = String::from_utf8(r.take(name_len)?.to_vec()).ok()?;
            let visible = r.take(1)?[0] != 0;
            let (selected, group) = if version >= 2 { (r.take(1)?[0] != 0, r.u32()?) } else { (false, 0) };
            let nv = r.u32()? as usize;
            let mut verts = Vec::new();
            for _ in 0..nv {
                verts.push(v3(r.f32()?, r.f32()?, r.f32()?));
            }
            let nf = r.u32()? as usize;
            let mut faces = Vec::new();
            for _ in 0..nf {
                let n = r.u32()? as usize;
                let mut f = Vec::new();
                for _ in 0..n {
                    let i = r.u32()?;
                    if i as usize >= nv {
                        return None;
                    }
                    f.push(i);
                }
                if f.len() < 3 {
                    return None;
                }
                faces.push(f);
            }
            objects.push(Object { name, mesh: Mesh { verts, faces }, visible, selected, group });
        }
        if r.p != data.len() || (active >= objects.len() && !objects.is_empty()) {
            return None;
        }
        Some((Scene { objects, active }, cam))
    }
}

fn put_u32(w: &mut Vec<u8>, v: u32) {
    w.extend_from_slice(&v.to_le_bytes());
}

fn put_f32(w: &mut Vec<u8>, v: f32) {
    w.extend_from_slice(&v.to_le_bytes());
}

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.p.checked_add(n)?;
        let s = self.d.get(self.p..end)?;
        self.p = end;
        Some(s)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        let v = f32::from_le_bytes(self.take(4)?.try_into().ok()?);
        v.is_finite().then_some(v)
    }
}

fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Scene {
        let mut s = Scene::starter();
        s.add("球", Mesh::sphere(1.0, 8, 4));
        s.objects[1].visible = false;
        s.active = 0;
        s
    }

    #[test]
    fn roundtrip_keeps_everything() {
        let s = sample();
        let cam = Camera { yaw: 1.25, pitch: -0.3, distance: 9.0, target: v3(1.0, 2.0, 3.0), ..Camera::default() };
        let bytes = s.to_bytes(&cam);
        let (s2, cam2) = Scene::from_bytes(&bytes).unwrap();
        assert_eq!(s2.objects, s.objects);
        assert_eq!(s2.active, 0);
        assert_eq!((cam2.yaw, cam2.pitch, cam2.distance, cam2.target), (1.25, -0.3, 9.0, v3(1.0, 2.0, 3.0)));
    }

    #[test]
    fn corrupt_files_are_rejected_not_panicked_on() {
        let bytes = sample().to_bytes(&Camera::default());
        for cut in 0..bytes.len() {
            assert!(Scene::from_bytes(&bytes[..cut]).is_none(), "prefix {cut}");
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(Scene::from_bytes(&extra).is_none());
        let mut bad = bytes.clone();
        bad[0] = b'X';
        assert!(Scene::from_bytes(&bad).is_none());
        // A face index pointing past the vertex list.
        let mut oob = Scene::starter();
        oob.objects[0].mesh.faces[0][0] = 99;
        assert!(Scene::from_bytes(&oob.to_bytes(&Camera::default())).is_none());
        // Garbage must never panic.
        for seed in 0..200u32 {
            let junk: Vec<u8> = (0..64).map(|i| (seed.wrapping_mul(2654435761).wrapping_add(i * 97) >> 8) as u8).collect();
            let _ = Scene::from_bytes(&junk);
        }
    }

    #[test]
    fn empty_scene_roundtrips() {
        let s = Scene { objects: vec![], active: 0 };
        let (s2, _) = Scene::from_bytes(&s.to_bytes(&Camera::default())).unwrap();
        assert!(s2.objects.is_empty() && s2.mesh().verts.is_empty() && !s2.has_active());
    }

    #[test]
    fn names_stay_unique_and_remove_keeps_active_valid() {
        let mut s = Scene::starter();
        s.add("立方体", Mesh::cube(1.0));
        s.add("立方体", Mesh::cube(1.0));
        let names: Vec<_> = s.objects.iter().map(|o| o.name.clone()).collect();
        assert_eq!(names, ["立方体", "立方体.001", "立方体.002"]);
        s.select_only(0);
        assert!(s.duplicate_selected());
        assert_eq!(s.objects.last().unwrap().name, "立方体.003");
        assert_eq!(s.objects.last().unwrap().mesh, s.objects[0].mesh, "the copy sits exactly on the original");
        assert_eq!(s.selected_indices(), vec![3], "the copy is selected, the original is not");
        s.select_all_visible();
        assert!(s.remove_selected());
        assert!(!s.has_active() && s.objects.is_empty());
        assert!(!s.remove_selected());
        assert!(!s.duplicate_selected());
    }

    #[test]
    fn groups_select_together_and_copies_get_their_own_group() {
        let mut s = Scene::starter();
        s.add("球", Mesh::sphere(1.0, 8, 4));
        s.add("円柱", Mesh::cylinder(1.0, 1.0, 8));
        s.select_only(0);
        assert!(!s.group_selected(), "a group needs two objects");
        s.toggle(1);
        assert_eq!(s.selected_indices(), vec![0, 1]);
        assert!(s.group_selected());
        s.select_only(2);
        s.select_only(1);
        assert_eq!(s.selected_indices(), vec![0, 1], "tapping one member selects the group");
        s.toggle(0);
        assert!(s.selected_indices().is_empty(), "toggling a member toggles the group");
        assert!(!s.has_active());
        s.select_only(0);
        assert!(s.duplicate_selected());
        let groups: Vec<u32> = s.objects.iter().map(|o| o.group).collect();
        assert_eq!(groups, vec![1, 1, 0, 2, 2], "the copies form a new group");
        assert_eq!(s.selected_indices(), vec![3, 4]);
        assert!(s.ungroup_selected());
        s.select_only(3);
        assert_eq!(s.selected_indices(), vec![3]);
    }

    #[test]
    fn version_1_files_still_open() {
        // The original format: no selected flag or group id per object.
        let mut w = Vec::new();
        w.extend_from_slice(b"IMND");
        put_u32(&mut w, 1);
        for f in [-0.7f32, 0.5, 7.0, 0.0, 0.0, 0.0] {
            put_f32(&mut w, f);
        }
        put_u32(&mut w, 0);
        put_u32(&mut w, 1);
        let name = "立方体".as_bytes();
        put_u32(&mut w, name.len() as u32);
        w.extend_from_slice(name);
        w.push(1);
        let m = Mesh::cube(1.0);
        put_u32(&mut w, m.verts.len() as u32);
        for v in &m.verts {
            put_f32(&mut w, v.x);
            put_f32(&mut w, v.y);
            put_f32(&mut w, v.z);
        }
        put_u32(&mut w, m.faces.len() as u32);
        for f in &m.faces {
            put_u32(&mut w, f.len() as u32);
            for &i in f {
                put_u32(&mut w, i);
            }
        }
        let (s, _) = Scene::from_bytes(&w).unwrap();
        assert_eq!(s.objects[0].mesh, m);
        assert_eq!((s.objects[0].selected, s.objects[0].group), (false, 0));
        let mut future = w.clone();
        future[4] = 3;
        assert!(Scene::from_bytes(&future).is_none(), "unknown versions are refused");
    }

    #[test]
    fn json_escapes() {
        let mut s = Scene::starter();
        s.objects[0].name = "a\"b\\c".into();
        assert_eq!(s.to_json(), r#"[{"name":"a\"b\\c","visible":true,"active":true,"selected":true,"group":0}]"#);
    }
}
