import * as THREE from "three";
import { TransformControls } from "three/examples/jsm/controls/TransformControls.js";
import { Viewport } from "./scene/Viewport";
import type { ViewPreset } from "./scene/Viewport";
import { SceneObject } from "./scene/SceneObject";
import { generatePrimitive } from "./core/primitives";
import type { PrimitiveType } from "./core/primitives";
import type { EdgeInfo, SelectMode } from "./core/EditableMesh";
import { createBevelModifier } from "./modifiers/bevelModifier";
import type { BevelModifier, BevelLimitMethod } from "./modifiers/bevelModifier";
import type { Modifier } from "./modifiers/types";

type EditorMode = "object" | "edit";
type TransformMode = "translate" | "rotate" | "scale";

interface EditHit {
  kind: "vertex" | "edge" | "face";
  key: number | string;
}

interface DragState {
  pointerId: number;
  obj: SceneObject;
  plane: THREE.Plane;
  startWorldPoint: THREE.Vector3;
  startLocal: Map<number, THREE.Vector3>;
}

const $ = <T extends HTMLElement>(id: string): T => document.getElementById(id) as T;

export class App {
  private viewport: Viewport;
  private objects: SceneObject[] = [];
  private selected: SceneObject | null = null;
  private mode: EditorMode = "object";
  private selectMode: SelectMode = "vertex";
  private transformMode: TransformMode = "translate";

  private transformControls: TransformControls;
  private suppressNextTap = false;

  private raycaster = new THREE.Raycaster();
  private dragState: DragState | null = null;
  private tapDown: { x: number; y: number; time: number } | null = null;

  constructor() {
    this.viewport = new Viewport($("viewport"));

    this.transformControls = new TransformControls(this.viewport.camera, this.viewport.renderer.domElement);
    this.transformControls.setSize(1.1);
    this.viewport.scene.add(this.transformControls.getHelper());
    this.transformControls.addEventListener("dragging-changed", (e) => {
      const dragging = Boolean(e.value);
      this.viewport.controls.enabled = !dragging;
      if (dragging) this.suppressNextTap = true;
    });
    this.transformControls.addEventListener("objectChange", () => {
      /* live during drag; nothing extra to sync since gizmo drives group transform directly */
    });

    this.bindHeader();
    this.bindPointerInput();
    this.updateToolRail();
    this.updateEditSelectRail();
    this.updateModifierPanel();
    this.updateOutliner();
    this.updateHint();
  }

  // ---------------------------------------------------------------- header
  private bindHeader(): void {
    const addMenu = $("add-menu");
    $("btn-add").addEventListener("click", (e) => {
      e.stopPropagation();
      addMenu.classList.toggle("hidden");
    });
    addMenu.querySelectorAll<HTMLButtonElement>("button[data-primitive]").forEach((btn) => {
      btn.addEventListener("click", () => {
        this.addPrimitive(btn.dataset.primitive as PrimitiveType);
        addMenu.classList.add("hidden");
      });
    });

    $("btn-mode").addEventListener("click", () => this.toggleMode());

    $("btn-view-front").addEventListener("click", () => this.setView("front"));
    $("btn-view-top").addEventListener("click", () => this.setView("top"));
    $("btn-view-right").addEventListener("click", () => this.setView("right"));
    $("btn-view-persp").addEventListener("click", () => this.setView("perspective"));

    $("btn-panel-toggle").addEventListener("click", () => $("side-panel").classList.toggle("open"));

    const addModifierMenu = $("add-modifier-menu");
    $("btn-add-modifier").addEventListener("click", (e) => {
      e.stopPropagation();
      addModifierMenu.classList.toggle("hidden");
    });
    addModifierMenu.querySelectorAll<HTMLButtonElement>("button[data-modifier]").forEach((btn) => {
      btn.addEventListener("click", () => {
        if (btn.dataset.modifier === "bevel" && this.selected) {
          this.selected.addModifier(createBevelModifier());
          this.updateModifierPanel();
        }
        addModifierMenu.classList.add("hidden");
      });
    });

    document.addEventListener("click", () => {
      addMenu.classList.add("hidden");
      addModifierMenu.classList.add("hidden");
    });

    window.addEventListener("keydown", (e) => {
      if (e.key === "Tab") {
        e.preventDefault();
        this.toggleMode();
      } else if (this.mode === "edit" && (e.key === "1" || e.key === "2" || e.key === "3")) {
        this.setSelectMode(e.key === "1" ? "vertex" : e.key === "2" ? "edge" : "face");
      } else if (this.mode === "edit" && (e.key === "x" || e.key === "Delete" || e.key === "Backspace")) {
        this.deleteSelected();
      } else if (this.mode === "edit" && e.key === "e") {
        this.extrudeSelected();
      } else if (this.mode === "object" && e.key === "g") {
        this.setTransformMode("translate");
      } else if (this.mode === "object" && e.key === "r") {
        this.setTransformMode("rotate");
      } else if (this.mode === "object" && e.key === "s") {
        this.setTransformMode("scale");
      }
    });
  }

  private setView(preset: ViewPreset): void {
    this.viewport.setView(preset);
  }

  // --------------------------------------------------------------- objects
  private addPrimitive(type: PrimitiveType): void {
    const mesh = generatePrimitive(type);
    const label: Record<PrimitiveType, string> = {
      cube: "立方体",
      plane: "平面",
      cylinder: "円柱",
      cone: "円錐",
      sphere: "球",
    };
    const obj = new SceneObject(mesh, `${label[type]}.${String(this.objects.length).padStart(3, "0")}`);
    this.objects.push(obj);
    this.viewport.objectRoot.add(obj.group);
    this.selectObject(obj);
    this.updateOutliner();
  }

  private selectObject(obj: SceneObject | null): void {
    this.objects.forEach((o) => o.setSelected(o === obj));
    this.selected = obj;
    if (this.mode === "object") {
      if (obj) this.transformControls.attach(obj.group);
      else this.transformControls.detach();
    }
    this.updateOutliner();
    this.updateModifierPanel();
  }

  private updateOutliner(): void {
    const root = $("outliner");
    root.innerHTML = "";
    this.objects.forEach((obj) => {
      const row = document.createElement("div");
      row.className = "outliner-item" + (obj === this.selected ? " active" : "");
      row.textContent = obj.name;
      row.addEventListener("click", () => this.selectObject(obj));
      root.appendChild(row);
    });
    if (this.objects.length === 0) {
      root.innerHTML = '<div class="muted">オブジェクトがありません</div>';
    }
  }

  // ------------------------------------------------------------------ mode
  private toggleMode(): void {
    if (!this.selected && this.mode === "object") return;
    this.mode = this.mode === "object" ? "edit" : "object";

    if (this.mode === "edit") {
      this.transformControls.detach();
      this.selected!.setEditMode(true);
      const mesh = this.selected!.base;
      if (mesh.selectedVerts.size === 0 && mesh.selectedEdges.size === 0 && mesh.selectedFaces.size === 0) {
        mesh.selectAll(this.selectMode);
        this.selected!.updateSelectionOverlay();
      }
    } else {
      this.objects.forEach((o) => o.setEditMode(false));
      if (this.selected) this.transformControls.attach(this.selected.group);
    }

    $("btn-mode").textContent = this.mode === "object" ? "オブジェクトモード" : "編集モード";
    this.updateToolRail();
    this.updateEditSelectRail();
    this.updateHint();
  }

  private setSelectMode(mode: SelectMode): void {
    this.selectMode = mode;
    if (this.selected) {
      this.selected.base.clearSelection();
      this.selected.base.selectAll(mode);
      this.selected.updateSelectionOverlay();
    }
    this.updateEditSelectRail();
    this.updateHint();
  }

  private setTransformMode(mode: TransformMode): void {
    this.transformMode = mode;
    this.transformControls.setMode(mode);
    this.updateToolRail();
  }

  // --------------------------------------------------------------- toolbar
  private updateToolRail(): void {
    const rail = $("tool-rail");
    rail.innerHTML = "";
    if (this.mode === "object") {
      const items: { key: TransformMode; label: string }[] = [
        { key: "translate", label: "移動" },
        { key: "rotate", label: "回転" },
        { key: "scale", label: "拡縮" },
      ];
      items.forEach((item) => {
        const btn = document.createElement("button");
        btn.textContent = item.label;
        btn.className = item.key === this.transformMode ? "active" : "";
        btn.addEventListener("click", () => this.setTransformMode(item.key));
        rail.appendChild(btn);
      });
    } else {
      const selectAllBtn = document.createElement("button");
      selectAllBtn.textContent = "全選択";
      selectAllBtn.addEventListener("click", () => {
        this.selected?.base.selectAll(this.selectMode);
        this.selected?.updateSelectionOverlay();
      });
      rail.appendChild(selectAllBtn);

      const deselectBtn = document.createElement("button");
      deselectBtn.textContent = "選択解除";
      deselectBtn.addEventListener("click", () => {
        this.selected?.base.clearSelection();
        this.selected?.updateSelectionOverlay();
      });
      rail.appendChild(deselectBtn);

      const extrudeBtn = document.createElement("button");
      extrudeBtn.textContent = "押し出し";
      extrudeBtn.addEventListener("click", () => this.extrudeSelected());
      rail.appendChild(extrudeBtn);

      const deleteBtn = document.createElement("button");
      deleteBtn.textContent = "削除";
      deleteBtn.addEventListener("click", () => this.deleteSelected());
      rail.appendChild(deleteBtn);
    }
  }

  private updateEditSelectRail(): void {
    const rail = $("edit-select-rail");
    rail.classList.toggle("hidden", this.mode !== "edit");
    rail.innerHTML = "";
    if (this.mode !== "edit") return;
    const items: { key: SelectMode; label: string }[] = [
      { key: "vertex", label: "頂点" },
      { key: "edge", label: "辺" },
      { key: "face", label: "面" },
    ];
    items.forEach((item) => {
      const btn = document.createElement("button");
      btn.textContent = item.label;
      btn.className = item.key === this.selectMode ? "active" : "";
      btn.addEventListener("click", () => this.setSelectMode(item.key));
      rail.appendChild(btn);
    });
  }

  private updateHint(): void {
    const hint = $("hint-bar");
    if (this.mode === "object") {
      hint.textContent = "指1本で回転・2本でパン/ズーム / タップで選択・ペンシルで正確に選択";
    } else {
      hint.textContent = `編集モード(${this.selectMode === "vertex" ? "頂点" : this.selectMode === "edge" ? "辺" : "面"}) — タップで選択・ドラッグで移動`;
    }
  }

  // ------------------------------------------------------------ modifiers
  private updateModifierPanel(): void {
    const empty = $("modifier-empty");
    const stack = $("modifier-stack");
    const addBtn = $("btn-add-modifier");
    stack.innerHTML = "";

    if (!this.selected) {
      empty.classList.remove("hidden");
      addBtn.classList.add("hidden");
      return;
    }
    empty.classList.add("hidden");
    addBtn.classList.remove("hidden");

    this.selected.modifiers.forEach((modifier) => {
      stack.appendChild(this.buildModifierCard(this.selected!, modifier));
    });
  }

  private buildModifierCard(obj: SceneObject, modifier: Modifier): HTMLElement {
    const card = document.createElement("div");
    card.className = "modifier-card";

    const header = document.createElement("div");
    header.className = "modifier-card-header";
    const title = document.createElement("span");
    title.textContent = `🔧 ${modifier.name}`;
    const removeBtn = document.createElement("button");
    removeBtn.textContent = "✕";
    removeBtn.addEventListener("click", () => {
      obj.removeModifier(modifier.id);
      this.updateModifierPanel();
    });
    header.append(title, removeBtn);
    card.appendChild(header);

    if (modifier.type === "bevel") {
      card.appendChild(this.buildBevelFields(obj, modifier as BevelModifier));
    }

    return card;
  }

  private buildBevelFields(obj: SceneObject, modifier: BevelModifier): HTMLElement {
    const wrap = document.createElement("div");
    const { params } = modifier;

    const widthRow = this.field("幅");
    const widthRange = document.createElement("input");
    widthRange.type = "range";
    widthRange.min = "0";
    widthRange.max = "0.5";
    widthRange.step = "0.001";
    widthRange.value = String(params.width);
    const widthNum = document.createElement("input");
    widthNum.type = "number";
    widthNum.step = "0.01";
    widthNum.value = params.width.toFixed(3);
    const syncWidth = (v: number) => {
      params.width = Math.max(0, v);
      widthRange.value = String(params.width);
      widthNum.value = params.width.toFixed(3);
      obj.recompute();
    };
    widthRange.addEventListener("input", () => syncWidth(parseFloat(widthRange.value)));
    widthNum.addEventListener("input", () => syncWidth(parseFloat(widthNum.value) || 0));
    widthRow.append(widthRange, widthNum);
    wrap.appendChild(widthRow);

    const segRow = this.field("セグメント");
    const segNum = document.createElement("input");
    segNum.type = "number";
    segNum.min = "1";
    segNum.max = "12";
    segNum.step = "1";
    segNum.value = String(params.segments);
    segNum.addEventListener("input", () => {
      params.segments = Math.max(1, Math.min(12, Math.round(parseFloat(segNum.value) || 1)));
      obj.recompute();
    });
    segRow.appendChild(segNum);
    wrap.appendChild(segRow);

    const limitRow = this.field("制限方法");
    const limitSelect = document.createElement("select");
    const options: { value: BevelLimitMethod; label: string }[] = [
      { value: "angle", label: "角度" },
      { value: "weight", label: "ウェイト" },
      { value: "none", label: "なし(全エッジ)" },
    ];
    options.forEach((o) => {
      const opt = document.createElement("option");
      opt.value = o.value;
      opt.textContent = o.label;
      if (o.value === params.limitMethod) opt.selected = true;
      limitSelect.appendChild(opt);
    });
    limitSelect.addEventListener("change", () => {
      params.limitMethod = limitSelect.value as BevelLimitMethod;
      obj.recompute();
      renderAngleRow();
    });
    limitRow.appendChild(limitSelect);
    wrap.appendChild(limitRow);

    const angleRow = this.field("角度しきい値");
    const angleRange = document.createElement("input");
    angleRange.type = "range";
    angleRange.min = "1";
    angleRange.max = "179";
    angleRange.step = "1";
    angleRange.value = String(Math.round(THREE.MathUtils.radToDeg(params.angle)));
    const angleNum = document.createElement("input");
    angleNum.type = "number";
    angleNum.value = angleRange.value;
    const syncAngle = (deg: number) => {
      params.angle = THREE.MathUtils.degToRad(deg);
      angleRange.value = String(deg);
      angleNum.value = String(deg);
      obj.recompute();
    };
    angleRange.addEventListener("input", () => syncAngle(parseFloat(angleRange.value)));
    angleNum.addEventListener("input", () => syncAngle(parseFloat(angleNum.value) || 0));
    angleRow.append(angleRange, angleNum);
    wrap.appendChild(angleRow);

    const renderAngleRow = () => {
      angleRow.classList.toggle("hidden", params.limitMethod !== "angle");
    };
    renderAngleRow();

    const clampRow = this.field("重なり防止");
    clampRow.classList.add("checkbox");
    const clampCheck = document.createElement("input");
    clampCheck.type = "checkbox";
    clampCheck.checked = params.clampOverlap;
    clampCheck.addEventListener("change", () => {
      params.clampOverlap = clampCheck.checked;
      obj.recompute();
    });
    clampRow.appendChild(clampCheck);
    wrap.appendChild(clampRow);

    return wrap;
  }

  private field(label: string): HTMLDivElement {
    const row = document.createElement("div");
    row.className = "field-row";
    const l = document.createElement("label");
    l.textContent = label;
    row.appendChild(l);
    return row;
  }

  // ------------------------------------------------------------- edit ops
  private extrudeSelected(): void {
    if (!this.selected || this.mode !== "edit" || this.selectMode !== "face") return;
    const mesh = this.selected.base;
    const facesToExtrude = [...mesh.selectedFaces];
    if (facesToExtrude.length === 0) return;

    const distance = 0.4;
    const keptFaces = mesh.faces.filter((_, i) => !mesh.selectedFaces.has(i));
    const addedFaces: number[][] = [];
    const capMarkers: number[] = [];

    for (const fi of facesToExtrude) {
      const face = mesh.faces[fi];
      const normal = mesh.faceNormal(fi);
      const newIndices = face.map((vi) => {
        const p = mesh.vertices[vi].clone().addScaledVector(normal, distance);
        const idx = mesh.vertices.length;
        mesh.vertices.push(p);
        return idx;
      });
      const n = face.length;
      for (let i = 0; i < n; i++) {
        const a = face[i];
        const b = face[(i + 1) % n];
        const na = newIndices[i];
        const nb = newIndices[(i + 1) % n];
        addedFaces.push([a, b, nb, na]);
      }
      capMarkers.push(addedFaces.length);
      addedFaces.push(newIndices);
    }

    const baseOffset = keptFaces.length;
    mesh.faces = [...keptFaces, ...addedFaces];
    mesh.selectedVerts.clear();
    mesh.selectedEdges.clear();
    mesh.selectedFaces = new Set(capMarkers.map((m) => baseOffset + m));
    this.selected.recompute();
  }

  private deleteSelected(): void {
    if (!this.selected || this.mode !== "edit") return;
    const mesh = this.selected.base;

    if (this.selectMode === "face") {
      mesh.faces = mesh.faces.filter((_, i) => !mesh.selectedFaces.has(i));
    } else {
      const doomedVerts = new Set<number>(mesh.selectedVerts);
      if (this.selectMode === "edge") {
        const edges = mesh.computeEdges();
        mesh.selectedEdges.forEach((key) => {
          const info = edges.get(key);
          if (info) {
            doomedVerts.add(info.v0);
            doomedVerts.add(info.v1);
          }
        });
      }
      mesh.faces = mesh.faces.filter((face) => !face.some((vi) => doomedVerts.has(vi)));
    }

    mesh.clearSelection();
    this.selected.recompute();
  }

  // ------------------------------------------------------------ pointers
  private bindPointerInput(): void {
    const dom = this.viewport.renderer.domElement;
    dom.addEventListener("pointerdown", (e) => this.onPointerDown(e));
    dom.addEventListener("pointermove", (e) => this.onPointerMove(e));
    dom.addEventListener("pointerup", (e) => this.onPointerUp(e));
    dom.addEventListener("pointercancel", (e) => this.onPointerUp(e));
  }

  private ndc(e: PointerEvent): THREE.Vector2 {
    const rect = this.viewport.renderer.domElement.getBoundingClientRect();
    return new THREE.Vector2(((e.clientX - rect.left) / rect.width) * 2 - 1, -((e.clientY - rect.top) / rect.height) * 2 + 1);
  }

  private worldToScreen(v: THREE.Vector3): { x: number; y: number } {
    const rect = this.viewport.renderer.domElement.getBoundingClientRect();
    const p = v.clone().project(this.viewport.camera);
    return { x: rect.left + ((p.x + 1) / 2) * rect.width, y: rect.top + ((1 - p.y) / 2) * rect.height };
  }

  private hitThreshold(e: PointerEvent): number {
    if (e.pointerType === "pen") return 14;
    if (e.pointerType === "touch") return 28;
    return 10;
  }

  private findEditHit(obj: SceneObject, e: PointerEvent): EditHit | null {
    const threshold = this.hitThreshold(e);
    const px = e.clientX;
    const py = e.clientY;
    let best: EditHit | null = null;
    let bestDist = threshold;

    const toWorld = (local: THREE.Vector3) => local.clone().applyMatrix4(obj.group.matrixWorld);

    if (this.selectMode === "vertex") {
      obj.base.vertices.forEach((v, i) => {
        const s = this.worldToScreen(toWorld(v));
        const d = Math.hypot(s.x - px, s.y - py);
        if (d < bestDist) {
          bestDist = d;
          best = { kind: "vertex", key: i };
        }
      });
    } else if (this.selectMode === "edge") {
      obj.base.computeEdges().forEach((info: EdgeInfo, key: string) => {
        const a = this.worldToScreen(toWorld(obj.base.vertices[info.v0]));
        const b = this.worldToScreen(toWorld(obj.base.vertices[info.v1]));
        const d = this.pointSegmentDistance(px, py, a.x, a.y, b.x, b.y);
        if (d < bestDist) {
          bestDist = d;
          best = { kind: "edge", key };
        }
      });
    } else {
      obj.base.faces.forEach((_face, i) => {
        const s = this.worldToScreen(toWorld(obj.base.faceCentroid(i)));
        const d = Math.hypot(s.x - px, s.y - py);
        if (d < bestDist) {
          bestDist = d;
          best = { kind: "face", key: i };
        }
      });
    }
    return best;
  }

  private pointSegmentDistance(px: number, py: number, ax: number, ay: number, bx: number, by: number): number {
    const dx = bx - ax;
    const dy = by - ay;
    const lenSq = dx * dx + dy * dy;
    let t = lenSq > 0 ? ((px - ax) * dx + (py - ay) * dy) / lenSq : 0;
    t = Math.max(0, Math.min(1, t));
    return Math.hypot(px - (ax + t * dx), py - (ay + t * dy));
  }

  private onPointerDown(e: PointerEvent): void {
    if (!e.isPrimary) return;
    this.tapDown = { x: e.clientX, y: e.clientY, time: performance.now() };

    if (this.mode === "edit" && this.selected) {
      const hit = this.findEditHit(this.selected, e);
      if (hit) {
        this.applyEditHitToSelection(hit);
        this.startEditDrag(this.selected, e);
      }
    }
  }

  private applyEditHitToSelection(hit: EditHit): void {
    const mesh = this.selected!.base;
    const alreadySelected =
      (hit.kind === "vertex" && mesh.selectedVerts.has(hit.key as number)) ||
      (hit.kind === "edge" && mesh.selectedEdges.has(hit.key as string)) ||
      (hit.kind === "face" && mesh.selectedFaces.has(hit.key as number));

    if (!alreadySelected) {
      mesh.clearSelection();
      if (hit.kind === "vertex") mesh.selectedVerts.add(hit.key as number);
      else if (hit.kind === "edge") mesh.selectedEdges.add(hit.key as string);
      else mesh.selectedFaces.add(hit.key as number);
    }
    this.selected!.updateSelectionOverlay();
  }

  private collectDragVertices(obj: SceneObject): Set<number> {
    const mesh = obj.base;
    const verts = new Set<number>();
    if (this.selectMode === "vertex") {
      mesh.selectedVerts.forEach((v) => verts.add(v));
    } else if (this.selectMode === "edge") {
      const edges = mesh.computeEdges();
      mesh.selectedEdges.forEach((key) => {
        const info = edges.get(key);
        if (info) {
          verts.add(info.v0);
          verts.add(info.v1);
        }
      });
    } else {
      mesh.selectedFaces.forEach((fi) => mesh.faces[fi].forEach((v) => verts.add(v)));
    }
    return verts;
  }

  private startEditDrag(obj: SceneObject, e: PointerEvent): void {
    const vertexIndices = this.collectDragVertices(obj);
    if (vertexIndices.size === 0) return;

    const worldPositions: THREE.Vector3[] = [];
    vertexIndices.forEach((vi) => worldPositions.push(obj.base.vertices[vi].clone().applyMatrix4(obj.group.matrixWorld)));
    const center = worldPositions.reduce((acc, v) => acc.add(v), new THREE.Vector3()).divideScalar(worldPositions.length);

    const camDir = new THREE.Vector3();
    this.viewport.camera.getWorldDirection(camDir);
    const plane = new THREE.Plane().setFromNormalAndCoplanarPoint(camDir, center);

    const startPoint = new THREE.Vector3();
    this.raycaster.setFromCamera(this.ndc(e), this.viewport.camera);
    if (!this.raycaster.ray.intersectPlane(plane, startPoint)) return;

    const startLocal = new Map<number, THREE.Vector3>();
    vertexIndices.forEach((vi) => startLocal.set(vi, obj.base.vertices[vi].clone()));

    this.viewport.controls.enabled = false;
    this.dragState = { pointerId: e.pointerId, obj, plane, startWorldPoint: startPoint, startLocal };
  }

  private onPointerMove(e: PointerEvent): void {
    if (!this.dragState || e.pointerId !== this.dragState.pointerId) return;
    const { obj, plane, startWorldPoint, startLocal } = this.dragState;

    this.raycaster.setFromCamera(this.ndc(e), this.viewport.camera);
    const current = new THREE.Vector3();
    if (!this.raycaster.ray.intersectPlane(plane, current)) return;
    const worldDelta = current.clone().sub(startWorldPoint);

    const invMatrix = new THREE.Matrix4().copy(obj.group.matrixWorld).invert();
    const originLocal = new THREE.Vector3(0, 0, 0).applyMatrix4(invMatrix);
    const deltaLocal = worldDelta.clone().applyMatrix4(invMatrix).sub(originLocal);

    startLocal.forEach((start, vi) => {
      obj.base.vertices[vi].copy(start).add(deltaLocal);
    });
    obj.recompute();
  }

  private onPointerUp(e: PointerEvent): void {
    if (this.dragState && e.pointerId === this.dragState.pointerId) {
      this.dragState = null;
      this.viewport.controls.enabled = true;
      return;
    }
    if (!e.isPrimary || !this.tapDown) return;

    const dx = e.clientX - this.tapDown.x;
    const dy = e.clientY - this.tapDown.y;
    const moved = Math.hypot(dx, dy);
    const elapsed = performance.now() - this.tapDown.time;
    this.tapDown = null;
    if (moved > 8 || elapsed > 500) return;

    if (this.suppressNextTap) {
      this.suppressNextTap = false;
      return;
    }

    if (this.mode === "object") {
      this.raycaster.setFromCamera(this.ndc(e), this.viewport.camera);
      const hits = this.raycaster.intersectObjects(this.objects.map((o) => o.renderMesh));
      if (hits.length > 0) {
        const hitObj = hits[0].object.userData.sceneObject as SceneObject;
        this.selectObject(hitObj);
      } else {
        this.selectObject(null);
      }
    } else if (this.selected) {
      const hit = this.findEditHit(this.selected, e);
      if (!hit) {
        this.selected.base.clearSelection();
        this.selected.updateSelectionOverlay();
      }
    }
  }
}
