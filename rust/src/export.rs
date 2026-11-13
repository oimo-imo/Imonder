//! Export to OBJ (text, n-gons kept) and glTF binary (.glb, triangles with flat normals).
//! Both convert from this app's Z-up world to the Y-up convention of those formats:
//! (x, y, z) -> (x, z, -y), so the model stands upright in Blender, Unity and others.

use crate::math::{v3, V3};
use crate::scene::Object;

fn y_up(p: V3) -> V3 {
    v3(p.x, p.z, -p.y)
}

fn fmt(f: f32) -> String {
    let s = format!("{:.6}", f);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() {
        "0".into()
    } else {
        s.into()
    }
}

/// Wavefront OBJ: one `o` block per object, one flat normal per face.
pub fn to_obj(objects: &[&Object]) -> String {
    let mut out = String::from("# Imonder\n");
    let (mut v_base, mut n_base) = (1usize, 1usize);
    for o in objects {
        out.push_str(&format!("o {}\n", o.name.replace(['\n', '\r'], " ")));
        for &p in &o.mesh.verts {
            let q = y_up(p);
            out.push_str(&format!("v {} {} {}\n", fmt(q.x), fmt(q.y), fmt(q.z)));
        }
        for f in &o.mesh.faces {
            let n = y_up(o.mesh.face_normal(f));
            out.push_str(&format!("vn {} {} {}\n", fmt(n.x), fmt(n.y), fmt(n.z)));
        }
        out.push_str("s off\n");
        for (fi, f) in o.mesh.faces.iter().enumerate() {
            out.push('f');
            for &i in f {
                out.push_str(&format!(" {}//{}", v_base + i as usize, n_base + fi));
            }
            out.push('\n');
        }
        v_base += o.mesh.verts.len();
        n_base += o.mesh.faces.len();
    }
    out
}

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// glTF 2.0 binary: one node + mesh per object, a shared clay-grey material.
/// Faces are fanned into triangles and every face gets its own vertices so it stays flat.
pub fn to_glb(objects: &[&Object]) -> Vec<u8> {
    let mut bin: Vec<u8> = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut meshes = Vec::new();
    let mut nodes = Vec::new();

    let mut push_view = |bin: &mut Vec<u8>, bytes: &[u8], target: u32| -> usize {
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let offset = bin.len();
        bin.extend_from_slice(bytes);
        views.push(format!("{{\"buffer\":0,\"byteOffset\":{offset},\"byteLength\":{},\"target\":{target}}}", bytes.len()));
        views.len() - 1
    };

    for o in objects {
        let (mut pos, mut nrm, mut idx): (Vec<f32>, Vec<f32>, Vec<u32>) = (Vec::new(), Vec::new(), Vec::new());
        for f in &o.mesh.faces {
            if f.len() < 3 {
                continue;
            }
            let n = y_up(o.mesh.face_normal(f));
            let first = (pos.len() / 3) as u32;
            for &i in f {
                let p = y_up(o.mesh.verts[i as usize]);
                pos.extend_from_slice(&[p.x, p.y, p.z]);
                nrm.extend_from_slice(&[n.x, n.y, n.z]);
            }
            for k in 1..f.len() as u32 - 1 {
                idx.extend_from_slice(&[first, first + k, first + k + 1]);
            }
        }
        if idx.is_empty() {
            continue;
        }
        let count = pos.len() / 3;
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for c in pos.chunks(3) {
            for k in 0..3 {
                lo[k] = lo[k].min(c[k]);
                hi[k] = hi[k].max(c[k]);
            }
        }
        let le = |v: &[f32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        let pv = push_view(&mut bin, &le(&pos), 34962);
        let nv = push_view(&mut bin, &le(&nrm), 34962);
        let iv = push_view(&mut bin, &idx.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>(), 34963);
        let a = accessors.len();
        accessors.push(format!(
            "{{\"bufferView\":{pv},\"componentType\":5126,\"count\":{count},\"type\":\"VEC3\",\"min\":[{},{},{}],\"max\":[{},{},{}]}}",
            lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]
        ));
        accessors.push(format!("{{\"bufferView\":{nv},\"componentType\":5126,\"count\":{count},\"type\":\"VEC3\"}}"));
        accessors.push(format!("{{\"bufferView\":{iv},\"componentType\":5125,\"count\":{},\"type\":\"SCALAR\"}}", idx.len()));
        meshes.push(format!(
            "{{\"name\":{},\"primitives\":[{{\"attributes\":{{\"POSITION\":{},\"NORMAL\":{}}},\"indices\":{},\"material\":0}}]}}",
            json_str(&o.name),
            a,
            a + 1,
            a + 2
        ));
        nodes.push(format!("{{\"name\":{},\"mesh\":{}}}", json_str(&o.name), meshes.len() - 1));
    }
    while bin.len() % 4 != 0 {
        bin.push(0);
    }

    let scene_nodes: Vec<String> = (0..nodes.len()).map(|i| i.to_string()).collect();
    let mut json = format!(
        concat!(
            "{{\"asset\":{{\"version\":\"2.0\",\"generator\":\"Imonder\"}},\"scene\":0,",
            "\"scenes\":[{{\"nodes\":[{}]}}],\"nodes\":[{}],\"meshes\":[{}],",
            "\"materials\":[{{\"name\":\"Clay\",\"pbrMetallicRoughness\":{{\"baseColorFactor\":[0.58,0.58,0.61,1],\"metallicFactor\":0,\"roughnessFactor\":0.8}}}}],",
            "\"accessors\":[{}],\"bufferViews\":[{}],\"buffers\":[{{\"byteLength\":{}}}]}}"
        ),
        scene_nodes.join(","),
        nodes.join(","),
        meshes.join(","),
        accessors.join(","),
        views.join(","),
        bin.len()
    );
    while json.len() % 4 != 0 {
        json.push(' ');
    }

    let total = 12 + 8 + json.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(json.as_bytes());
    if !bin.is_empty() {
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::Mesh;

    fn obj(name: &str, mesh: Mesh) -> Object {
        Object::new(name.into(), mesh)
    }

    #[test]
    fn obj_keeps_quads_and_turns_z_up_into_y_up() {
        let mut tall = Mesh::cube(1.0);
        for v in &mut tall.verts {
            v.z = v.z * 2.0 + 2.0; // z: 0..4
        }
        let a = obj("塔", tall);
        let b = obj("床", Mesh::plane(3.0));
        let text = to_obj(&[&a, &b]);
        let verts: Vec<Vec<f32>> = text
            .lines()
            .filter(|l| l.starts_with("v "))
            .map(|l| l[2..].split(' ').map(|x| x.parse().unwrap()).collect())
            .collect();
        assert_eq!(verts.len(), 12);
        let ymax = verts.iter().map(|v| v[1]).fold(f32::MIN, f32::max);
        assert_eq!(ymax, 4.0, "the tower's height ends up on +Y");
        let faces: Vec<&str> = text.lines().filter(|l| l.starts_with("f ")).collect();
        assert_eq!(faces.len(), 7);
        assert!(faces.iter().all(|f| f.split(' ').count() == 5), "quads stay quads");
        assert_eq!(faces[6], "f 9//7 10//7 11//7 12//7", "indices continue across objects");
        assert!(text.contains("o 塔\n") && text.contains("o 床\n"));
        // The plane faced +Z, so its normal is now +Y.
        let normals: Vec<&str> = text.lines().filter(|l| l.starts_with("vn ")).collect();
        assert_eq!(normals[6], "vn 0 1 0");
    }

    #[test]
    fn glb_is_well_formed_and_self_consistent() {
        let a = obj("立方体", Mesh::cube(1.0));
        let b = obj("球 \"1\"", Mesh::sphere(1.0, 8, 4));
        let glb = to_glb(&[&a, &b]);
        assert_eq!(&glb[0..4], b"glTF");
        let u32_at = |i: usize| u32::from_le_bytes(glb[i..i + 4].try_into().unwrap()) as usize;
        assert_eq!(u32_at(4), 2);
        assert_eq!(u32_at(8), glb.len());
        let json_len = u32_at(12);
        assert_eq!(&glb[16..20], b"JSON");
        assert_eq!(json_len % 4, 0);
        let json: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        let bin_start = 20 + json_len;
        let bin_len = u32_at(bin_start);
        assert_eq!(&glb[bin_start + 4..bin_start + 8], b"BIN\0");
        let bin = &glb[bin_start + 8..bin_start + 8 + bin_len];
        assert_eq!(json["buffers"][0]["byteLength"].as_u64().unwrap() as usize, bin_len);
        assert_eq!(json["nodes"][1]["name"], "球 \"1\"");
        let views = json["bufferViews"].as_array().unwrap();
        let accessors = json["accessors"].as_array().unwrap();
        for acc in accessors {
            let view = &views[acc["bufferView"].as_u64().unwrap() as usize];
            let per = match acc["type"].as_str().unwrap() {
                "VEC3" => 12,
                "SCALAR" => 4,
                t => panic!("{t}"),
            };
            let len = view["byteLength"].as_u64().unwrap() as usize;
            let off = view["byteOffset"].as_u64().unwrap() as usize;
            assert_eq!(len, acc["count"].as_u64().unwrap() as usize * per);
            assert!(off + len <= bin.len() && off % 4 == 0);
        }
        // Cube: 6 quads -> 24 corner vertices, 12 triangles; indices stay in range.
        let prim = &json["meshes"][0]["primitives"][0];
        let pos = &accessors[prim["attributes"]["POSITION"].as_u64().unwrap() as usize];
        assert_eq!(pos["count"], 24);
        let min: Vec<f64> = pos["min"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
        assert_eq!(min, vec![-1.0, -1.0, -1.0]);
        let ind = &accessors[prim["indices"].as_u64().unwrap() as usize];
        assert_eq!(ind["count"], 36);
        let iv = &views[ind["bufferView"].as_u64().unwrap() as usize];
        let off = iv["byteOffset"].as_u64().unwrap() as usize;
        let max_index = bin[off..off + 36 * 4].chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).max().unwrap();
        assert_eq!(max_index, 23);
        // Every triangle winds counter-clockwise around its stored normal (outward).
        let nv = &views[accessors[prim["attributes"]["NORMAL"].as_u64().unwrap() as usize]["bufferView"].as_u64().unwrap() as usize];
        let pv = &views[pos["bufferView"].as_u64().unwrap() as usize];
        let f = |view: &serde_json::Value, i: usize| {
            let o = view["byteOffset"].as_u64().unwrap() as usize + i * 12;
            let g = |k: usize| f32::from_le_bytes(bin[o + k * 4..o + k * 4 + 4].try_into().unwrap());
            v3(g(0), g(1), g(2))
        };
        let tri: Vec<usize> = bin[off..off + 36 * 4].chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap()) as usize).collect();
        for t in tri.chunks(3) {
            let (a, b, c) = (f(pv, t[0]), f(pv, t[1]), f(pv, t[2]));
            assert!(b.sub(a).cross(c.sub(a)).dot(f(nv, t[0])) > 0.0);
        }
    }

    #[test]
    fn empty_export_is_still_a_valid_file() {
        let glb = to_glb(&[]);
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let json: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        assert_eq!(json["nodes"].as_array().unwrap().len(), 0);
        assert_eq!(glb.len(), 20 + json_len);
        assert_eq!(to_obj(&[]), "# Imonder\n");
    }
}
