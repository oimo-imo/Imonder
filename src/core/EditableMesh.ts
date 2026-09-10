import * as THREE from "three";

export function edgeKey(a: number, b: number): string {
  return a < b ? `${a}_${b}` : `${b}_${a}`;
}

export interface EdgeInfo {
  v0: number;
  v1: number;
  faces: number[];
}

export type SelectMode = "vertex" | "edge" | "face";

/**
 * A simple indexed polygon mesh (n-gon faces, assumed manifold-ish and
 * outward-facing CCW winding). This is the "edit mesh" that the user
 * directly manipulates in Edit Mode, and the source geometry that the
 * non-destructive modifier stack (see modifiers/) reads from.
 */
export class EditableMesh {
  vertices: THREE.Vector3[];
  faces: number[][];

  selectedVerts = new Set<number>();
  selectedEdges = new Set<string>();
  selectedFaces = new Set<number>();

  /** Per-edge bevel weight (0..1), analogous to Blender's Edge Bevel Weight. */
  bevelWeights = new Map<string, number>();

  constructor(vertices: THREE.Vector3[] = [], faces: number[][] = []) {
    this.vertices = vertices;
    this.faces = faces;
  }

  clone(): EditableMesh {
    const m = new EditableMesh(
      this.vertices.map((v) => v.clone()),
      this.faces.map((f) => f.slice()),
    );
    m.selectedVerts = new Set(this.selectedVerts);
    m.selectedEdges = new Set(this.selectedEdges);
    m.selectedFaces = new Set(this.selectedFaces);
    m.bevelWeights = new Map(this.bevelWeights);
    return m;
  }

  clearSelection(): void {
    this.selectedVerts.clear();
    this.selectedEdges.clear();
    this.selectedFaces.clear();
  }

  /** Builds the edge -> adjacent-faces map from the current face list. */
  computeEdges(): Map<string, EdgeInfo> {
    const edges = new Map<string, EdgeInfo>();
    this.faces.forEach((face, faceIndex) => {
      const n = face.length;
      for (let i = 0; i < n; i++) {
        const v0 = face[i];
        const v1 = face[(i + 1) % n];
        const key = edgeKey(v0, v1);
        let info = edges.get(key);
        if (!info) {
          info = { v0, v1, faces: [] };
          edges.set(key, info);
        }
        info.faces.push(faceIndex);
      }
    });
    return edges;
  }

  /** Faces (indices) that touch a given vertex, in no particular order. */
  facesOfVertex(vertexIndex: number): number[] {
    const result: number[] = [];
    this.faces.forEach((face, i) => {
      if (face.includes(vertexIndex)) result.push(i);
    });
    return result;
  }

  faceNormal(faceIndex: number): THREE.Vector3 {
    const face = this.faces[faceIndex];
    const a = this.vertices[face[0]];
    const b = this.vertices[face[1]];
    const c = this.vertices[face[2] ?? face[0]];
    const normal = new THREE.Vector3()
      .subVectors(c, b)
      .cross(new THREE.Vector3().subVectors(a, b))
      .normalize();
    return normal;
  }

  faceCentroid(faceIndex: number): THREE.Vector3 {
    const face = this.faces[faceIndex];
    const c = new THREE.Vector3();
    for (const vi of face) c.add(this.vertices[vi]);
    c.divideScalar(face.length);
    return c;
  }

  /** Selects everything (used when entering edit mode fresh, Blender-style). */
  selectAll(mode: SelectMode): void {
    this.clearSelection();
    if (mode === "vertex") {
      this.vertices.forEach((_, i) => this.selectedVerts.add(i));
    } else if (mode === "face") {
      this.faces.forEach((_, i) => this.selectedFaces.add(i));
    } else {
      this.computeEdges().forEach((_, key) => this.selectedEdges.add(key));
    }
  }
}

/**
 * Corrects face winding so every face's normal points away from the mesh's
 * average vertex position. Valid for convex / star-shaped-from-the-center
 * meshes (our primitives and their beveled output) -- lets primitive and
 * modifier code build faces without hand-deriving exact winding order.
 */
export function fixOutwardWinding(mesh: EditableMesh): void {
  const center = new THREE.Vector3();
  for (const v of mesh.vertices) center.add(v);
  center.divideScalar(mesh.vertices.length || 1);

  mesh.faces.forEach((face, i) => {
    if (face.length < 3) return;
    const normal = mesh.faceNormal(i);
    const centroid = mesh.faceCentroid(i);
    const outward = centroid.clone().sub(center);
    if (normal.dot(outward) < 0) {
      face.reverse();
    }
  });
}
