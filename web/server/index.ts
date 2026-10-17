import { capsule } from "lakebed/server";

// The globe runs entirely in the browser: the Rust core ships as wasm inside
// the client bundle, so the server has no data or endpoints.
export default capsule({
  name: "cesium-rs",
  auth: { requireSignIn: false },
});
