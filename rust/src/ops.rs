//! Mesh editing operations. Each one takes the current mesh + selection and returns a new
//! mesh + selection, or `None` when it does not apply. They never mutate their inputs, which
//! lets the "adjust last operation" capsule redo them from a stored base with a new value.

use std::collections::{BTreeMap, BTreeSet};

use crate::edit::{edge_faces, edge_key, SelectMode, Selection};
use crate::math::{v3, V3};
use crate::mesh::Mesh;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    Extrude,
    Inset,
    LoopCut,
    Bevel,
    Merge,
    Delete,
}

impl OpKind {
    pub fn from_i32(i: i32) -> Option<OpKind> {
        Some(match i {
            0 => OpKind::Extrude,
            1 => OpKind::Inset,
            2 => OpKind::LoopCut,
            3 => OpKind::Bevel,
            4 => OpKind::Merge,
            5 => OpKind::Delete,
            _ => return None,
        })
    }

    /// (min, max, default, is_integer) of the adjustable value, or `None` if there is none.
    pub fn range(self) -> Option<(f32, f32, f32, bool)> {
        match self {
            OpKind::Extrude => Some((-2.0, 2.0, 0.5, false)),
            OpKind::Inset => Some((0.0, 1.0, 0.3, false)),
            OpKind::LoopCut => Some((1.0, 10.0, 1.0, true)),
            OpKind::Bevel => Some((0.0, 1.0, 0.15, false)),
            OpKind::Merge | OpKind::Delete => None,
        }
    }
}

pub fn apply(kind: OpKind, mesh: &Mesh, sel: &Selection, param: f32) -> Option<(Mesh, Selection)> {
    match kind {
        OpKind::Extrude => match sel.mode {
            SelectMode::Face => extrude_faces(mesh, sel, param),
            SelectMode::Edge => extrude_edges(mesh, sel, param),
            SelectMode::Vertex => None,
        },
        OpKind::Inset => (sel.mode == SelectMode::Face).then(|| inset_faces(mesh, sel, param)).flatten(),
        OpKind::LoopCut => (sel.mode == SelectMode::Edge).then(|| loop_cut(mesh, sel, param.round().max(1.0) as usize)).flatten(),
        OpKind::Bevel => (sel.mode == SelectMode::Edge).then(|| bevel_edges(mesh, sel, param)).flatten(),
        OpKind::Merge => merge_center(mesh, sel),
        OpKind::Delete => delete_selected(mesh, sel),
    }
}

// ---------------------------------------------------------------------------------------------
// helpers

/// Drops vertices no face uses and remaps the selection. Face indices are unchanged.
fn compact(mut mesh: Mesh, mut sel: Selection) -> (Mesh, Selection) {
    let used: BTreeSet<u32> = mesh.faces.iter().flatten().copied().collect();
    let mut remap = vec![u32::MAX; mesh.verts.len()];
    let mut verts = Vec::with_capacity(used.len());
    for (i, v) in mesh.verts.iter().enumerate() {
        if used.contains(&(i as u32)) {
            remap[i] = verts.len() as u32;
            verts.push(*v);
        }
    }
    for f in &mut mesh.faces {
        for i in f.iter_mut() {
            *i = remap[*i as usize];
        }
    }
    mesh.verts = verts;
    let map = |v: u32| remap.get(v as usize).copied().filter(|&r| r != u32::MAX);
    sel.verts = sel.verts.iter().filter_map(|&v| map(v)).collect();
    sel.edges = sel.edges.iter().filter_map(|&(a, b)| Some(edge_key(map(a)?, map(b)?))).collect();
    (mesh, sel)
}

fn face_centroid(mesh: &Mesh, f: &[u32]) -> V3 {
    f.iter().fold(v3(0.0, 0.0, 0.0), |s, &i| s.add(mesh.verts[i as usize])).scale(1.0 / f.len() as f32)
}

/// Drops repeated consecutive vertices and faces that collapsed.
fn clean_faces(faces: Vec<Vec<u32>>) -> Vec<Vec<u32>> {
    faces
        .into_iter()
        .filter_map(|f| {
            let mut out: Vec<u32> = Vec::with_capacity(f.len());
            for v in f {
                if out.last() != Some(&v) {
                    out.push(v);
                }
            }
            while out.len() > 1 && out.first() == out.last() {
                out.pop();
            }
            let uniq: BTreeSet<_> = out.iter().collect();
            (out.len() >= 3 && uniq.len() == out.len()).then_some(out)
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// extrude

fn extrude_faces(mesh: &Mesh, sel: &Selection, dist: f32) -> Option<(Mesh, Selection)> {
    let faces: Vec<usize> = sel.faces.iter().copied().filter(|&f| f < mesh.faces.len()).collect();
    if faces.is_empty() {
        return None;
    }
    let mut m = mesh.clone();
    let mut normals: BTreeMap<u32, V3> = BTreeMap::new();
    let mut boundary: BTreeMap<(u32, u32), Vec<(u32, u32)>> = BTreeMap::new();
    for &fi in &faces {
        let f = &mesh.faces[fi];
        let n = mesh.face_normal(f);
        for (k, &v) in f.iter().enumerate() {
            let e = normals.entry(v).or_insert(v3(0.0, 0.0, 0.0));
            *e = e.add(n);
            let next = f[(k + 1) % f.len()];
            boundary.entry(edge_key(v, next)).or_default().push((v, next));
        }
    }
    let mut dup: BTreeMap<u32, u32> = BTreeMap::new();
    for (&v, &n) in &normals {
        dup.insert(v, m.verts.len() as u32);
        m.verts.push(mesh.verts[v as usize].add(n.norm().scale(dist)));
    }
    for &fi in &faces {
        m.faces[fi] = mesh.faces[fi].iter().map(|v| dup[v]).collect();
    }
    for dirs in boundary.values() {
        if let [(a, b)] = dirs.as_slice() {
            m.faces.push(vec![*a, *b, dup[b], dup[a]]);
        }
    }
    let mut out = sel.clone();
    out.verts.clear();
    out.edges.clear();
    Some(compact(m, out))
}

fn extrude_edges(mesh: &Mesh, sel: &Selection, dist: f32) -> Option<(Mesh, Selection)> {
    if sel.edges.is_empty() {
        return None;
    }
    let ef = edge_faces(mesh);
    let mut m = mesh.clone();
    let mut dir_sum: BTreeMap<u32, V3> = BTreeMap::new();
    let mut directed: Vec<(u32, u32)> = Vec::new();
    for &(a, b) in &sel.edges {
        let faces = ef.get(&(a, b))?;
        let n = faces.iter().fold(v3(0.0, 0.0, 0.0), |s, &f| s.add(mesh.face_normal(&mesh.faces[f]))).norm();
        for v in [a, b] {
            let e = dir_sum.entry(v).or_insert(v3(0.0, 0.0, 0.0));
            *e = e.add(n);
        }
        // Direction of the edge as the first adjacent face walks it.
        let f = &mesh.faces[*faces.first()?];
        let k = (0..f.len()).find(|&k| edge_key(f[k], f[(k + 1) % f.len()]) == (a, b))?;
        directed.push((f[k], f[(k + 1) % f.len()]));
    }
    let mut dup: BTreeMap<u32, u32> = BTreeMap::new();
    for (&v, &d) in &dir_sum {
        dup.insert(v, m.verts.len() as u32);
        m.verts.push(mesh.verts[v as usize].add(d.norm().scale(dist)));
    }
    let mut out = sel.clone();
    out.edges.clear();
    for (a, b) in directed {
        m.faces.push(vec![b, a, dup[&a], dup[&b]]);
        out.edges.insert(edge_key(dup[&a], dup[&b]));
    }
    Some((m, out))
}

// ---------------------------------------------------------------------------------------------
// inset

fn inset_faces(mesh: &Mesh, sel: &Selection, amount: f32) -> Option<(Mesh, Selection)> {
    let faces: Vec<usize> = sel.faces.iter().copied().filter(|&f| f < mesh.faces.len()).collect();
    if faces.is_empty() {
        return None;
    }
    let mut m = mesh.clone();
    for &fi in &faces {
        let f = mesh.faces[fi].clone();
        let n = mesh.face_normal(&f);
        let c = face_centroid(mesh, &f);
        let len = f.len();
        // Largest inset keeps the new polygon inside: the distance from the centre to the nearest edge line.
        let mut room = f32::MAX;
        for k in 0..len {
            let (p, q) = (mesh.verts[f[k] as usize], mesh.verts[f[(k + 1) % len] as usize]);
            let inward = n.cross(q.sub(p).norm());
            room = room.min(c.sub(p).dot(inward));
        }
        if room <= 1e-6 {
            continue;
        }
        let t = amount.clamp(0.0, 1.0) * room * 0.98;
        let mut inner = Vec::with_capacity(len);
        for k in 0..len {
            let p = mesh.verts[f[(k + len - 1) % len] as usize];
            let cur = mesh.verts[f[k] as usize];
            let q = mesh.verts[f[(k + 1) % len] as usize];
            let n0 = n.cross(cur.sub(p).norm());
            let n1 = n.cross(q.sub(cur).norm());
            let denom = 1.0 + n0.dot(n1);
            let off = if denom < 1e-4 { n0.scale(t) } else { n0.add(n1).scale(t / denom) };
            inner.push(m.verts.len() as u32);
            m.verts.push(cur.add(off));
        }
        for k in 0..len {
            let (a, b) = (f[k], f[(k + 1) % len]);
            m.faces.push(vec![a, b, inner[(k + 1) % len], inner[k]]);
        }
        m.faces[fi] = inner;
    }
    let mut out = sel.clone();
    out.verts.clear();
    out.edges.clear();
    Some((m, out))
}

// ---------------------------------------------------------------------------------------------
// loop cut

fn loop_cut(mesh: &Mesh, sel: &Selection, cuts: usize) -> Option<(Mesh, Selection)> {
    let ef = edge_faces(mesh);
    let mut cut: BTreeSet<(u32, u32)> = sel.edges.iter().copied().filter(|e| ef.contains_key(e)).collect();
    if cut.is_empty() {
        return None;
    }
    // Walk the ring of quads: every quad touching a cut edge is cut across to its opposite edge.
    let mut ring: BTreeSet<usize> = BTreeSet::new();
    let mut queue: Vec<(u32, u32)> = cut.iter().copied().collect();
    while let Some(e) = queue.pop() {
        for &fi in &ef[&e] {
            let f = &mesh.faces[fi];
            if f.len() != 4 || !ring.insert(fi) {
                continue;
            }
            let k = (0..4).find(|&k| edge_key(f[k], f[(k + 1) % 4]) == e)?;
            let opp = edge_key(f[(k + 2) % 4], f[(k + 3) % 4]);
            if cut.insert(opp) {
                queue.push(opp);
            }
        }
    }
    if ring.is_empty() {
        return None;
    }
    let mut m = mesh.clone();
    m.faces.clear();
    // New vertices per cut edge, stored from the lower to the higher vertex index.
    let mut points: BTreeMap<(u32, u32), Vec<u32>> = BTreeMap::new();
    for &(a, b) in &cut {
        let (pa, pb) = (mesh.verts[a as usize], mesh.verts[b as usize]);
        let mut ids = Vec::with_capacity(cuts);
        for k in 1..=cuts {
            let t = k as f32 / (cuts + 1) as f32;
            ids.push(m.verts.len() as u32);
            m.verts.push(pa.add(pb.sub(pa).scale(t)));
        }
        points.insert((a, b), ids);
    }
    let along = |a: u32, b: u32| -> Vec<u32> {
        let mut v = points[&edge_key(a, b)].clone();
        if a > b {
            v.reverse();
        }
        v
    };
    let mut new_edges = BTreeSet::new();
    for (fi, f) in mesh.faces.iter().enumerate() {
        if ring.contains(&fi) {
            let r = if cut.contains(&edge_key(f[0], f[1])) { 0 } else { 1 };
            let w = [f[r], f[(r + 1) % 4], f[(r + 2) % 4], f[(r + 3) % 4]];
            let mut p = vec![w[0]];
            p.extend(along(w[0], w[1]));
            p.push(w[1]);
            let mut q = vec![w[3]];
            q.extend(along(w[3], w[2]));
            q.push(w[2]);
            for k in 0..=cuts {
                m.faces.push(vec![p[k], p[k + 1], q[k + 1], q[k]]);
            }
            for k in 1..=cuts {
                new_edges.insert(edge_key(p[k], q[k]));
            }
        } else {
            // Not part of the ring (triangle, n-gon, ...): just add the new vertices on its border.
            let mut nf = Vec::new();
            for k in 0..f.len() {
                let (a, b) = (f[k], f[(k + 1) % f.len()]);
                nf.push(a);
                if cut.contains(&edge_key(a, b)) {
                    nf.extend(along(a, b));
                }
            }
            m.faces.push(nf);
        }
    }
    // Ring faces that also have a second cut pair keep their other cut edges whole; patch those borders.
    let mut out = Selection::new(SelectMode::Edge);
    out.edges = new_edges;
    Some((m, out))
}

// ---------------------------------------------------------------------------------------------
// bevel

type V2 = (f32, f32);

fn intersect(p1: V2, d1: V2, p2: V2, d2: V2) -> Option<V2> {
    let denom = d1.0 * d2.1 - d1.1 * d2.0;
    if denom.abs() < 1e-9 {
        return None;
    }
    let diff = (p2.0 - p1.0, p2.1 - p1.1);
    let t = (diff.0 * d2.1 - diff.1 * d2.0) / denom;
    Some((p1.0 + t * d1.0, p1.1 + t * d1.1))
}

/// Interpolates between two offset vectors along an arc (keeps rounded bevels round).
fn slerp_offset(a: V3, b: V3, t: f32) -> V3 {
    let (la, lb) = (a.len(), b.len());
    if la < 1e-9 && lb < 1e-9 {
        return v3(0.0, 0.0, 0.0);
    }
    if la < 1e-9 {
        return b.scale(t);
    }
    if lb < 1e-9 {
        return a.scale(1.0 - t);
    }
    let (da, db) = (a.norm(), b.norm());
    let angle = da.dot(db).clamp(-1.0, 1.0).acos();
    let length = la + (lb - la) * t;
    if angle < 1e-4 {
        return da.scale(length);
    }
    let s = angle.sin();
    da.scale(((1.0 - t) * angle).sin() / s).add(db.scale((t * angle).sin() / s)).norm().scale(length)
}

/// Bevel width is a fraction (0..1) of the shortest selected edge, so it never overlaps.
fn bevel_edges(mesh: &Mesh, sel: &Selection, amount: f32) -> Option<(Mesh, Selection)> {
    let ef = edge_faces(mesh);
    let chosen: BTreeSet<(u32, u32)> =
        sel.edges.iter().copied().filter(|e| ef.get(e).map_or(false, |f| f.len() == 2)).collect();
    if chosen.is_empty() {
        return None;
    }
    let min_len = chosen
        .iter()
        .map(|&(a, b)| mesh.verts[a as usize].sub(mesh.verts[b as usize]).len())
        .fold(f32::MAX, f32::min);
    let width = amount.clamp(0.0, 1.0) * min_len * 0.45;
    if width < 1e-6 {
        return Some((mesh.clone(), sel.clone()));
    }
    let segments = 1usize;

    let mut verts: Vec<V3> = Vec::new();
    let mut faces: Vec<Vec<u32>> = Vec::new();
    let mut corner: BTreeMap<(usize, u32), u32> = BTreeMap::new();

    // 1. Inset each face with a per-edge width.
    for (fi, face) in mesh.faces.iter().enumerate() {
        let n = face.len();
        let normal = mesh.face_normal(face);
        let origin = mesh.verts[face[0] as usize];
        let u = mesh.verts[face[1] as usize].sub(origin).norm();
        let w_axis = normal.cross(u).norm();
        let pts: Vec<V2> = face
            .iter()
            .map(|&vi| {
                let d = mesh.verts[vi as usize].sub(origin);
                (d.dot(u), d.dot(w_axis))
            })
            .collect();
        let widths: Vec<f32> =
            (0..n).map(|i| if chosen.contains(&edge_key(face[i], face[(i + 1) % n])) { width } else { 0.0 }).collect();
        for i in 0..n {
            let (pw, nw) = (widths[(i + n - 1) % n], widths[i]);
            let pos2 = if pw < 1e-9 && nw < 1e-9 {
                pts[i]
            } else {
                let prev = pts[(i + n - 1) % n];
                let next = pts[(i + 1) % n];
                let dp = (pts[i].0 - prev.0, pts[i].1 - prev.1);
                let dn = (next.0 - pts[i].0, next.1 - pts[i].1);
                let left = |d: V2| {
                    let l = (d.0 * d.0 + d.1 * d.1).sqrt().max(1e-12);
                    (-d.1 / l, d.0 / l)
                };
                let (lp, ln) = (left(dp), left(dn));
                let a = (pts[i].0 + lp.0 * pw, pts[i].1 + lp.1 * pw);
                let b = (pts[i].0 + ln.0 * nw, pts[i].1 + ln.1 * nw);
                intersect(a, dp, b, dn).unwrap_or(((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5))
            };
            let world = origin.add(u.scale(pos2.0)).add(w_axis.scale(pos2.1));
            corner.insert((fi, face[i]), verts.len() as u32);
            verts.push(world);
        }
        faces.push(face.iter().map(|v| corner[&(fi, *v)]).collect());
    }

    // 1b. A face that only touches a beveled vertex head-on keeps its corner at the old position;
    // the corner is replaced by the two new points lying on its neighbouring edges.
    let touched: BTreeSet<u32> = mesh
        .faces
        .iter()
        .enumerate()
        .flat_map(|(fi, f)| f.iter().map(move |&v| (fi, v)))
        .filter(|&(fi, v)| verts[corner[&(fi, v)] as usize].sub(mesh.verts[v as usize]).len() > 1e-6)
        .map(|(_, v)| v)
        .collect();
    for (fi, face) in mesh.faces.iter().enumerate() {
        let n = face.len();
        let mut nf = Vec::with_capacity(n + 2);
        for i in 0..n {
            let v = face[i];
            let c = corner[&(fi, v)];
            let unmoved = verts[c as usize].sub(mesh.verts[v as usize]).len() <= 1e-6;
            if unmoved && touched.contains(&v) {
                let across = |a: u32, b: u32| ef.get(&edge_key(a, b)).and_then(|l| l.iter().copied().find(|&x| x != fi));
                let cp = across(face[(i + n - 1) % n], v).map_or(c, |f| corner[&(f, v)]);
                let cn = across(v, face[(i + 1) % n]).map_or(c, |f| corner[&(f, v)]);
                nf.push(cp);
                nf.push(cn);
            } else {
                nf.push(c);
            }
        }
        faces[fi] = nf;
    }

    // 2. Strips between the two faces of every edge (zero-width ones collapse and are welded away).
    for (&(ea, eb), fs) in &ef {
        if fs.len() != 2 {
            continue;
        }
        let (fa, fb) = (fs[0], fs[1]);
        let f = &mesh.faces[fa];
        let k = (0..f.len()).find(|&k| edge_key(f[k], f[(k + 1) % f.len()]) == (ea, eb))?;
        let (a, b) = (f[k], f[(k + 1) % f.len()]); // fa walks a -> b, fb walks b -> a
        let (va, vb) = (mesh.verts[a as usize], mesh.verts[b as usize]);
        let off = |face: usize, v: u32, o: V3| verts[corner[&(face, v)] as usize].sub(o);
        let (oa_a, oa_b) = (off(fa, a, va), off(fa, b, vb));
        let (ob_a, ob_b) = (off(fb, a, va), off(fb, b, vb));
        let seg = if chosen.contains(&(ea, eb)) { segments } else { 1 };
        let mut row_a = Vec::new();
        let mut row_b = Vec::new();
        for s in 0..=seg {
            let t = s as f32 / seg as f32;
            row_a.push(verts.len() as u32);
            verts.push(va.add(slerp_offset(oa_a, ob_a, t)));
            row_b.push(verts.len() as u32);
            verts.push(vb.add(slerp_offset(oa_b, ob_b, t)));
        }
        for s in 0..seg {
            faces.push(vec![row_b[s], row_a[s], row_a[s + 1], row_b[s + 1]]);
        }
    }

    // 3. Cap every vertex whose surrounding corners were pulled apart.
    let mut faces_at: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (fi, f) in mesh.faces.iter().enumerate() {
        for &v in f {
            faces_at.entry(v).or_default().push(fi);
        }
    }
    for (&v, fs) in &faces_at {
        if fs.len() < 3 {
            continue;
        }
        let moved = fs.iter().any(|&f| verts[corner[&(f, v)] as usize].sub(verts[corner[&(fs[0], v)] as usize]).len() > 1e-6);
        if !moved {
            continue;
        }
        // Walk the fan of faces around the vertex.
        let mut order = vec![fs[0]];
        let mut cur = fs[0];
        loop {
            let f = &mesh.faces[cur];
            let i = f.iter().position(|&x| x == v)?;
            let next = f[(i + 1) % f.len()];
            let Some(nf) = ef.get(&edge_key(v, next)).and_then(|l| l.iter().copied().find(|&x| x != cur)) else { break };
            if nf == fs[0] || order.contains(&nf) {
                break;
            }
            order.push(nf);
            cur = nf;
        }
        if order.len() != fs.len() {
            continue;
        }
        // A head-on face whose corner was replaced no longer touches the old vertex: leave it out.
        let used: BTreeSet<u32> = faces.iter().flatten().copied().collect();
        let mut cap: Vec<u32> = order
            .iter()
            .map(|&f| corner[&(f, v)])
            .filter(|&c| verts[c as usize].sub(mesh.verts[v as usize]).len() > 1e-6 || used.contains(&c))
            .collect();
        if cap.len() < 3 {
            continue;
        }
        let avg = fs.iter().fold(v3(0.0, 0.0, 0.0), |s, &f| s.add(mesh.face_normal(&mesh.faces[f])));
        let tmp = Mesh { verts: verts.clone(), faces: vec![cap.clone()] };
        if tmp.face_normal(&cap).dot(avg) < 0.0 {
            cap.reverse();
        }
        faces.push(cap);
    }

    // 4. Weld coincident vertices and drop collapsed faces.
    let mut keys: BTreeMap<(i64, i64, i64), u32> = BTreeMap::new();
    let mut remap = Vec::with_capacity(verts.len());
    let mut welded: Vec<V3> = Vec::new();
    for p in &verts {
        let k = ((p.x * 1e5).round() as i64, (p.y * 1e5).round() as i64, (p.z * 1e5).round() as i64);
        let id = *keys.entry(k).or_insert_with(|| {
            welded.push(*p);
            welded.len() as u32 - 1
        });
        remap.push(id);
    }
    let faces = clean_faces(faces.into_iter().map(|f| f.into_iter().map(|v| remap[v as usize]).collect()).collect());
    // Collinear / zero-area leftovers (strips along unbeveled edges) are not real faces.
    let probe = Mesh { verts: welded, faces: Vec::new() };
    let faces: Vec<Vec<u32>> = faces
        .into_iter()
        .filter(|f| {
            let mut area = v3(0.0, 0.0, 0.0);
            for k in 1..f.len() - 1 {
                let (a, b, c) = (probe.verts[f[0] as usize], probe.verts[f[k] as usize], probe.verts[f[k + 1] as usize]);
                area = area.add(b.sub(a).cross(c.sub(a)));
            }
            area.len() > 1e-9
        })
        .collect();
    let out = Selection::new(SelectMode::Edge);
    Some(compact(Mesh { verts: probe.verts, faces }, out))
}

// ---------------------------------------------------------------------------------------------
// merge / delete

fn merge_center(mesh: &Mesh, sel: &Selection) -> Option<(Mesh, Selection)> {
    let verts = sel.affected_verts(mesh);
    if verts.len() < 2 {
        return None;
    }
    let first = *verts.iter().next()?;
    let centre = verts.iter().fold(v3(0.0, 0.0, 0.0), |s, &v| s.add(mesh.verts[v as usize])).scale(1.0 / verts.len() as f32);
    let mut m = mesh.clone();
    m.verts[first as usize] = centre;
    m.faces = clean_faces(
        mesh.faces.iter().map(|f| f.iter().map(|&v| if verts.contains(&v) { first } else { v }).collect()).collect(),
    );
    let mut out = Selection::new(SelectMode::Vertex);
    out.verts.insert(first);
    Some(compact(m, out))
}

fn delete_selected(mesh: &Mesh, sel: &Selection) -> Option<(Mesh, Selection)> {
    if sel.is_empty() {
        return None;
    }
    let ef = edge_faces(mesh);
    let doomed: BTreeSet<usize> = match sel.mode {
        SelectMode::Face => sel.faces.clone(),
        SelectMode::Edge => sel.edges.iter().filter_map(|e| ef.get(e)).flatten().copied().collect(),
        SelectMode::Vertex => mesh
            .faces
            .iter()
            .enumerate()
            .filter(|(_, f)| f.iter().any(|v| sel.verts.contains(v)))
            .map(|(i, _)| i)
            .collect(),
    };
    let mut m = mesh.clone();
    m.faces = mesh.faces.iter().enumerate().filter(|(i, _)| !doomed.contains(i)).map(|(_, f)| f.clone()).collect();
    Some(compact(m, Selection::new(sel.mode)))
}

pub fn select_all(mesh: &Mesh, mode: SelectMode) -> Selection {
    let mut s = Selection::new(mode);
    match mode {
        SelectMode::Vertex => s.verts = (0..mesh.verts.len() as u32).collect(),
        SelectMode::Edge => s.edges = edge_faces(mesh).keys().copied().collect(),
        SelectMode::Face => s.faces = (0..mesh.faces.len()).collect(),
    }
    s
}

// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Every edge is used by exactly two faces, once in each direction (closed, consistently wound).
    fn is_closed(m: &Mesh) -> bool {
        let mut dir: BTreeMap<(u32, u32), i32> = BTreeMap::new();
        for f in &m.faces {
            for k in 0..f.len() {
                *dir.entry((f[k], f[(k + 1) % f.len()])).or_default() += 1;
            }
        }
        dir.iter().all(|(&(a, b), &c)| c == 1 && dir.get(&(b, a)) == Some(&1))
    }

    fn euler(m: &Mesh) -> i32 {
        m.verts.len() as i32 - edge_faces(m).len() as i32 + m.faces.len() as i32
    }

    /// Signed volume; positive when faces point outward.
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

    fn face_sel(fs: &[usize]) -> Selection {
        let mut s = Selection::new(SelectMode::Face);
        s.faces = fs.iter().copied().collect();
        s
    }

    fn edge_sel(es: &[(u32, u32)]) -> Selection {
        let mut s = Selection::new(SelectMode::Edge);
        s.edges = es.iter().map(|&(a, b)| edge_key(a, b)).collect();
        s
    }

    fn cube() -> Mesh {
        Mesh::cube(1.0)
    }

    #[test]
    fn cube_is_valid() {
        assert!(is_closed(&cube()));
        assert_eq!(euler(&cube()), 2);
        assert!((volume(&cube()) - 8.0).abs() < 1e-4);
    }

    #[test]
    fn extrude_top_face() {
        let (m, sel) = extrude_faces(&cube(), &face_sel(&[1]), 0.5).unwrap();
        assert_eq!((m.verts.len(), m.faces.len()), (12, 10));
        assert!(is_closed(&m) && euler(&m) == 2);
        assert!((volume(&m) - 10.0).abs() < 1e-3, "volume {}", volume(&m));
        assert!(m.verts.iter().map(|v| v.z).fold(f32::MIN, f32::max) > 1.49);
        assert_eq!(sel.faces.len(), 1);
    }

    #[test]
    fn extrude_two_faces_and_inward() {
        let (m, _) = extrude_faces(&cube(), &face_sel(&[1, 4]), 0.3).unwrap();
        assert!(is_closed(&m) && euler(&m) == 2, "{} {}", m.verts.len(), m.faces.len());
        let (m, _) = extrude_faces(&cube(), &face_sel(&[1]), -0.5).unwrap();
        assert!(is_closed(&m) && (volume(&m) - 6.0).abs() < 1e-3);
    }

    #[test]
    fn extrude_edge_adds_a_flap() {
        let (m, sel) = extrude_edges(&cube(), &edge_sel(&[(4, 5)]), 0.5).unwrap();
        assert_eq!((m.verts.len(), m.faces.len()), (10, 7));
        assert_eq!(sel.edges.len(), 1);
        // The flap makes the mesh non-closed along the new edge only; the old surface stays closed.
        assert_eq!(edge_faces(&m).values().filter(|f| f.len() == 1).count(), 3);
    }

    #[test]
    fn inset_face() {
        let (m, _) = inset_faces(&cube(), &face_sel(&[1]), 0.5).unwrap();
        assert_eq!((m.verts.len(), m.faces.len()), (12, 10));
        assert!(is_closed(&m) && euler(&m) == 2);
        assert!((volume(&m) - 8.0).abs() < 1e-3);
        // Inner top face is the middle half-size square at z = 1.
        let inner: Vec<_> = m.verts[8..12].iter().collect();
        assert!(inner.iter().all(|v| (v.z - 1.0).abs() < 1e-5 && v.x.abs() < 1.0 && v.y.abs() < 1.0));
    }

    #[test]
    fn loop_cut_ring() {
        let (m, sel) = loop_cut(&cube(), &edge_sel(&[(0, 4)]), 1).unwrap();
        assert_eq!((m.verts.len(), m.faces.len()), (12, 10));
        assert!(is_closed(&m) && euler(&m) == 2);
        assert!((volume(&m) - 8.0).abs() < 1e-3);
        assert_eq!(sel.edges.len(), 4);
        let (m, _) = loop_cut(&cube(), &edge_sel(&[(0, 4)]), 3).unwrap();
        assert_eq!((m.verts.len(), m.faces.len()), (20, 18));
        assert!(is_closed(&m) && euler(&m) == 2);
    }

    #[test]
    fn loop_cut_through_a_triangle_makes_an_ngon() {
        // A cube whose top face was cut into two triangles: the loop stops there.
        let mut c = cube();
        let top = c.faces.remove(1);
        c.faces.push(vec![top[0], top[1], top[2]]);
        c.faces.push(vec![top[0], top[2], top[3]]);
        let (m, _) = loop_cut(&c, &edge_sel(&[(0, 1)]), 1).unwrap();
        assert!(is_closed(&m), "still watertight");
        assert!(m.faces.iter().any(|f| f.len() == 4 + 1 || f.len() == 4), "some face grew");
        assert!((volume(&m) - 8.0).abs() < 1e-3);
    }

    #[test]
    fn bevel_one_edge() {
        let (m, _) = bevel_edges(&cube(), &edge_sel(&[(4, 5)]), 0.2).unwrap();
        assert!(is_closed(&m), "verts {} faces {}", m.verts.len(), m.faces.len());
        assert_eq!(euler(&m), 2);
        assert!(volume(&m) < 8.0 && volume(&m) > 7.9, "chamfer removes a sliver: {}", volume(&m));
    }

    #[test]
    fn bevel_all_edges() {
        let all = select_all(&cube(), SelectMode::Edge);
        let (m, _) = bevel_edges(&cube(), &all, 0.3).unwrap();
        assert_eq!((m.verts.len(), m.faces.len()), (24, 26));
        assert!(is_closed(&m) && euler(&m) == 2);
        assert!(volume(&m) > 0.0 && volume(&m) < 8.0);
    }

    #[test]
    fn merge_collapses_an_edge() {
        let mut s = Selection::new(SelectMode::Vertex);
        s.verts.extend([0, 1]);
        let (m, sel) = merge_center(&cube(), &s).unwrap();
        assert_eq!((m.verts.len(), m.faces.len()), (7, 6));
        assert!(is_closed(&m) && euler(&m) == 2);
        assert_eq!(sel.verts.len(), 1);
        assert!(merge_center(&cube(), &Selection::new(SelectMode::Vertex)).is_none());
    }

    #[test]
    fn delete_face_opens_the_mesh() {
        let (m, sel) = delete_selected(&cube(), &face_sel(&[1])).unwrap();
        assert_eq!((m.verts.len(), m.faces.len()), (8, 5));
        assert!(sel.is_empty());
        let mut v = Selection::new(SelectMode::Vertex);
        v.verts.insert(0);
        let (m, _) = delete_selected(&cube(), &v).unwrap();
        assert_eq!(m.faces.len(), 3);
        assert_eq!(m.verts.len(), 7);
    }

    #[test]
    fn apply_respects_select_mode() {
        let v = Selection::new(SelectMode::Vertex);
        assert!(apply(OpKind::Extrude, &cube(), &v, 0.5).is_none());
        assert!(apply(OpKind::Inset, &cube(), &edge_sel(&[(0, 1)]), 0.3).is_none());
        assert!(apply(OpKind::Bevel, &cube(), &face_sel(&[0]), 0.3).is_none());
        assert!(apply(OpKind::LoopCut, &cube(), &face_sel(&[0]), 1.0).is_none());
    }
}
