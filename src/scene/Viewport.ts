import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";

export type ViewPreset = "front" | "top" | "right" | "perspective";

/** Renderer + camera + grid + lighting + navigation controls for the 3D viewport. */
export class Viewport {
  readonly renderer: THREE.WebGLRenderer;
  readonly scene: THREE.Scene;
  readonly camera: THREE.PerspectiveCamera;
  readonly controls: OrbitControls;
  readonly objectRoot: THREE.Group;
  private container: HTMLElement;

  constructor(container: HTMLElement) {
    this.container = container;
    this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    this.renderer.setClearColor(0x2b2b2f, 1);
    container.appendChild(this.renderer.domElement);
    this.renderer.domElement.style.touchAction = "none";

    this.scene = new THREE.Scene();

    this.camera = new THREE.PerspectiveCamera(50, 1, 0.01, 1000);
    this.camera.position.set(4, 3.5, 5);

    this.controls = new OrbitControls(this.camera, this.renderer.domElement);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.12;
    this.controls.touches = { ONE: THREE.TOUCH.ROTATE, TWO: THREE.TOUCH.DOLLY_PAN };
    this.controls.mouseButtons = {
      LEFT: THREE.MOUSE.ROTATE,
      MIDDLE: THREE.MOUSE.DOLLY,
      RIGHT: THREE.MOUSE.PAN,
    };
    this.controls.target.set(0, 0.5, 0);
    this.controls.update();

    const grid = new THREE.GridHelper(20, 20, 0x555555, 0x3d3d40);
    (grid.material as THREE.Material).transparent = true;
    (grid.material as THREE.Material).opacity = 0.6;
    this.scene.add(grid);

    const hemi = new THREE.HemisphereLight(0xffffff, 0x30302f, 1.1);
    this.scene.add(hemi);
    const dir = new THREE.DirectionalLight(0xffffff, 1.4);
    dir.position.set(5, 8, 6);
    this.scene.add(dir);

    this.objectRoot = new THREE.Group();
    this.scene.add(this.objectRoot);

    window.addEventListener("resize", () => this.resize());
    this.resize();
    this.animate();
  }

  private resize(): void {
    const w = this.container.clientWidth;
    const h = this.container.clientHeight;
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    this.renderer.setSize(w, h);
  }

  private animate = (): void => {
    requestAnimationFrame(this.animate);
    this.controls.update();
    this.renderer.render(this.scene, this.camera);
  };

  /** Blender-style numpad view presets (1 / 7 / 3 / 0), exposed as on-screen buttons. */
  setView(preset: ViewPreset): void {
    const target = this.controls.target.clone();
    const distance = this.camera.position.distanceTo(target) || 6;
    let dir: THREE.Vector3;
    switch (preset) {
      case "front":
        dir = new THREE.Vector3(0, 0, 1);
        break;
      case "top":
        dir = new THREE.Vector3(0, 1, 0.0001);
        break;
      case "right":
        dir = new THREE.Vector3(1, 0, 0);
        break;
      default:
        dir = new THREE.Vector3(1, 0.7, 1).normalize();
    }
    this.camera.position.copy(target.clone().addScaledVector(dir, distance));
    this.camera.up.set(0, 1, 0);
    this.camera.lookAt(target);
    this.controls.update();
  }

  frameObject(object: THREE.Object3D): void {
    const box = new THREE.Box3().setFromObject(object);
    if (box.isEmpty()) return;
    const size = box.getSize(new THREE.Vector3()).length();
    const center = box.getCenter(new THREE.Vector3());
    const direction = this.camera.position.clone().sub(this.controls.target).normalize();
    this.controls.target.copy(center);
    this.camera.position.copy(center.clone().addScaledVector(direction, Math.max(size, 1.5)));
    this.controls.update();
  }
}
