import * as THREE from "three";
import { EditableMesh, fixOutwardWinding } from "./EditableMesh";

export function generateCube(size = 2): EditableMesh {
  const s = size / 2;
  const vertices = [
    new THREE.Vector3(-s, -s, -s), // 0
    new THREE.Vector3(s, -s, -s), // 1
    new THREE.Vector3(s, s, -s), // 2
    new THREE.Vector3(-s, s, -s), // 3
    new THREE.Vector3(-s, -s, s), // 4
    new THREE.Vector3(s, -s, s), // 5
    new THREE.Vector3(s, s, s), // 6
    new THREE.Vector3(-s, s, s), // 7
  ];
  const faces = [
    [0, 3, 2, 1], // back  (-z)
    [4, 5, 6, 7], // front (+z)
    [0, 1, 5, 4], // bottom (-y)
    [3, 7, 6, 2], // top    (+y)
    [0, 4, 7, 3], // left   (-x)
    [1, 2, 6, 5], // right  (+x)
  ];
  return new EditableMesh(vertices, faces);
}

export function generatePlane(size = 2): EditableMesh {
  const s = size / 2;
  const vertices = [
    new THREE.Vector3(-s, 0, -s),
    new THREE.Vector3(-s, 0, s),
    new THREE.Vector3(s, 0, s),
    new THREE.Vector3(s, 0, -s),
  ];
  return new EditableMesh(vertices, [[0, 1, 2, 3]]);
}

export function generateCylinder(radius = 1, height = 2, segments = 16): EditableMesh {
  const vertices: THREE.Vector3[] = [];
  const bottomRing: number[] = [];
  const topRing: number[] = [];

  for (let seg = 0; seg < segments; seg++) {
    const theta = (seg / segments) * Math.PI * 2;
    const x = radius * Math.cos(theta);
    const z = radius * Math.sin(theta);
    bottomRing.push(vertices.length);
    vertices.push(new THREE.Vector3(x, -height / 2, z));
  }
  for (let seg = 0; seg < segments; seg++) {
    const theta = (seg / segments) * Math.PI * 2;
    const x = radius * Math.cos(theta);
    const z = radius * Math.sin(theta);
    topRing.push(vertices.length);
    vertices.push(new THREE.Vector3(x, height / 2, z));
  }

  const faces: number[][] = [];
  faces.push(bottomRing.slice());
  faces.push(topRing.slice());
  for (let seg = 0; seg < segments; seg++) {
    const next = (seg + 1) % segments;
    faces.push([bottomRing[seg], bottomRing[next], topRing[next], topRing[seg]]);
  }

  const mesh = new EditableMesh(vertices, faces);
  fixOutwardWinding(mesh);
  return mesh;
}

export function generateCone(radius = 1, height = 2, segments = 16): EditableMesh {
  const vertices: THREE.Vector3[] = [];
  const baseRing: number[] = [];

  for (let seg = 0; seg < segments; seg++) {
    const theta = (seg / segments) * Math.PI * 2;
    const x = radius * Math.cos(theta);
    const z = radius * Math.sin(theta);
    baseRing.push(vertices.length);
    vertices.push(new THREE.Vector3(x, -height / 2, z));
  }
  const apexIndex = vertices.length;
  vertices.push(new THREE.Vector3(0, height / 2, 0));

  const faces: number[][] = [];
  faces.push(baseRing.slice());
  for (let seg = 0; seg < segments; seg++) {
    const next = (seg + 1) % segments;
    faces.push([baseRing[seg], baseRing[next], apexIndex]);
  }

  const mesh = new EditableMesh(vertices, faces);
  fixOutwardWinding(mesh);
  return mesh;
}

export function generateUVSphere(radius = 1, widthSegments = 16, heightSegments = 8): EditableMesh {
  const vertices: THREE.Vector3[] = [];
  const topPole = 0;
  vertices.push(new THREE.Vector3(0, radius, 0));

  const rings: number[][] = [];
  for (let ring = 1; ring < heightSegments; ring++) {
    const phi = (Math.PI * ring) / heightSegments;
    const y = radius * Math.cos(phi);
    const ringRadius = radius * Math.sin(phi);
    const indices: number[] = [];
    for (let seg = 0; seg < widthSegments; seg++) {
      const theta = (seg / widthSegments) * Math.PI * 2;
      indices.push(vertices.length);
      vertices.push(new THREE.Vector3(ringRadius * Math.cos(theta), y, ringRadius * Math.sin(theta)));
    }
    rings.push(indices);
  }
  const bottomPole = vertices.length;
  vertices.push(new THREE.Vector3(0, -radius, 0));

  const faces: number[][] = [];
  const firstRing = rings[0];
  for (let seg = 0; seg < widthSegments; seg++) {
    const next = (seg + 1) % widthSegments;
    faces.push([topPole, firstRing[seg], firstRing[next]]);
  }
  for (let r = 0; r < rings.length - 1; r++) {
    const ringA = rings[r];
    const ringB = rings[r + 1];
    for (let seg = 0; seg < widthSegments; seg++) {
      const next = (seg + 1) % widthSegments;
      faces.push([ringA[seg], ringA[next], ringB[next], ringB[seg]]);
    }
  }
  const lastRing = rings[rings.length - 1];
  for (let seg = 0; seg < widthSegments; seg++) {
    const next = (seg + 1) % widthSegments;
    faces.push([bottomPole, lastRing[next], lastRing[seg]]);
  }

  const mesh = new EditableMesh(vertices, faces);
  fixOutwardWinding(mesh);
  return mesh;
}

export type PrimitiveType = "cube" | "plane" | "cylinder" | "cone" | "sphere";

export function generatePrimitive(type: PrimitiveType): EditableMesh {
  switch (type) {
    case "cube":
      return generateCube();
    case "plane":
      return generatePlane();
    case "cylinder":
      return generateCylinder();
    case "cone":
      return generateCone();
    case "sphere":
      return generateUVSphere();
  }
}
