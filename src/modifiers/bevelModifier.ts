import * as THREE from "three";
import { EditableMesh, edgeKey, fixOutwardWinding } from "../core/EditableMesh";
import type { EdgeInfo } from "../core/EditableMesh";
import type { Modifier, ModifierEvaluator } from "./types";

export type BevelLimitMethod = "angle" | "none" | "weight";

export interface BevelParams {
  width: number;
  segments: number;
  limitMethod: BevelLimitMethod;
  /** radians */
  angle: number;
  clampOverlap: boolean;
}

export interface BevelModifier extends Modifier {
  type: "bevel";
  params: BevelParams;
}

export function defaultBevelParams(): BevelParams {
  return {
    width: 0.1,
    segments: 1,
    limitMethod: "angle",
    angle: THREE.MathUtils.degToRad(30),
    clampOverlap: true,
  };
}

export function createBevelModifier(name = "Bevel"): BevelModifier {
  return {
    id: crypto.randomUUID(),
    type: "bevel",
    enabled: true,
    name,
    params: defaultBevelParams(),
  };
}

interface Vec2 {
  x: number;
  y: number;
}

const EPS = 1e-9;

function sub2(a: Vec2, b: Vec2): Vec2 {
  return { x: a.x - b.x, y: a.y - b.y };
}

function rot90(v: Vec2): Vec2 {
  return { x: -v.y, y: v.x };
}

function len2(v: Vec2): number {
  return Math.hypot(v.x, v.y);
}

function normalize2(v: Vec2): Vec2 {
  const l = len2(v) || 1;
  return { x: v.x / l, y: v.y / l };
}

/** Intersects two infinite 2D lines given as point + direction. Null if parallel. */
function intersectLines(p1: Vec2, d1: Vec2, p2: Vec2, d2: Vec2): Vec2 | null {
  const denom = d1.x * d2.y - d1.y * d2.x;
  if (Math.abs(denom) < EPS) return null;
  const diff = sub2(p2, p1);
  const t = (diff.x * d2.y - diff.y * d2.x) / denom;
  return { x: p1.x + t * d1.x, y: p1.y + t * d1.y };
}

/** Spherical-ish interpolation between two (possibly zero-length) offset vectors. */
function slerpOffset(a: THREE.Vector3, b: THREE.Vector3, t: number): THREE.Vector3 {
  const lenA = a.length();
  const lenB = b.length();
  if (lenA < EPS && lenB < EPS) return new THREE.Vector3();
  if (lenA < EPS) return b.clone().multiplyScalar(t);
  if (lenB < EPS) return a.clone().multiplyScalar(1 - t);

  const dirA = a.clone().normalize();
  const dirB = b.clone().normalize();
  const angle = dirA.angleTo(dirB);
  const length = THREE.MathUtils.lerp(lenA, lenB, t);
  if (angle < 1e-4) {
    return dirA.multiplyScalar(length);
  }
  const sinAngle = Math.sin(angle);
  const w1 = Math.sin((1 - t) * angle) / sinAngle;
  const w2 = Math.sin(t * angle) / sinAngle;
  return dirA.multiplyScalar(w1).add(dirB.multiplyScalar(w2)).normalize().multiplyScalar(length);
}

function otherFace(edges: Map<string, EdgeInfo>, a: number, b: number, exclude: number): number | undefined {
  const info = edges.get(edgeKey(a, b));
  if (!info) return undefined;
  return info.faces.find((f) => f !== exclude);
}

/**
 * Non-destructive Bevel modifier, modeled after Blender's Bevel modifier:
 * chamfers edges (flat for segments=1, rounded for segments>1) based on a
 * limit method (Angle / None / Weight), without touching the base mesh.
 *
 * Implementation notes / known limitations vs. Blender's BMesh bevel solver:
 * - Operates per-face via 2D polygon inset (variable width per edge) then
 *   bridges adjacent faces and caps each vertex with a fan of its
 *   surrounding inset points, walked in adjacency order.
 * - Boundary edges (open meshes) are never beveled.
 * - Works best on convex, manifold, quad/tri meshes (our primitives); may
 *   produce artifacts on concave or non-manifold geometry.
 */
export const bevelEvaluator: ModifierEvaluator = {
  apply(mesh: EditableMesh, modifier: Modifier): EditableMesh {
    const { params } = modifier as BevelModifier;
    const width = Math.max(0, params.width);
    const segments = Math.max(1, Math.round(params.segments));

    if (width < EPS) return mesh.clone();

    const edges = mesh.computeEdges();
    const edgeWidth = new Map<string, number>();
    edges.forEach((info, key) => {
      if (info.faces.length < 2) {
        edgeWidth.set(key, 0);
        return;
      }
      let w = 0;
      if (params.limitMethod === "none") {
        w = width;
      } else if (params.limitMethod === "weight") {
        const weight = mesh.bevelWeights.get(key) ?? 0;
        w = width * weight;
      } else {
        const nA = mesh.faceNormal(info.faces[0]);
        const nB = mesh.faceNormal(info.faces[1]);
        w = nA.angleTo(nB) >= params.angle ? width : 0;
      }
      edgeWidth.set(key, w);
    });

    if (![...edgeWidth.values()].some((w) => w > EPS)) return mesh.clone();

    const newVertices: THREE.Vector3[] = [];
    const newFaces: number[][] = [];
    // (faceIndex, originalVertexIndex) -> new vertex index
    const cornerIndex = new Map<string, number>();

    // --- 1. Per-face inset (variable width per edge) ---
    mesh.faces.forEach((face, faceIndex) => {
      const n = face.length;
      const normal = mesh.faceNormal(faceIndex);
      const origin = mesh.vertices[face[0]];
      const uAxis = new THREE.Vector3().subVectors(mesh.vertices[face[1]], origin).normalize();
      const vAxis = new THREE.Vector3().crossVectors(normal, uAxis).normalize();

      const project = (p: THREE.Vector3): Vec2 => {
        const d = new THREE.Vector3().subVectors(p, origin);
        return { x: d.dot(uAxis), y: d.dot(vAxis) };
      };
      const unproject = (p: Vec2): THREE.Vector3 =>
        origin.clone().addScaledVector(uAxis, p.x).addScaledVector(vAxis, p.y);

      const pts2D = face.map((vi) => project(mesh.vertices[vi]));
      const centroid2D: Vec2 = pts2D.reduce(
        (acc, p) => ({ x: acc.x + p.x / n, y: acc.y + p.y / n }),
        { x: 0, y: 0 },
      );

      const edgeW: number[] = [];
      for (let i = 0; i < n; i++) {
        const a = face[i];
        const b = face[(i + 1) % n];
        let w = edgeWidth.get(edgeKey(a, b)) ?? 0;
        if (params.clampOverlap) {
          const edgeLen = len2(sub2(pts2D[(i + 1) % n], pts2D[i]));
          w = Math.min(w, edgeLen * 0.45);
        }
        edgeW.push(w);
      }

      const inwardPerp = (from: Vec2, dir: Vec2): Vec2 => {
        let perp = normalize2(rot90(dir));
        const toCentroid = sub2(centroid2D, from);
        if (perp.x * toCentroid.x + perp.y * toCentroid.y < 0) perp = { x: -perp.x, y: -perp.y };
        return perp;
      };

      for (let i = 0; i < n; i++) {
        const prevW = edgeW[(i - 1 + n) % n];
        const nextW = edgeW[i];
        const originalIndex = face[i];

        let newPos2D: Vec2;
        if (prevW < EPS && nextW < EPS) {
          newPos2D = pts2D[i];
        } else {
          const prevPoint = pts2D[(i - 1 + n) % n];
          const nextPoint = pts2D[(i + 1) % n];
          const dPrev = sub2(pts2D[i], prevPoint);
          const dNext = sub2(nextPoint, pts2D[i]);
          const perpPrev = inwardPerp(prevPoint, dPrev);
          const perpNext = inwardPerp(pts2D[i], dNext);
          const offsetA = { x: pts2D[i].x + perpPrev.x * prevW, y: pts2D[i].y + perpPrev.y * prevW };
          const offsetB = { x: pts2D[i].x + perpNext.x * nextW, y: pts2D[i].y + perpNext.y * nextW };
          const hit = intersectLines(offsetA, dPrev, offsetB, dNext);
          newPos2D = hit ?? { x: (offsetA.x + offsetB.x) / 2, y: (offsetA.y + offsetB.y) / 2 };
        }

        const worldPos = unproject(newPos2D);
        const idx = newVertices.length;
        newVertices.push(worldPos);
        cornerIndex.set(`${faceIndex}_${originalIndex}`, idx);
      }

      newFaces.push(face.map((vi) => cornerIndex.get(`${faceIndex}_${vi}`)!));
    });

    // --- 2. Straps bridging the two faces of each shared edge ---
    edges.forEach((info) => {
      if (info.faces.length < 2) return;
      const [fA, fB] = info.faces;
      const w = edgeWidth.get(edgeKey(info.v0, info.v1)) ?? 0;
      const segCount = w > EPS ? segments : 1;

      const pA0 = newVertices[cornerIndex.get(`${fA}_${info.v0}`)!];
      const pA1 = newVertices[cornerIndex.get(`${fA}_${info.v1}`)!];
      const pB0 = newVertices[cornerIndex.get(`${fB}_${info.v0}`)!];
      const pB1 = newVertices[cornerIndex.get(`${fB}_${info.v1}`)!];

      const origin0 = mesh.vertices[info.v0];
      const origin1 = mesh.vertices[info.v1];
      const offA0 = new THREE.Vector3().subVectors(pA0, origin0);
      const offB0 = new THREE.Vector3().subVectors(pB0, origin0);
      const offA1 = new THREE.Vector3().subVectors(pA1, origin1);
      const offB1 = new THREE.Vector3().subVectors(pB1, origin1);

      const row0: number[] = [];
      const row1: number[] = [];
      for (let s = 0; s <= segCount; s++) {
        const t = s / segCount;
        const p0 = origin0.clone().add(slerpOffset(offA0, offB0, t));
        const p1 = origin1.clone().add(slerpOffset(offA1, offB1, t));
        row0.push(newVertices.length);
        newVertices.push(p0);
        row1.push(newVertices.length);
        newVertices.push(p1);
      }
      for (let s = 0; s < segCount; s++) {
        newFaces.push([row0[s], row0[s + 1], row1[s + 1], row1[s]]);
      }
    });

    // --- 3. Vertex corner caps (fan of surrounding inset points) ---
    mesh.vertices.forEach((_pos, vi) => {
      const facesAtV = mesh.facesOfVertex(vi);
      if (facesAtV.length < 3) return;

      const positions = facesAtV.map((f) => newVertices[cornerIndex.get(`${f}_${vi}`)!]);
      const distinct = new Set(positions.map((p) => `${p.x.toFixed(6)}_${p.y.toFixed(6)}_${p.z.toFixed(6)}`));
      if (distinct.size <= 1) return; // vertex untouched by the bevel

      const order: number[] = [];
      let current = facesAtV[0];
      const start = current;
      for (let guard = 0; guard < facesAtV.length + 1; guard++) {
        order.push(current);
        const faceVerts = mesh.faces[current];
        const idx = faceVerts.indexOf(vi);
        const nextVert = faceVerts[(idx + 1) % faceVerts.length];
        const nf = otherFace(edges, vi, nextVert, current);
        if (nf === undefined || nf === start) break;
        if (order.includes(nf)) break;
        current = nf;
      }
      if (order.length !== facesAtV.length) return; // not a clean manifold fan; skip cap

      const capIndices = order.map((f) => cornerIndex.get(`${f}_${vi}`)!);
      newFaces.push(capIndices);
    });

    const result = new EditableMesh(newVertices, newFaces);
    fixOutwardWinding(result);
    return result;
  },
};
