import * as THREE from "three";
import { EditableMesh } from "../core/EditableMesh";
import { edgesOverlayGeometry, faceDotsOverlayGeometry, meshToBufferGeometry, verticesOverlayGeometry } from "../core/meshToGeometry";
import { evaluateStack } from "../modifiers/types";
import type { Modifier, ModifierType, ModifierEvaluator } from "../modifiers/types";
import { bevelEvaluator } from "../modifiers/bevelModifier";

const evaluators: Record<ModifierType, ModifierEvaluator> = {
  bevel: bevelEvaluator,
};

const BASE_MATERIAL = () =>
  new THREE.MeshStandardMaterial({ color: 0x9a9a9a, flatShading: true, roughness: 0.6, metalness: 0.05 });

const SELECTED_VERT_COLOR = 0xff8a1f;
const VERT_COLOR = 0x111111;
const EDGE_COLOR = 0x1a1a1a;
const SELECTED_EDGE_COLOR = 0xff8a1f;
const FACE_DOT_COLOR = 0x1a1a1a;
const SELECTED_FACE_COLOR = 0xff8a1f;

let nextId = 1;

/** One object in the scene: an editable base mesh plus its modifier stack. */
export class SceneObject {
  readonly id: string;
  name: string;
  base: EditableMesh;
  modifiers: Modifier[] = [];

  readonly group: THREE.Group;
  readonly renderMesh: THREE.Mesh;
  readonly outline: THREE.LineSegments;

  readonly editVerts: THREE.Points;
  readonly editVertsSelected: THREE.Points;
  readonly editEdges: THREE.LineSegments;
  readonly editEdgesSelected: THREE.LineSegments;
  readonly editFaceDots: THREE.Points;
  readonly editFaceDotsSelected: THREE.Points;
  readonly editGroup: THREE.Group;

  private material = BASE_MATERIAL();

  constructor(base: EditableMesh, name: string) {
    this.id = `obj_${nextId++}`;
    this.name = name;
    this.base = base;

    this.group = new THREE.Group();

    this.renderMesh = new THREE.Mesh(meshToBufferGeometry(base), this.material);
    this.renderMesh.userData.sceneObject = this;
    this.group.add(this.renderMesh);

    this.outline = new THREE.LineSegments(
      new THREE.EdgesGeometry(this.renderMesh.geometry, 1),
      new THREE.LineBasicMaterial({ color: 0xff8a1f, linewidth: 2 }),
    );
    this.outline.visible = false;
    this.group.add(this.outline);

    const vertMat = new THREE.PointsMaterial({ color: VERT_COLOR, size: 8, sizeAttenuation: false });
    const vertSelMat = new THREE.PointsMaterial({ color: SELECTED_VERT_COLOR, size: 10, sizeAttenuation: false });
    const edgeMat = new THREE.LineBasicMaterial({ color: EDGE_COLOR });
    const edgeSelMat = new THREE.LineBasicMaterial({ color: SELECTED_EDGE_COLOR, linewidth: 2 });
    const faceDotMat = new THREE.PointsMaterial({ color: FACE_DOT_COLOR, size: 6, sizeAttenuation: false });
    const faceDotSelMat = new THREE.PointsMaterial({ color: SELECTED_FACE_COLOR, size: 8, sizeAttenuation: false });

    this.editEdges = new THREE.LineSegments(edgesOverlayGeometry(base), edgeMat);
    this.editEdgesSelected = new THREE.LineSegments(new THREE.BufferGeometry(), edgeSelMat);
    this.editVerts = new THREE.Points(verticesOverlayGeometry(base), vertMat);
    this.editVertsSelected = new THREE.Points(new THREE.BufferGeometry(), vertSelMat);
    this.editFaceDots = new THREE.Points(faceDotsOverlayGeometry(base), faceDotMat);
    this.editFaceDotsSelected = new THREE.Points(new THREE.BufferGeometry(), faceDotSelMat);

    this.editGroup = new THREE.Group();
    this.editGroup.add(
      this.editEdges,
      this.editEdgesSelected,
      this.editVerts,
      this.editVertsSelected,
      this.editFaceDots,
      this.editFaceDotsSelected,
    );
    this.editGroup.visible = false;
    this.group.add(this.editGroup);
  }

  get evaluatedMesh(): EditableMesh {
    return evaluateStack(this.base, this.modifiers, evaluators);
  }

  addModifier(modifier: Modifier): void {
    this.modifiers.push(modifier);
    this.recompute();
  }

  removeModifier(id: string): void {
    this.modifiers = this.modifiers.filter((m) => m.id !== id);
    this.recompute();
  }

  /** Rebuilds render + edit-overlay geometry from the current base mesh + modifier stack. */
  recompute(): void {
    const evaluated = this.evaluatedMesh;
    this.renderMesh.geometry.dispose();
    this.renderMesh.geometry = meshToBufferGeometry(evaluated);
    this.outline.geometry.dispose();
    this.outline.geometry = new THREE.EdgesGeometry(this.renderMesh.geometry, 1);

    this.editEdges.geometry.dispose();
    this.editEdges.geometry = edgesOverlayGeometry(this.base);
    this.editVerts.geometry.dispose();
    this.editVerts.geometry = verticesOverlayGeometry(this.base);
    this.editFaceDots.geometry.dispose();
    this.editFaceDots.geometry = faceDotsOverlayGeometry(this.base);

    this.updateSelectionOverlay();
  }

  /** Cheap refresh of just the selection-highlight overlays (no full rebuild). */
  updateSelectionOverlay(): void {
    const mesh = this.base;

    const vertPositions: number[] = [];
    mesh.selectedVerts.forEach((vi) => {
      const v = mesh.vertices[vi];
      vertPositions.push(v.x, v.y, v.z);
    });
    this.editVertsSelected.geometry.dispose();
    this.editVertsSelected.geometry = new THREE.BufferGeometry();
    this.editVertsSelected.geometry.setAttribute("position", new THREE.Float32BufferAttribute(vertPositions, 3));

    const edgePositions: number[] = [];
    const edges = mesh.computeEdges();
    mesh.selectedEdges.forEach((key) => {
      const info = edges.get(key);
      if (!info) return;
      const a = mesh.vertices[info.v0];
      const b = mesh.vertices[info.v1];
      edgePositions.push(a.x, a.y, a.z, b.x, b.y, b.z);
    });
    this.editEdgesSelected.geometry.dispose();
    this.editEdgesSelected.geometry = new THREE.BufferGeometry();
    this.editEdgesSelected.geometry.setAttribute("position", new THREE.Float32BufferAttribute(edgePositions, 3));

    const facePositions: number[] = [];
    mesh.selectedFaces.forEach((fi) => {
      const c = mesh.faceCentroid(fi);
      facePositions.push(c.x, c.y, c.z);
    });
    this.editFaceDotsSelected.geometry.dispose();
    this.editFaceDotsSelected.geometry = new THREE.BufferGeometry();
    this.editFaceDotsSelected.geometry.setAttribute("position", new THREE.Float32BufferAttribute(facePositions, 3));
  }

  setEditMode(active: boolean): void {
    this.editGroup.visible = active;
    this.renderMesh.material = this.material;
    (this.material as THREE.MeshStandardMaterial).wireframe = false;
    this.renderMesh.visible = true;
    if (active) this.updateSelectionOverlay();
  }

  setSelected(selected: boolean): void {
    this.outline.visible = selected;
  }

  dispose(): void {
    this.renderMesh.geometry.dispose();
    this.outline.geometry.dispose();
    this.editVerts.geometry.dispose();
    this.editVertsSelected.geometry.dispose();
    this.editEdges.geometry.dispose();
    this.editEdgesSelected.geometry.dispose();
    this.editFaceDots.geometry.dispose();
    this.editFaceDotsSelected.geometry.dispose();
    this.material.dispose();
  }
}
