import "./style.css";
import { App } from "./App";

const app = new App();
if (import.meta.env.DEV) {
  (window as unknown as { __app: App }).__app = app;
}
