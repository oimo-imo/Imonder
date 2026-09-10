import * as THREE from "three";
import { EditableMesh } from "./EditableMesh";

/**
 * Triangulates all faces (simple fan triangulation, valid for the convex
 * n-gons produced by our primitives and by the bevel modifier) into a
 * flat-shaded, non-indexed BufferGeometry -- matching Blender's default
 * flat shading, which keeps bevel facets crisp and readable.
 */
export function meshToBufferGeometry(mesh: EditableMesh): THREE.BufferGeometry {
  const positions: number[] = [];
  const normals: number[] = [];

  mesh.faces.forEach((face, faceIndex) => {
    if (face.length < 3) return;
    const normal = mesh.faceNormal(faceIndex);
    const v0 = mesh.vertices[face[0]];
    for (let i = 1; i < face.length - 1; i++) {
      const v1 = mesh.vertices[face[i]];
      const v2 = mesh.vertices[face[i + 1]];
      positions.push(v0.x, v0.y, v0.z, v1.x, v1.y, v1.z, v2.x, v2.y, v2.z);
      for (let k = 0; k < 3; k++) normals.push(normal.x, normal.y, normal.z);
    }
  });

  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  geometry.setAttribute("normal", new THREE.Float32BufferAttribute(normals, 3));
  return geometry;
}

/** Line-segment geometry (two points per edge) for the edit-mode wire overlay. */
export function edgesOverlayGeometry(mesh: EditableMesh): THREE.BufferGeometry {
  const positions: number[] = [];
  mesh.computeEdges().forEach((edge) => {
    const a = mesh.vertices[edge.v0];
    const b = mesh.vertices[edge.v1];
    positions.push(a.x, a.y, a.z, b.x, b.y, b.z);
  });
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  return geometry;
}

/** Point geometry (one point per vertex) for the edit-mode vertex overlay. */
export function verticesOverlayGeometry(mesh: EditableMesh): THREE.BufferGeometry {
  const positions: number[] = [];
  mesh.vertices.forEach((v) => positions.push(v.x, v.y, v.z));
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  return geometry;
}

/** Point geometry at face centroids, used for face-select-mode dots. */
export function faceDotsOverlayGeometry(mesh: EditableMesh): THREE.BufferGeometry {
  const positions: number[] = [];
  mesh.faces.forEach((_, i) => {
    const c = mesh.faceCentroid(i);
    positions.push(c.x, c.y, c.z);
  });
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  return geometry;
}
