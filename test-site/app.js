// CesiumRS Full-Feature Live Testbed Frontend
// Drives examples/live_testbed.rs compiled via wasm-bindgen into ./pkg/cesium_rs_testbed.js

import init, {
  BrowserCore,
  BrowserPinBuilder,
  BrowserScreenSpaceEventHandler,
  LiveGlobeApp,
  LiveModelViewer,
  core_astronomy_inspect,
  core_geodesy_compare,
  core_spatial_inspect,
  eval_time_properties,
  evaluate_tile_style,
  export_kml_from_geojson,
  parse_czml,
  parse_geojson,
  parse_gpx,
  parse_kml,
  render_brdf_lut_preview,
  render_sun_preview,
  renderer_atlas_demo,
  renderer_capabilities_report,
  renderer_compute_demo,
  renderer_run_gpu_suite,
  run_auto_exposure_preview,
  shaders_get,
  shaders_list,
  shaders_test_compile,
  worker_codecs_test,
  worker_gaussian_splats,
  worker_geometry_build,
  worker_geometry_list,
  worker_terrain_build,
  writeTextToCanvas,
} from "./pkg/cesium_rs_testbed.js";

const RAD = Math.PI / 180;
const $ = (id) => document.getElementById(id);

function setStatus(kind, label) {
  const dot = $("wasm-dot");
  dot.className = "status-dot" + (kind === "ok" ? " ok" : kind === "err" ? " err" : "");
  $("wasm-label").textContent = label;
}

function hexToRgba(hex) {
  const h = hex.replace("#", "");
  return [
    parseInt(h.slice(0, 2), 16) / 255,
    parseInt(h.slice(2, 4), 16) / 255,
    parseInt(h.slice(4, 6), 16) / 255,
    1.0,
  ];
}

function clamp(v, lo, hi) { return Math.min(hi, Math.max(lo, v)); }
function wrapLon(lon) { return ((((lon + Math.PI) % (2 * Math.PI)) + 2 * Math.PI) % (2 * Math.PI)) - Math.PI; }

// ============================================================================
// Sample DataSource Documents
// ============================================================================

const DS_SAMPLES = {
  czml: JSON.stringify([
    {
      id: "document",
      name: "CesiumRS Orbital Satellite",
      version: "1.0",
      clock: {
        interval: "2026-06-21T12:00:00Z/2026-06-21T13:30:00Z",
        currentTime: "2026-06-21T12:30:00Z",
        multiplier: 60,
      },
    },
    {
      id: "iss-demo",
      name: "CesiumRS Demo Satellite",
      availability: "2026-06-21T12:00:00Z/2026-06-21T13:30:00Z",
      position: {
        epoch: "2026-06-21T12:00:00Z",
        interpolationAlgorithm: "LAGRANGE",
        interpolationDegree: 3,
        cartographicDegrees: [
          0, -120.0, 22.0, 420000,
          900, -85.0, 42.0, 422000,
          1800, -40.0, 51.5, 425000,
          2700, 10.0, 46.0, 421000,
          3600, 55.0, 25.0, 418000,
          4500, 95.0, -5.0, 420000,
          5400, 135.0, -32.0, 423000,
        ],
      },
      point: { pixelSize: 10, color: { rgba: [56, 214, 245, 255] } },
      path: { width: 3 },
    },
    {
      id: "ground-station",
      name: "Boulder Ground Station",
      position: { cartographicDegrees: [-105.27, 40.01, 1655] },
      point: { pixelSize: 9, color: { rgba: [240, 173, 48, 255] } },
    },
  ], null, 2),

  geojson: JSON.stringify({
    type: "FeatureCollection",
    features: [
      {
        type: "Feature",
        properties: { name: "Trans-Atlantic Corridor", category: "route" },
        geometry: {
          type: "LineString",
          coordinates: [
            [-74.006, 40.7128, 12000],
            [-45.0, 48.0, 12000],
            [-0.1276, 51.5074, 12000],
            [13.405, 52.52, 12000],
          ],
        },
      },
      {
        type: "Feature",
        properties: { name: "Colorado Test Zone" },
        geometry: {
          type: "Polygon",
          coordinates: [[
            [-109.05, 37.0],
            [-102.05, 37.0],
            [-102.05, 41.0],
            [-109.05, 41.0],
            [-109.05, 37.0],
          ]],
        },
      },
      {
        type: "Feature",
        properties: { name: "Zurich Research Hub" },
        geometry: { type: "Point", coordinates: [8.5417, 47.3769, 408] },
      },
    ],
  }, null, 2),

  kml: `<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Document>
    <name>CesiumRS KML Sample</name>
    <Placemark id="pm-alps">
      <name>Mont Blanc</name>
      <description>Highest peak in the Alps</description>
      <Point><coordinates>6.8652,45.8326,4808</coordinates></Point>
    </Placemark>
    <Placemark id="pm-path">
      <name>Alpine Ridge Path</name>
      <LineString>
        <coordinates>
          6.8652,45.8326,4808
          7.6586,45.9763,4478
          8.5417,47.3769,408
          11.4041,47.2692,574
        </coordinates>
      </LineString>
    </Placemark>
  </Document>
</kml>`,

  gpx: `<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="CesiumRS Live Testbed" xmlns="http://www.topografix.com/GPX/1/1">
  <metadata><name>Pacific Coast Flight Track</name></metadata>
  <wpt lat="37.7749" lon="-122.4194"><ele>50</ele><name>San Francisco</name></wpt>
  <wpt lat="47.6062" lon="-122.3321"><ele>120</ele><name>Seattle</name></wpt>
  <trk>
    <name>West Coast Corridor</name>
    <trkseg>
      <trkpt lat="34.0522" lon="-118.2437"><ele>1500</ele></trkpt>
      <trkpt lat="37.7749" lon="-122.4194"><ele>3200</ele></trkpt>
      <trkpt lat="42.3265" lon="-122.8756"><ele>4100</ele></trkpt>
      <trkpt lat="45.5152" lon="-122.6784"><ele>3500</ele></trkpt>
      <trkpt lat="47.6062" lon="-122.3321"><ele>1200</ele></trkpt>
    </trkseg>
  </trk>
</gpx>`,
};

// ============================================================================
// Lightweight WebGL2 Turntable Renderer for Workers / Terrain / Splats Canvases
// ============================================================================

function makeMat4Perspective(fovRad, aspect, near, far) {
  const f = 1 / Math.tan(fovRad / 2);
  const nf = 1 / (near - far);
  return new Float32Array([
    f / aspect, 0, 0, 0,
    0, f, 0, 0,
    0, 0, (far + near) * nf, -1,
    0, 0, 2 * far * near * nf, 0,
  ]);
}

function makeMat4LookAt(eye, target, up) {
  const zx = eye[0] - target[0], zy = eye[1] - target[1], zz = eye[2] - target[2];
  const zl = Math.hypot(zx, zy, zz) || 1;
  const z = [zx / zl, zy / zl, zz / zl];
  const xx = up[1] * z[2] - up[2] * z[1], xy = up[2] * z[0] - up[0] * z[2], xz = up[0] * z[1] - up[1] * z[0];
  const xl = Math.hypot(xx, xy, xz) || 1;
  const x = [xx / xl, xy / xl, xz / xl];
  const y = [z[1] * x[2] - z[2] * x[1], z[2] * x[0] - z[0] * x[2], z[0] * x[1] - z[1] * x[0]];
  return new Float32Array([
    x[0], y[0], z[0], 0,
    x[1], y[1], z[1], 0,
    x[2], y[2], z[2], 0,
    -(x[0] * eye[0] + x[1] * eye[1] + x[2] * eye[2]),
    -(y[0] * eye[0] + y[1] * eye[1] + y[2] * eye[2]),
    -(z[0] * eye[0] + z[1] * eye[1] + z[2] * eye[2]),
    1,
  ]);
}

function mulMat4(a, b) {
  const out = new Float32Array(16);
  for (let c = 0; c < 4; c++) {
    for (let r = 0; r < 4; r++) {
      out[c * 4 + r] = a[r] * b[c * 4] + a[4 + r] * b[c * 4 + 1] + a[8 + r] * b[c * 4 + 2] + a[12 + r] * b[c * 4 + 3];
    }
  }
  return out;
}

function compileGL2(gl, vsSrc, fsSrc) {
  const prog = gl.createProgram();
  for (const [type, src] of [[gl.VERTEX_SHADER, vsSrc], [gl.FRAGMENT_SHADER, fsSrc]]) {
    const s = gl.createShader(type);
    gl.shaderSource(s, src);
    gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
    gl.attachShader(prog, s);
  }
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(prog));
  return prog;
}

class MiniMeshViewport {
  constructor(canvas, mode = "mesh") {
    this.canvas = canvas;
    this.mode = mode;
    this.gl = canvas.getContext("webgl2", { antialias: true });
    this.yaw = 0.65;
    this.pitch = 0.45;
    this.dist = 3.1;
    this.wireframe = false;
    this.isLines = false;
    this.count = 0;
    this.wireCount = 0;
    this.vbo = null;
    this.ibo = null;
    this.wibo = null;
    this.colVbo = null;
    this.setupProgram();
    this.attachDrag();
  }

  setupProgram() {
    const gl = this.gl;
    if (!gl) return;
    if (this.mode === "terrain") {
      this.prog = compileGL2(gl, `#version 300 es
        layout(location=0) in vec3 a_pos;
        layout(location=1) in vec3 a_norm;
        layout(location=2) in float a_h;
        uniform mat4 u_mvp;
        out vec3 v_norm; out float v_h;
        void main(){ v_norm = a_norm; v_h = a_h; gl_Position = u_mvp * vec4(a_pos, 1.0); }`,
        `#version 300 es
        precision highp float;
        in vec3 v_norm; in float v_h; out vec4 out_c;
        void main(){
          vec3 low = vec3(0.09, 0.28, 0.44);
          vec3 mid = vec3(0.18, 0.56, 0.34);
          vec3 hi = vec3(0.68, 0.58, 0.44);
          vec3 snow = vec3(0.95, 0.97, 1.0);
          vec3 base = v_h < 0.35 ? mix(low, mid, v_h/0.35)
                    : v_h < 0.72 ? mix(mid, hi, (v_h-0.35)/0.37)
                    : mix(hi, snow, (v_h-0.72)/0.28);
          float l = 0.25 + 0.75 * max(dot(normalize(v_norm), normalize(vec3(0.6, 0.85, 0.45))), 0.0);
          out_c = vec4(base * l, 1.0);
        }`);
    } else if (this.mode === "splats") {
      this.prog = compileGL2(gl, `#version 300 es
        layout(location=0) in vec3 a_pos;
        layout(location=1) in vec4 a_col;
        uniform mat4 u_mvp;
        out vec4 v_col;
        void main(){
          gl_Position = u_mvp * vec4(a_pos, 1.0);
          gl_PointSize = clamp(24.0 / max(gl_Position.w, 0.4), 2.5, 16.0);
          v_col = a_col;
        }`,
        `#version 300 es
        precision highp float;
        in vec4 v_col; out vec4 out_c;
        void main(){
          vec2 d = gl_PointCoord * 2.0 - 1.0;
          float r2 = dot(d, d);
          if (r2 > 1.0) discard;
          float g = exp(-r2 * 3.2);
          out_c = vec4(v_col.rgb, v_col.a * g);
        }`);
    } else {
      this.prog = compileGL2(gl, `#version 300 es
        layout(location=0) in vec3 a_pos;
        layout(location=1) in vec3 a_norm;
        uniform mat4 u_mvp;
        out vec3 v_norm;
        void main(){ v_norm = a_norm; gl_Position = u_mvp * vec4(a_pos, 1.0); }`,
        `#version 300 es
        precision highp float;
        uniform vec4 u_col;
        in vec3 v_norm; out vec4 out_c;
        void main(){
          float l = 0.30 + 0.70 * abs(dot(normalize(v_norm), normalize(vec3(0.55, 0.75, 0.45))));
          out_c = vec4(u_col.rgb * l, u_col.a);
        }`);
    }
  }

  attachDrag() {
    let dragging = false, lx = 0, ly = 0;
    this.canvas.addEventListener("pointerdown", (e) => { dragging = true; lx = e.clientX; ly = e.clientY; this.canvas.setPointerCapture(e.pointerId); });
    this.canvas.addEventListener("pointermove", (e) => {
      if (!dragging) return;
      this.yaw += (e.clientX - lx) * 0.01;
      this.pitch = clamp(this.pitch + (e.clientY - ly) * 0.01, -1.4, 1.4);
      lx = e.clientX; ly = e.clientY;
    });
    this.canvas.addEventListener("pointerup", () => { dragging = false; });
    this.canvas.addEventListener("wheel", (e) => { e.preventDefault(); this.dist = clamp(this.dist * Math.exp(e.deltaY * 0.001), 1.4, 10.0); }, { passive: false });
  }

  setGeometry(interleaved, indices, wireIndices, isLines) {
    const gl = this.gl;
    if (!gl) return;
    if (this.vbo) gl.deleteBuffer(this.vbo);
    if (this.ibo) gl.deleteBuffer(this.ibo);
    if (this.wibo) gl.deleteBuffer(this.wibo);
    this.vbo = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, this.vbo);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(interleaved), gl.STATIC_DRAW);
    this.ibo = gl.createBuffer();
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.ibo);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, new Uint32Array(indices), gl.STATIC_DRAW);
    this.count = indices.length;
    this.isLines = isLines;
    if (wireIndices && wireIndices.length) {
      this.wibo = gl.createBuffer();
      gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.wibo);
      gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, new Uint32Array(wireIndices), gl.STATIC_DRAW);
      this.wireCount = wireIndices.length;
    } else {
      this.wibo = null;
      this.wireCount = 0;
    }
  }

  setSplats(positions, colors, count) {
    const gl = this.gl;
    if (!gl) return;
    if (this.vbo) gl.deleteBuffer(this.vbo);
    if (this.colVbo) gl.deleteBuffer(this.colVbo);
    this.vbo = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, this.vbo);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(positions), gl.STATIC_DRAW);
    this.colVbo = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, this.colVbo);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(colors), gl.STATIC_DRAW);
    this.count = count;
  }

  render(autoSpin = 0) {
    const gl = this.gl;
    if (!gl || !this.vbo || this.count === 0) return;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const w = Math.max(1, Math.round(this.canvas.clientWidth * dpr));
    const h = Math.max(1, Math.round(this.canvas.clientHeight * dpr));
    if (this.canvas.width !== w || this.canvas.height !== h) {
      this.canvas.width = w;
      this.canvas.height = h;
    }
    this.yaw += autoSpin;
    gl.viewport(0, 0, w, h);
    gl.clearColor(0.03, 0.045, 0.075, 1);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);

    const cp = Math.cos(this.pitch);
    const eye = [this.dist * cp * Math.sin(this.yaw), this.dist * Math.sin(this.pitch), this.dist * cp * Math.cos(this.yaw)];
    const mvp = mulMat4(makeMat4Perspective(45 * RAD, w / h, 0.1, 40), makeMat4LookAt(eye, [0, 0, 0], [0, 1, 0]));
    gl.useProgram(this.prog);
    gl.uniformMatrix4fv(gl.getUniformLocation(this.prog, "u_mvp"), false, mvp);

    if (this.mode === "terrain") {
      gl.enable(gl.DEPTH_TEST);
      gl.disable(gl.BLEND);
      gl.bindBuffer(gl.ARRAY_BUFFER, this.vbo);
      gl.enableVertexAttribArray(0); gl.vertexAttribPointer(0, 3, gl.FLOAT, false, 28, 0);
      gl.enableVertexAttribArray(1); gl.vertexAttribPointer(1, 3, gl.FLOAT, false, 28, 12);
      gl.enableVertexAttribArray(2); gl.vertexAttribPointer(2, 1, gl.FLOAT, false, 28, 24);
      gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.ibo);
      gl.drawElements(gl.TRIANGLES, this.count, gl.UNSIGNED_INT, 0);
    } else if (this.mode === "splats") {
      gl.disable(gl.DEPTH_TEST);
      gl.enable(gl.BLEND);
      gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
      gl.bindBuffer(gl.ARRAY_BUFFER, this.vbo);
      gl.enableVertexAttribArray(0); gl.vertexAttribPointer(0, 3, gl.FLOAT, false, 12, 0);
      gl.bindBuffer(gl.ARRAY_BUFFER, this.colVbo);
      gl.enableVertexAttribArray(1); gl.vertexAttribPointer(1, 4, gl.FLOAT, false, 16, 0);
      gl.drawArrays(gl.POINTS, 0, this.count);
    } else {
      gl.enable(gl.DEPTH_TEST);
      gl.disable(gl.BLEND);
      gl.bindBuffer(gl.ARRAY_BUFFER, this.vbo);
      gl.enableVertexAttribArray(0); gl.vertexAttribPointer(0, 3, gl.FLOAT, false, 24, 0);
      gl.enableVertexAttribArray(1); gl.vertexAttribPointer(1, 3, gl.FLOAT, false, 24, 12);
      const useWire = this.wireframe && this.wibo && !this.isLines;
      gl.uniform4fv(gl.getUniformLocation(this.prog, "u_col"), useWire || this.isLines ? [0.35, 0.88, 1.0, 1.0] : [0.94, 0.68, 0.22, 1.0]);
      gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, useWire ? this.wibo : this.ibo);
      gl.drawElements(useWire || this.isLines ? gl.LINES : gl.TRIANGLES, useWire ? this.wireCount : this.count, gl.UNSIGNED_INT, 0);
    }
  }
}

// ============================================================================
// Main Bootstrap
// ============================================================================

async function main() {
  try {
    await init();
    setStatus("ok", "WASM Online · WebGL2");
  } catch (e) {
    setStatus("err", "WASM init failed");
    console.error(e);
    alert("Failed to initialize CesiumRS WASM: " + e);
    return;
  }

  let activeTab = "globe";
  document.querySelectorAll("#main-tabs .tab-btn").forEach((btn) => {
    btn.addEventListener("click", () => {
      activeTab = btn.dataset.tab;
      document.querySelectorAll("#main-tabs .tab-btn").forEach((b) => b.classList.toggle("active", b === btn));
      document.querySelectorAll(".panel-view").forEach((p) => p.classList.toggle("active", p.id === `tab-${activeTab}`));
      onTabActivated(activeTab);
    });
  });

  // --------------------------------------------------------------------------
  // 1. LIVE GLOBE & SCENE
  // --------------------------------------------------------------------------
  const globeCanvas = $("globe-canvas");
  const globeGl = globeCanvas.getContext("webgl2", { antialias: true, alpha: false, preserveDrawingBuffer: false });
  if (!globeGl) {
    setStatus("err", "WebGL2 required");
    return;
  }
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  globeCanvas.width = Math.max(1, Math.round(globeCanvas.clientWidth * dpr));
  globeCanvas.height = Math.max(1, Math.round(globeCanvas.clientHeight * dpr));
  const globe = new LiveGlobeApp(globeGl, globeCanvas.width, globeCanvas.height, dpr);

  // Load Natural Earth II texture via createImageBitmap -> Texture::from_image
  fetch("/assets/natural-earth-ii-2048.jpg")
    .then((r) => r.blob())
    .then((b) => createImageBitmap(b))
    .then((bmp) => {
      globe.setEarthImage(bmp, bmp.width, bmp.height);
      bmp.close();
    })
    .catch((e) => console.warn("Earth texture load failed:", e));

  const cam = { lon: -40 * RAD, lat: 28 * RAD, heading: 0, pitch: -90 * RAD, range: 19500e3, fov: 50 };
  let bodyRadius = 6378137;
  let flight = null;
  let measuring = false;
  let measureStart = null;
  let clockRunning = true;
  let clockSpeedIdx = 2;
  const clockSpeeds = [1, 60, 3600, 21600, 86400];
  let simTimeMs = Date.now();
  let lastPerfMs = performance.now();
  let lastAstroMs = 0;

  function flyTo(target, dur = 1200) {
    const from = { ...cam };
    const to = { ...from, ...target };
    to.lon = from.lon + wrapLon(to.lon - from.lon);
    to.heading = from.heading + wrapLon(to.heading - from.heading);
    flight = { from, to, start: performance.now(), dur };
  }

  function applyLayers() {
    const schemeBtn = document.querySelector("#tile-scheme-row button.active");
    globe.setLayers(
      $("chk-graticule").checked,
      $("chk-tiles").checked,
      Number(schemeBtn?.dataset.scheme || 0),
      $("chk-sun").checked,
      $("chk-atmo").checked,
      $("chk-stars").checked,
      $("chk-rhumb").checked,
      $("chk-spline").checked,
    );
  }

  function applyPostProcess() {
    const exp = Number($("pp-exposure").value);
    $("pp-exp-val").textContent = exp.toFixed(1);
    globe.setPostProcess($("pp-effect").value, $("pp-fxaa").checked, $("pp-bloom").checked, $("pp-tonemapper").value, exp);
  }

  function applyClipping() {
    const d = Number($("clip-dist").value);
    const a = Number($("clip-ang").value);
    $("clip-dist-val").textContent = d.toFixed(2);
    $("clip-ang-val").textContent = `${a}°`;
    globe.setClipping($("chk-clip").checked, d, a);
  }

  function setRouteAndReadout(lon1, lat1, lon2, lat2, fly = false) {
    const res = JSON.parse(globe.setRoute(lon1, lat1, lon2, lat2));
    $("route-stats").textContent =
      `Geodesic : ${res.geodesicDistanceKm.toFixed(1)} km (hdg ${res.geodesicStartHeadingDeg.toFixed(1)}° → ${res.geodesicEndHeadingDeg.toFixed(1)}°)\n` +
      `Rhumb    : ${res.rhumbDistanceKm.toFixed(1)} km (const hdg ${res.rhumbHeadingDeg.toFixed(1)}°)\n` +
      `Delta    : +${res.deltaKm.toFixed(1)} km longer on rhumb\n` +
      `Midpoint : ${res.midpointDeg[1].toFixed(2)}°, ${res.midpointDeg[0].toFixed(2)}°`;
    globe.setMarkers(JSON.stringify([
      { lon: lon1 * RAD, lat: lat1 * RAD, color: [0.95, 0.72, 0.18], size: 10 },
      { lon: lon2 * RAD, lat: lat2 * RAD, color: [0.95, 0.72, 0.18], size: 10 },
    ]));
    if (fly) {
      flyTo({ lon: res.midpointDeg[0] * RAD, lat: res.midpointDeg[1] * RAD, range: Math.max(res.geodesicDistanceKm * 1800, bodyRadius * 1.4), pitch: -90 * RAD, heading: 0 });
    }
  }
  setRouteAndReadout(-74.006, 40.7128, -0.1276, 51.5074, false);
  globe.setSplineMode("catmull");
  applyLayers();

  // Body buttons
  document.querySelectorAll("#body-buttons button").forEach((b) => {
    b.addEventListener("click", () => {
      document.querySelectorAll("#body-buttons button").forEach((x) => x.classList.toggle("active", x === b));
      const info = JSON.parse(globe.setBody(b.dataset.body));
      bodyRadius = info.radii[0];
      cam.range = info.range;
    });
  });

  // Scene mode buttons
  document.querySelectorAll("#mode-buttons button").forEach((b) => {
    b.addEventListener("click", () => {
      document.querySelectorAll("#mode-buttons button").forEach((x) => x.classList.toggle("active", x === b));
      globe.setSceneMode(Number(b.dataset.mode));
    });
  });

  // Camera presets
  document.querySelectorAll("#preset-buttons button").forEach((b) => {
    b.addEventListener("click", () => {
      const scale = bodyRadius / 6378137;
      flyTo({ lon: Number(b.dataset.lon) * RAD, lat: Number(b.dataset.lat) * RAD, range: Number(b.dataset.range) * scale, heading: 0, pitch: -68 * RAD });
    });
  });
  $("btn-reset-north").addEventListener("click", () => flyTo({ heading: 0, pitch: -90 * RAD }, 600));
  $("fov-slider").addEventListener("input", () => {
    cam.fov = Number($("fov-slider").value);
    $("fov-val").textContent = `${cam.fov}°`;
  });

  // Routes & Tap-to-measure
  document.querySelectorAll("#route-buttons button").forEach((b) => {
    b.addEventListener("click", () => {
      document.querySelectorAll("#route-buttons button").forEach((x) => x.classList.toggle("active", x === b));
      const [lon1, lat1, lon2, lat2] = b.dataset.route.split(",").map(Number);
      setRouteAndReadout(lon1, lat1, lon2, lat2, true);
    });
  });
  $("btn-measure").addEventListener("click", () => {
    measuring = !measuring;
    measureStart = null;
    $("btn-measure").classList.toggle("active", measuring);
    $("btn-measure").textContent = measuring ? "Click start point on globe…" : "Tap-to-Measure on Globe";
  });

  // 3D Geometry showcase on globe
  function updateGlobeGeom() {
    const kind = $("geom-select").value;
    const wire = $("chk-geom-wire").checked;
    const res = JSON.parse(globe.setGeometryShowcase(kind, wire));
    $("geom-meta").textContent = kind === "none" ? "" : `${res.vertices} verts · ${res.triangles} tris · ${res.wireframeSegments} edges`;
    if (kind !== "none") {
      const scale = bodyRadius / 6378137;
      flyTo({ lon: -105.0 * RAD, lat: 39.5 * RAD, range: 5200e3 * scale, pitch: -52 * RAD, heading: 0.25 });
    }
  }
  $("geom-select").addEventListener("change", updateGlobeGeom);
  $("chk-geom-wire").addEventListener("change", updateGlobeGeom);

  // Layers & Splines & Clipping & PostProcess
  ["chk-graticule", "chk-rhumb", "chk-sun", "chk-atmo", "chk-stars", "chk-tiles", "chk-spline"].forEach((id) => {
    $(id).addEventListener("change", applyLayers);
  });
  document.querySelectorAll("#tile-scheme-row button").forEach((b) => {
    b.addEventListener("click", () => {
      document.querySelectorAll("#tile-scheme-row button").forEach((x) => x.classList.toggle("active", x === b));
      applyLayers();
    });
  });
  document.querySelectorAll("#spline-mode-row button").forEach((b) => {
    b.addEventListener("click", () => {
      document.querySelectorAll("#spline-mode-row button").forEach((x) => x.classList.toggle("active", x === b));
      globe.setSplineMode(b.dataset.spline);
      $("chk-spline").checked = true;
      applyLayers();
    });
  });
  ["chk-clip", "clip-dist", "clip-ang"].forEach((id) => $(id).addEventListener("input", applyClipping));
  ["pp-effect", "pp-fxaa", "pp-bloom", "pp-tonemapper", "pp-exposure"].forEach((id) => $(id).addEventListener("input", applyPostProcess));

  // Clock controls
  $("clk-toggle").addEventListener("click", () => {
    clockRunning = !clockRunning;
    $("clk-toggle").textContent = clockRunning ? "Pause" : "Play";
  });
  $("clk-speed").addEventListener("click", () => {
    clockSpeedIdx = (clockSpeedIdx + 1) % clockSpeeds.length;
    $("clk-speed").textContent = `×${clockSpeeds[clockSpeedIdx]}`;
  });
  $("clk-now").addEventListener("click", () => { simTimeMs = Date.now(); });

  // Globe pointer interaction (orbit / tilt / zoom / pick)
  let ptrDown = false, ptrMoved = false, ptrTilt = false, px = 0, py = 0, sx = 0, sy = 0;
  globeCanvas.addEventListener("contextmenu", (e) => e.preventDefault());
  globeCanvas.addEventListener("pointerdown", (e) => {
    ptrDown = true; ptrMoved = false; flight = null;
    ptrTilt = e.button === 2 || e.button === 1 || e.shiftKey || e.ctrlKey;
    px = sx = e.clientX; py = sy = e.clientY;
    globeCanvas.setPointerCapture(e.pointerId);
  });
  globeCanvas.addEventListener("pointermove", (e) => {
    if (!ptrDown) return;
    const dx = e.clientX - px, dy = e.clientY - py;
    px = e.clientX; py = e.clientY;
    if (Math.hypot(e.clientX - sx, e.clientY - sy) > 5) ptrMoved = true;
    if (ptrTilt) {
      cam.heading = wrapLon(cam.heading + dx * 0.006);
      cam.pitch = clamp(cam.pitch - dy * 0.005, -90 * RAD, -12 * RAD);
    } else {
      const mpp = (2 * cam.range * Math.tan((cam.fov * RAD) / 2)) / Math.max(1, globeCanvas.clientHeight);
      const k = mpp / bodyRadius;
      const sh = Math.sin(cam.heading), ch = Math.cos(cam.heading);
      cam.lat = clamp(cam.lat + (dx * sh + dy * ch) * k, -89.5 * RAD, 89.5 * RAD);
      cam.lon = wrapLon(cam.lon + ((-dx * ch + dy * sh) * k) / Math.max(Math.cos(cam.lat), 0.08));
    }
  });
  globeCanvas.addEventListener("pointerup", (e) => {
    if (ptrDown && !ptrMoved) {
      const rect = globeCanvas.getBoundingClientRect();
      const ndcX = ((e.clientX - rect.left) / rect.width) * 2 - 1;
      const ndcY = 1 - ((e.clientY - rect.top) / rect.height) * 2;
      const hit = JSON.parse(globe.pickNdc(ndcX, ndcY));
      if (hit.hit) {
        $("hud-pick").textContent = `${hit.latDeg.toFixed(2)}°, ${hit.lonDeg.toFixed(2)}°`;
        if (measuring) {
          if (!measureStart) {
            measureStart = { lon: hit.lonDeg, lat: hit.latDeg };
            $("btn-measure").textContent = "Click end point on globe…";
          } else {
            setRouteAndReadout(measureStart.lon, measureStart.lat, hit.lonDeg, hit.latDeg, false);
            measureStart = null;
            measuring = false;
            $("btn-measure").classList.remove("active");
            $("btn-measure").textContent = "Tap-to-Measure on Globe";
          }
        }
      } else {
        $("hud-pick").textContent = "Space (missed)";
      }
    }
    ptrDown = false;
  });
  globeCanvas.addEventListener("wheel", (e) => {
    e.preventDefault();
    flight = null;
    const d = clamp(e.deltaY, -200, 200);
    cam.range = clamp(cam.range * Math.exp(d * 0.0014), bodyRadius * 0.05, bodyRadius * 14);
  }, { passive: false });

  // --------------------------------------------------------------------------
  // 2. glTF MODEL VIEWER & GPU EFFECTS
  // --------------------------------------------------------------------------
  let modelViewer = null;
  let modelInitDone = false;
  let customGlbBytes = null;
  let mYaw = 0.6, mPitch = 0.35, mDist = 5.5;

  async function initModelTab() {
    if (!modelViewer) {
      const mc = $("model-canvas");
      const mgl = mc.getContext("webgl2", { antialias: true });
      mc.width = Math.max(300, mc.clientWidth * dpr);
      mc.height = Math.max(200, mc.clientHeight * dpr);
      modelViewer = new LiveModelViewer(mgl, mc.width, mc.height);

      let mDrag = false, mlx = 0, mly = 0;
      mc.addEventListener("pointerdown", (e) => { mDrag = true; mlx = e.clientX; mly = e.clientY; mc.setPointerCapture(e.pointerId); });
      mc.addEventListener("pointermove", (e) => {
        if (!mDrag) return;
        mYaw += (e.clientX - mlx) * 0.01;
        mPitch = clamp(mPitch + (e.clientY - mly) * 0.01, -1.4, 1.4);
        mlx = e.clientX; mly = e.clientY;
        modelViewer.setOrbit(mYaw, mPitch, mDist);
      });
      mc.addEventListener("pointerup", () => { mDrag = false; });
      mc.addEventListener("wheel", (e) => {
        e.preventDefault();
        mDist = clamp(mDist * Math.exp(e.deltaY * 0.001), 1.6, 25);
        modelViewer.setOrbit(mYaw, mPitch, mDist);
      }, { passive: false });
    }
    if (!modelInitDone) {
      modelInitDone = true;
      await rebuildModel();
      evalStyleNow();
      runEffectsPreviews();
    }
  }

  async function rebuildModel() {
    if (!modelViewer) return;
    $("model-status").textContent = "Loading…";
    try {
      const cfg = {
        color: hexToRgba($("model-color").value),
        colorBlendMode: $("model-blend").value,
        colorBlendAmount: Number($("model-blend-amt").value),
        customShaderFs: $("model-custom-fs").value,
      };
      const res = JSON.parse(await modelViewer.loadModel(customGlbBytes, JSON.stringify(cfg)));
      $("model-status").textContent = `Ready · ${(res.glbBytes / 1024).toFixed(1)} KB GLB`;
    } catch (e) {
      $("model-status").textContent = "Error: " + e;
    }
  }

  $("model-blend-amt").addEventListener("input", () => { $("model-blend-amt-val").textContent = Number($("model-blend-amt").value).toFixed(2); });
  $("btn-model-reload").addEventListener("click", rebuildModel);
  $("btn-model-reset").addEventListener("click", () => { customGlbBytes = null; $("model-custom-fs").value = ""; rebuildModel(); });
  $("model-file").addEventListener("change", async (e) => {
    const f = e.target.files?.[0];
    if (!f) return;
    customGlbBytes = new Uint8Array(await f.arrayBuffer());
    await rebuildModel();
  });
  $("btn-shader-preset-1").addEventListener("click", () => {
    $("model-custom-fs").value =
`void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) {
  float rim = 1.0 - abs(normalize(fsInput.attributes.normalEC).z);
  material.diffuse = mix(vec3(0.05, 0.25, 0.45), vec3(0.25, 0.95, 1.0), pow(rim, 1.5));
  material.emissive = vec3(0.08, 0.45, 0.65) * rim;
}`;
    rebuildModel();
  });
  $("btn-shader-preset-2").addEventListener("click", () => {
    $("model-custom-fs").value =
`void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) {
  float band = 0.5 + 0.5 * sin(fsInput.attributes.positionMC.y * 10.0);
  material.diffuse = mix(vec3(0.88, 0.22, 0.14), vec3(0.98, 0.86, 0.22), band);
}`;
    rebuildModel();
  });
  $("btn-shader-preset-clear").addEventListener("click", () => { $("model-custom-fs").value = ""; rebuildModel(); });

  function evalStyleNow() {
    try {
      const res = JSON.parse(evaluate_tile_style($("style-json").value, $("style-features").value));
      $("style-out").textContent = JSON.stringify(res, null, 2);
    } catch (e) {
      $("style-out").textContent = "Error: " + e;
    }
  }
  $("btn-eval-style").addEventListener("click", evalStyleNow);

  function runEffectsPreviews() {
    try {
      const sunCanvas = $("sun-canvas");
      sunCanvas.width = 240; sunCanvas.height = 240;
      const sunGl = sunCanvas.getContext("webgl2");
      const glow = Number($("sun-glow").value);
      $("sun-glow-val").textContent = glow.toFixed(1);
      const sunRes = JSON.parse(render_sun_preview(sunGl, glow, 240));

      const brdfCanvas = $("brdf-canvas");
      brdfCanvas.width = 240; brdfCanvas.height = 240;
      const brdfGl = brdfCanvas.getContext("webgl2");
      const brdfRes = JSON.parse(render_brdf_lut_preview(brdfGl, 240));

      const expCanvas = document.createElement("canvas");
      expCanvas.width = 16; expCanvas.height = 16;
      const expRes = JSON.parse(run_auto_exposure_preview(expCanvas.getContext("webgl2"), 180, 120, 64));

      $("effects-out").textContent = JSON.stringify({ sun: sunRes, brdfLut: brdfRes, autoExposure: expRes }, null, 2);
    } catch (e) {
      $("effects-out").textContent = "Error: " + e;
    }
  }
  $("sun-glow").addEventListener("input", runEffectsPreviews);
  $("btn-run-effects").addEventListener("click", runEffectsPreviews);

  // --------------------------------------------------------------------------
  // 3. DATASOURCES LAB
  // --------------------------------------------------------------------------
  let currentDsFmt = "czml";
  let lastOverlay = null;
  $("ds-input").value = DS_SAMPLES.czml;

  document.querySelectorAll("#ds-format-tabs button[data-ds]").forEach((b) => {
    b.addEventListener("click", () => {
      document.querySelectorAll("#ds-format-tabs button[data-ds]").forEach((x) => x.classList.toggle("active", x === b));
      currentDsFmt = b.dataset.ds;
      $("ds-input").value = DS_SAMPLES[currentDsFmt];
      parseCurrentDs();
    });
  });

  async function parseCurrentDs() {
    try {
      const text = $("ds-input").value;
      let raw;
      if (currentDsFmt === "czml") raw = await parse_czml(text, null);
      else if (currentDsFmt === "geojson") raw = await parse_geojson(text);
      else if (currentDsFmt === "kml") raw = await parse_kml(text);
      else raw = await parse_gpx(text);
      const res = JSON.parse(raw);
      lastOverlay = res.summary?.overlay || null;
      $("ds-output").textContent = JSON.stringify(res, null, 2);
    } catch (e) {
      $("ds-output").textContent = "Parse error: " + e;
    }
  }
  $("btn-ds-parse").addEventListener("click", parseCurrentDs);
  $("btn-ds-plot").addEventListener("click", async () => {
    await parseCurrentDs();
    if (lastOverlay) {
      globe.setDataSourceOverlay(JSON.stringify(lastOverlay));
      document.querySelector('#main-tabs button[data-tab="globe"]').click();
    }
  });

  async function doExportKml(asKmz) {
    try {
      const res = JSON.parse(await export_kml_from_geojson(DS_SAMPLES.geojson, asKmz));
      $("kml-export-out").textContent = res.mode === "kml" ? res.kml : JSON.stringify(res, null, 2);
    } catch (e) {
      $("kml-export-out").textContent = "Export error: " + e;
    }
  }
  $("btn-export-kml").addEventListener("click", () => doExportKml(false));
  $("btn-export-kmz").addEventListener("click", () => doExportKml(true));

  let propAnim = false;
  function updateTimeProps() {
    const t = Number($("prop-t-slider").value);
    $("prop-t-val").textContent = `${t.toFixed(1)}s`;
    try {
      $("prop-out").textContent = JSON.stringify(JSON.parse(eval_time_properties(t)), null, 2);
    } catch (e) {
      $("prop-out").textContent = "Error: " + e;
    }
  }
  $("prop-t-slider").addEventListener("input", updateTimeProps);
  $("btn-prop-anim").addEventListener("click", () => {
    propAnim = !propAnim;
    $("btn-prop-anim").classList.toggle("active", propAnim);
  });

  // --------------------------------------------------------------------------
  // 4. WORKERS, TERRAIN, SPLATS & SPATIAL INDICES
  // --------------------------------------------------------------------------
  const wGeomView = new MiniMeshViewport($("wgeom-canvas"), "mesh");
  const terrainView = new MiniMeshViewport($("terrain-canvas"), "terrain");
  const splatView = new MiniMeshViewport($("splat-canvas"), "splats");
  let workersInitDone = false;

  function initWorkersTab() {
    if (workersInitDone) return;
    workersInitDone = true;
    const names = JSON.parse(worker_geometry_list());
    const sel = $("wgeom-select");
    sel.innerHTML = names.map((n) => `<option value="${n}">${n}</option>`).join("");
    buildSelectedWorkerGeom();
    buildTerrain();
    runSplats();
    runSpatial();
  }

  function buildSelectedWorkerGeom() {
    try {
      const mod = $("wgeom-select").value;
      const res = JSON.parse(worker_geometry_build(mod));
      wGeomView.setGeometry(res.interleaved, res.drawIndices, res.wireIndices, res.isLines);
      $("wgeom-badge").textContent = `${res.vertices}v · ${res.elapsedMs.toFixed(1)}ms`;
      $("wgeom-out").textContent = JSON.stringify({
        module: res.module,
        primitiveType: res.primitiveType,
        vertices: res.vertices,
        indices: res.indices,
        elapsedMs: res.elapsedMs,
        boundingSphere: res.boundingSphere,
      }, null, 2);
    } catch (e) {
      $("wgeom-out").textContent = "Error: " + e;
    }
  }
  $("wgeom-select").addEventListener("change", buildSelectedWorkerGeom);
  $("wgeom-wire-btn").addEventListener("click", () => {
    wGeomView.wireframe = !wGeomView.wireframe;
    $("wgeom-wire-btn").classList.toggle("active", wGeomView.wireframe);
  });

  function buildTerrain() {
    const pattern = $("terrain-pattern").value;
    const grid = Number($("terrain-grid").value);
    const exag = Number($("terrain-exag").value);
    $("terrain-grid-val").textContent = `${grid}×${grid}`;
    $("terrain-exag-val").textContent = `${exag.toFixed(1)}×`;
    try {
      const res = JSON.parse(worker_terrain_build(pattern, grid, exag, 100.0));
      terrainView.setGeometry(res.previewVertices, res.previewIndices, null, false);
      $("terrain-out").textContent = JSON.stringify({
        pattern: res.pattern,
        grid: res.grid,
        tessellatorFloatCount: res.tessellatorFloatCount,
        encodingStride: res.encodingStride,
        minHeight: res.minHeight,
        maxHeight: res.maxHeight,
        centerInterpolatedHeight: res.centerInterpolatedHeight,
        elapsedMs: res.elapsedMs,
      }, null, 2);
    } catch (e) {
      $("terrain-out").textContent = "Error: " + e;
    }
  }
  ["terrain-pattern", "terrain-grid", "terrain-exag"].forEach((id) => $(id).addEventListener("input", buildTerrain));

  function runSplats() {
    const count = Number($("splat-count").value);
    $("splat-count-val").textContent = String(count);
    try {
      const res = JSON.parse(worker_gaussian_splats(count, splatView.yaw / RAD, splatView.pitch / RAD));
      splatView.setSplats(res.positions, res.colors, res.drawCount);
      $("splat-out").textContent = JSON.stringify({
        totalSplats: res.count,
        renderedSplats: res.drawCount,
        radixSortMs: res.sortMs,
        textureGenMs: res.textureGenMs,
        rgba32uiTexture: `${res.textureWidth}×${res.textureHeight} (${res.textureU32Len} u32s)`,
      }, null, 2);
    } catch (e) {
      $("splat-out").textContent = "Error: " + e;
    }
  }
  $("splat-count").addEventListener("input", () => { $("splat-count-val").textContent = $("splat-count").value; });
  $("btn-splat-run").addEventListener("click", runSplats);

  function runSpatial() {
    try {
      const res = JSON.parse(core_spatial_inspect(
        Number($("sp-lon").value),
        Number($("sp-lat").value),
        Number($("sp-lvl").value),
        Number($("sp-mx").value),
        Number($("sp-my").value),
        Number($("sp-mz").value),
      ));
      $("spatial-out").textContent = JSON.stringify(res, null, 2);
    } catch (e) {
      $("spatial-out").textContent = "Error: " + e;
    }
  }
  $("btn-spatial").addEventListener("click", runSpatial);
  $("btn-codecs").addEventListener("click", () => {
    try {
      $("spatial-out").textContent = JSON.stringify(JSON.parse(worker_codecs_test()), null, 2);
    } catch (e) {
      $("spatial-out").textContent = "Error: " + e;
    }
  });

  // --------------------------------------------------------------------------
  // 5. RENDERER & GPU DIAGNOSTICS
  // --------------------------------------------------------------------------
  let rendererInitDone = false;
  let computeGl = null;

  async function initRendererTab() {
    if (rendererInitDone) return;
    rendererInitDone = true;
    const capCanvas = document.createElement("canvas");
    $("gpu-caps-out").textContent = JSON.stringify(JSON.parse(renderer_capabilities_report(capCanvas.getContext("webgl2"))), null, 2);

    const cc = $("compute-canvas");
    cc.width = 240; cc.height = 240;
    computeGl = cc.getContext("webgl2");

    const ac = $("atlas-canvas");
    ac.width = 240; ac.height = 240;
    try {
      const atlasRes = JSON.parse(await renderer_atlas_demo(ac.getContext("webgl2"), 240));
      $("atlas-out").textContent = JSON.stringify(atlasRes, null, 2);
    } catch (e) {
      $("atlas-out").textContent = "Atlas error: " + e;
    }
    await runGpuSuite();
  }

  async function runGpuSuite() {
    const c = document.createElement("canvas");
    c.width = 16; c.height = 16;
    const res = JSON.parse(await renderer_run_gpu_suite(c.getContext("webgl2")));
    const host = $("gpu-suite-list");
    host.innerHTML = res.results.map((r) => `
      <div class="test-item">
        <div>
          <strong>${r.name}</strong>
          <div class="meta">${r.detail}</div>
        </div>
        <span class="badge ${r.ok ? "pass" : "fail"}">${r.ok ? "PASS" : "FAIL"} · ${r.ms.toFixed(1)}ms</span>
      </div>
    `).join("");
    return res.results;
  }
  $("btn-gpu-suite").addEventListener("click", runGpuSuite);

  // --------------------------------------------------------------------------
  // 6. BROWSER HOST ADAPTERS
  // --------------------------------------------------------------------------
  let browserHostInit = false;
  let browserCore = null;
  let pinBuilder = null;
  let screenHandler = null;
  const gestureLogs = [];

  function initBrowserHostTab() {
    if (browserHostInit) return;
    browserHostInit = true;
    browserCore = new BrowserCore();
    pinBuilder = new BrowserPinBuilder();
    renderTextDemo();
    renderPinsDemo();
    sanitizeCreditDemo();

    const pad = $("gesture-pad");
    screenHandler = new BrowserScreenSpaceEventHandler(pad);
    const events = [
      [0, "LEFT_DOWN"], [1, "LEFT_UP"], [2, "LEFT_CLICK"], [3, "LEFT_DOUBLE_CLICK"],
      [5, "RIGHT_DOWN"], [6, "RIGHT_UP"], [7, "RIGHT_CLICK"], [15, "MOUSE_MOVE"], [16, "WHEEL"],
    ];
    for (const [code, name] of events) {
      screenHandler.setInputAction((ev) => {
        if (name === "MOUSE_MOVE" && gestureLogs[0]?.startsWith("MOUSE_MOVE")) {
          gestureLogs[0] = `MOUSE_MOVE -> (${Math.round(ev.endPosition?.x || 0)}, ${Math.round(ev.endPosition?.y || 0)})`;
        } else {
          gestureLogs.unshift(`${name} -> ${JSON.stringify(ev)}`);
          if (gestureLogs.length > 14) gestureLogs.pop();
        }
        $("gesture-log").textContent = gestureLogs.join("\n");
      }, code);
    }
  }

  function renderTextDemo() {
    const canvas = writeTextToCanvas($("txt-label").value, {
      font: $("txt-font").value,
      fill: true,
      stroke: true,
      strokeWidth: 3,
      padding: Number($("txt-pad").value),
      fillColor: hexToRgba($("txt-fill").value),
      strokeColor: hexToRgba($("txt-stroke").value),
      backgroundColor: [0.06, 0.1, 0.17, 1.0],
    });
    const host = $("text-canvas-host");
    host.innerHTML = "";
    if (canvas) {
      host.appendChild(canvas);
      $("text-dims-out").textContent = JSON.stringify(canvas.dimensions, null, 2);
    }
  }

  function renderPinsDemo() {
    const host = $("pin-gallery");
    host.innerHTML = "";
    const size = Number($("pin-size").value);
    const col = hexToRgba($("pin-color").value);
    host.appendChild(pinBuilder.fromText($("pin-text").value, col, size));
    host.appendChild(pinBuilder.fromColor([0.94, 0.68, 0.18, 1.0], size));
    host.appendChild(pinBuilder.fromText("3D", [0.95, 0.36, 0.52, 1.0], size));
    host.appendChild(pinBuilder.fromText("★", [0.22, 0.88, 0.55, 1.0], size));
  }

  function sanitizeCreditDemo() {
    const el = browserCore.creditElement($("credit-html").value, true);
    const host = $("credit-dom-out");
    host.innerHTML = "";
    host.appendChild(el);
    $("credit-raw-out").textContent = el.outerHTML;
  }

  $("btn-render-text").addEventListener("click", renderTextDemo);
  $("btn-render-pins").addEventListener("click", renderPinsDemo);
  $("btn-sanitize-credit").addEventListener("click", sanitizeCreditDemo);

  // --------------------------------------------------------------------------
  // 7. 307 GLSL SHADERS EXPLORER
  // --------------------------------------------------------------------------
  let allShaders = [];
  let selectedShader = null;

  function initShadersTab() {
    if (allShaders.length > 0) return;
    const data = JSON.parse(shaders_list());
    allShaders = data.shaders;
    renderShaderList();
    if (allShaders.length) selectShader(allShaders[0].id);
  }

  function renderShaderList() {
    const q = $("shader-search").value.trim().toLowerCase();
    const cat = $("shader-cat").value;
    const filtered = allShaders.filter((s) => {
      if (cat !== "ALL") {
        const isRoot = !["Builtin", "Appearances", "Materials", "Model", "PostProcessStages", "Voxels"].includes(s.category);
        if (cat === "Root" && !isRoot) return false;
        if (cat !== "Root" && s.category !== cat) return false;
      }
      return !q || s.id.toLowerCase().includes(q) || s.name.toLowerCase().includes(q);
    });
    const host = $("shader-items");
    host.innerHTML = filtered.map((s) =>
      `<button class="shader-row ${selectedShader?.id === s.id ? "active" : ""}" data-id="${s.id}">${s.name} <span style="color:var(--muted)">(${s.lines}L)</span></button>`
    ).join("");
    host.querySelectorAll(".shader-row").forEach((b) => {
      b.addEventListener("click", () => selectShader(b.dataset.id));
    });
  }

  function selectShader(id) {
    const detail = JSON.parse(shaders_get(id));
    selectedShader = detail;
    $("shader-title").textContent = detail.id;
    $("shader-meta").textContent = `symbol: ${detail.name}`;
    $("shader-src").textContent = detail.source;
    $("shader-webgl1").textContent = detail.demodernizedWebgl1;
    $("shader-compile-badge").textContent = "Ready";
    $("shader-compile-badge").className = "badge";
    renderShaderList();
  }

  $("shader-search").addEventListener("input", renderShaderList);
  $("shader-cat").addEventListener("change", renderShaderList);
  $("btn-compile-shader").addEventListener("click", () => {
    if (!selectedShader) return;
    const c = document.createElement("canvas");
    const isFrag = !selectedShader.id.endsWith("VS");
    let src = selectedShader.source;
    // Wrap standalone helper functions in a minimal main() so WebGL2 can compile them
    if (!/\bvoid\s+main\s*\(/.test(src)) {
      src = src + "\nvoid main() { " + (isFrag ? "out_FragColor = vec4(1.0);" : "gl_Position = vec4(0.0);") + " }\n";
    }
    const res = JSON.parse(shaders_test_compile(c.getContext("webgl2"), src, isFrag));
    const badge = $("shader-compile-badge");
    if (res.ok) {
      badge.className = "badge pass";
      badge.textContent = `COMPILED (${res.uniforms} uniforms)`;
    } else {
      badge.className = "badge fail";
      badge.textContent = `INFO: ${String(res.error).slice(0, 70)}`;
    }
  });

  // --------------------------------------------------------------------------
  // 8. FULL-STACK ONE-CLICK LIVE SELF-TEST SUITE
  // --------------------------------------------------------------------------
  async function runAllLiveTests() {
    const list = $("selftest-list");
    list.innerHTML = "";
    const results = [];
    const addRow = (name, ok, ms, detail) => {
      results.push({ name, ok });
      const div = document.createElement("div");
      div.className = "test-item";
      div.innerHTML = `<div><strong>${name}</strong><div class="meta">${detail}</div></div><span class="badge ${ok ? "pass" : "fail"}">${ok ? "PASS" : "FAIL"} · ${ms.toFixed(1)}ms</span>`;
      list.appendChild(div);
    };

    const runCase = async (name, fn) => {
      const t0 = performance.now();
      try {
        const detail = await fn();
        addRow(name, true, performance.now() - t0, detail);
      } catch (e) {
        addRow(name, false, performance.now() - t0, String(e));
      }
    };

    await runCase("Core: Geodesic vs Rhumb + ENU + Projections (WGS84, Moon, Mars)", () => {
      const w = JSON.parse(core_geodesy_compare("wgs84", -74.006, 40.7128, -0.1276, 51.5074));
      const m = JSON.parse(core_geodesy_compare("moon", 0, 0, 45, 30));
      if (!(w.geodesic.distanceKm > 5500 && w.rhumb.distanceKm > w.geodesic.distanceKm)) throw new Error("Geodesy mismatch");
      return `WGS84 NYC→LON ${w.geodesic.distanceKm.toFixed(1)}km (rhumb ${w.rhumb.distanceKm.toFixed(1)}km), Moon ${m.geodesic.distanceKm.toFixed(1)}km`;
    });

    await runCase("Core: Astronomy (JulianDate, Leap Seconds, Simon1994 Sun & Moon)", () => {
      const a = JSON.parse(core_astronomy_inspect("2026-06-21T12:00:00Z"));
      if (!(a.sun.distanceAu > 0.98 && a.sun.distanceAu < 1.03)) throw new Error("Bad Sun AU");
      return `JD ${a.julianDay.toFixed(3)} · TAI-UTC=${a.taiMinusUtcSec}s · Sun ${a.sun.distanceAu.toFixed(4)} AU · Moon ${a.moon.distanceKm.toFixed(0)} km`;
    });

    await runCase("Core: Spatial Indices (S2Cell, Morton 2D/3D, Hilbert, TilingSchemes)", () => {
      const sp = JSON.parse(core_spatial_inspect(-105.27, 40.01, 8, 42, 19, 7));
      if (!sp.s2Cell?.token || sp.morton2d.decoded[0] !== 42 || sp.morton3d.decoded[2] !== 7) throw new Error("Spatial roundtrip failed");
      return `S2 token=${sp.s2Cell.token} · Morton2D=${sp.morton2d.encoded} · Hilbert=${sp.hilbert2d.encoded}`;
    });

    await runCase("Workers: All 31 Geometry Workers (pack/build/cache-reorder)", () => {
      const mods = JSON.parse(worker_geometry_list());
      let totalVerts = 0;
      for (const m of mods) {
        const g = JSON.parse(worker_geometry_build(m));
        if (g.vertices <= 0) throw new Error(`${m} produced 0 verts`);
        totalVerts += g.vertices;
      }
      return `All ${mods.length}/31 geometry workers built (${totalVerts.toLocaleString()} total verts)`;
    });

    await runCase("Workers: HeightmapTerrainTessellator + interpolate_height", () => {
      const t = JSON.parse(worker_terrain_build("alpine", 33, 2.0, 100.0));
      if (t.tessellatorFloatCount <= 0) throw new Error("Empty terrain mesh");
      return `33×33 tile -> ${t.tessellatorFloatCount} encoded floats · h=[${t.minHeight.toFixed(0)}m, ${t.maxHeight.toFixed(0)}m]`;
    });

    await runCase("Workers: Gaussian Splat Radix Sorter & RGBA32UI Covariance Packer", () => {
      const s = JSON.parse(worker_gaussian_splats(5000, 30, 20));
      if (s.textureWidth !== 2048 || s.drawCount === 0) throw new Error("Splat pack failed");
      return `5,000 splats sorted in ${s.radixSortMs?.toFixed(2) ?? s.sortMs.toFixed(2)}ms · tex ${s.textureWidth}×${s.textureHeight}`;
    });

    await runCase("Workers & Core: Codecs, Compression, Pipeline, TaskProcessor", () => {
      const c = JSON.parse(worker_codecs_test());
      const failed = c.checks.filter((x) => !x.ok);
      if (failed.length) throw new Error(JSON.stringify(failed));
      return `${c.checks.length}/${c.checks.length} checks passed`;
    });

    await runCase("DataSources: CZML + GeoJSON + KML + GPX + ExportKML/KMZ + TimeProperties", async () => {
      const cz = JSON.parse(await parse_czml(DS_SAMPLES.czml, null));
      const gj = JSON.parse(await parse_geojson(DS_SAMPLES.geojson));
      const km = JSON.parse(await parse_kml(DS_SAMPLES.kml));
      const gp = JSON.parse(await parse_gpx(DS_SAMPLES.gpx));
      const ex = JSON.parse(await export_kml_from_geojson(DS_SAMPLES.geojson, false));
      const kmz = JSON.parse(await export_kml_from_geojson(DS_SAMPLES.geojson, true));
      const tp = JSON.parse(eval_time_properties(40.0));
      if (!ex.kml.includes("<kml") || kmz.zipSignature !== "504B") throw new Error("KML/KMZ export invalid");
      return `CZML(${cz.summary.entityCount}) GeoJSON(${gj.summary.entityCount}) KML(${km.summary.entityCount}) GPX(${gp.summary.entityCount}) KMZ(${kmz.byteLength}B) Speed=${tp.speedMps.toFixed(1)}m/s`;
    });

    await runCase("Scene: glTF 2.0 Model::from_gltf + CustomShader + Cesium3DTileStyle", async () => {
      const c = document.createElement("canvas");
      c.width = 128; c.height = 128;
      const mv = new LiveModelViewer(c.getContext("webgl2"), 128, 128);
      const r = JSON.parse(await mv.loadModel(null, JSON.stringify({ colorBlendMode: "mix", colorBlendAmount: 0.5 })));
      const draws = mv.render();
      const st = JSON.parse(evaluate_tile_style($("style-json").value, $("style-features").value));
      if (!r.ready || draws < 1 || st.results.length !== 4) throw new Error("Model/Style render failed");
      return `Model ready (${r.glbBytes}B, ${draws} draw cmd), Style evaluated 4 features + GLSL generated`;
    });

    await runCase("Scene GPU: Sun Compute + BrdfLutGenerator + AutoExposure + PostProcess", () => {
      const c = document.createElement("canvas");
      c.width = 64; c.height = 64;
      const sun = JSON.parse(render_sun_preview(c.getContext("webgl2"), 1.2, 64));
      const c2 = document.createElement("canvas");
      c2.width = 64; c2.height = 64;
      const brdf = JSON.parse(render_brdf_lut_preview(c2.getContext("webgl2"), 64));
      const c3 = document.createElement("canvas");
      c3.width = 16; c3.height = 16;
      const exp = JSON.parse(run_auto_exposure_preview(c3.getContext("webgl2"), 200, 150, 100));
      return `Sun ${sun.textureWidth}×${sun.textureHeight} · BRDF ${brdf.lutWidth}×${brdf.lutHeight} · AutoExposure ${exp.iterations.length} iters`;
    });

    await runCase("Renderer WebGL2: 8-Test GPU Pipeline Suite (VA/MSAA/PBO/Compute/Cube/3D/Atlas/Pick)", async () => {
      const c = document.createElement("canvas");
      c.width = 16; c.height = 16;
      const suite = JSON.parse(await renderer_run_gpu_suite(c.getContext("webgl2")));
      const bad = suite.results.filter((x) => !x.ok);
      if (bad.length) throw new Error(bad.map((x) => x.name).join(", "));
      return `${suite.results.length}/${suite.results.length} WebGL2 GPU tests passed`;
    });

    await runCase("Browser Host: writeTextToCanvas + PinBuilder + DOMPurify + KTX2/Basis Transcoder", async () => {
      const bc = new BrowserCore();
      const pb = new BrowserPinBuilder();
      const tc = writeTextToCanvas("CesiumRS", { font: "20px sans-serif", padding: 4 });
      const pin = pb.fromText("OK", [0.2, 0.7, 0.9, 1], 48);
      const cred = bc.creditElement('Safe <script>alert(1)</script><b>Text</b>', true);
      const ktxBytes = new Uint8Array(await (await fetch("/testdata/ktx2/Green4x4Mipmap_ETC1S.ktx2")).arrayBuffer());
      const mips = await bc.transcodeKtx2(ktxBytes, { etc: true });
      if (!tc || !pin || cred.innerHTML.includes("<script") || !mips || mips.length !== 3) {
        throw new Error("Browser host / KTX2 check failed");
      }
      return `Text ${tc.width}×${tc.height}px · Pin ${pin.width}×${pin.height}px · XSS stripped · KTX2/Basis ${mips.length} mips`;
    });

    await runCase("Shaders: All 306 GLSL Modules + WebGL1 Demodernizer + Live Compile", () => {
      const sl = JSON.parse(shaders_list());
      const sample = JSON.parse(shaders_get("Shaders/PostProcessStages/NightVision"));
      const c = document.createElement("canvas");
      const comp = JSON.parse(shaders_test_compile(c.getContext("webgl2"), sample.source, true));
      if (sl.total < 300 || !sample.demodernizedWebgl1.length || !comp.ok) {
        throw new Error(`total=${sl.total}, ok=${comp.ok}, err=${comp.error || ""}`);
      }
      return `${sl.total} shaders registered · NightVision demodernized & WebGL2 compiled (${comp.uniforms} uniforms)`;
    });

    const passed = results.filter((r) => r.ok).length;
    const sum = $("selftest-summary");
    sum.className = `badge ${passed === results.length ? "pass" : "fail"}`;
    sum.textContent = `${passed}/${results.length} PASSED`;
  }
  $("btn-run-all-tests").addEventListener("click", runAllLiveTests);

  // Tab activation lazy initializers
  function onTabActivated(tab) {
    if (tab === "model") void initModelTab();
    if (tab === "datasources") { void parseCurrentDs(); updateTimeProps(); }
    if (tab === "workers") initWorkersTab();
    if (tab === "renderer") void initRendererTab();
    if (tab === "browserhost") initBrowserHostTab();
    if (tab === "shaders") initShadersTab();
  }

  // --------------------------------------------------------------------------
  // Main 60 FPS Animation Loop
  // --------------------------------------------------------------------------
  let fpsFrames = 0, fpsLast = performance.now(), fpsVal = 60;
  function frameLoop(now) {
    requestAnimationFrame(frameLoop);
    const dt = (now - lastPerfMs) / 1000;
    lastPerfMs = now;
    fpsFrames++;
    if (now - fpsLast >= 500) {
      fpsVal = Math.round((fpsFrames * 1000) / (now - fpsLast));
      fpsFrames = 0;
      fpsLast = now;
    }

    if (activeTab === "globe") {
      const w = Math.max(1, Math.round(globeCanvas.clientWidth * dpr));
      const h = Math.max(1, Math.round(globeCanvas.clientHeight * dpr));
      if (globeCanvas.width !== w || globeCanvas.height !== h) {
        globeCanvas.width = w;
        globeCanvas.height = h;
        globe.resize(w, h, dpr);
      }
      if (flight) {
        const t = clamp((now - flight.start) / flight.dur, 0, 1);
        const k = t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
        cam.lon = wrapLon(flight.from.lon + (flight.to.lon - flight.from.lon) * k);
        cam.lat = flight.from.lat + (flight.to.lat - flight.from.lat) * k;
        cam.heading = flight.from.heading + (flight.to.heading - flight.from.heading) * k;
        cam.pitch = flight.from.pitch + (flight.to.pitch - flight.from.pitch) * k;
        cam.range = Math.exp(Math.log(flight.from.range) + (Math.log(flight.to.range) - Math.log(flight.from.range)) * k);
        if (t >= 1) flight = null;
      }
      if (clockRunning) {
        simTimeMs += dt * 1000 * clockSpeeds[clockSpeedIdx];
      }
      const jdDays = simTimeMs / 86400000 + 2440587.5;
      globe.setJulianDate(jdDays);
      globe.setCamera(cam.lon, cam.lat, cam.heading, cam.pitch, cam.range, cam.fov);
      const info = JSON.parse(globe.render(now / 1000));

      $("hud-cam").textContent = `${(cam.lat / RAD).toFixed(2)}°, ${(cam.lon / RAD).toFixed(2)}°`;
      $("hud-range").textContent = `${(cam.range / 1000).toFixed(0)} km`;
      $("hud-hp").textContent = `${((cam.heading / RAD + 360) % 360).toFixed(0)}° / ${(cam.pitch / RAD).toFixed(0)}°`;
      $("hud-fps").textContent = `${fpsVal} fps · L${info.tileLevel}`;
      $("clk-jd").textContent = `JD ${jdDays.toFixed(3)}`;
      if (now - lastAstroMs > 400) {
        lastAstroMs = now;
        try {
          const astro = JSON.parse(core_astronomy_inspect(String(simTimeMs)));
          $("clk-iso").textContent = astro.iso8601.slice(0, 19) + "Z";
          if (astro.sun.subsolarDeg) $("hud-sun").textContent = `${astro.sun.subsolarDeg[1].toFixed(1)}°, ${astro.sun.subsolarDeg[0].toFixed(1)}°`;
          if (astro.moon.sublunarDeg) $("hud-moon").textContent = `${astro.moon.sublunarDeg[1].toFixed(1)}°, ${astro.moon.sublunarDeg[0].toFixed(1)}°`;
        } catch {}
      }
    } else if (activeTab === "model" && modelViewer) {
      mYaw += 0.004;
      modelViewer.setOrbit(mYaw, mPitch, mDist);
      modelViewer.render();
    } else if (activeTab === "datasources" && propAnim) {
      const next = (Number($("prop-t-slider").value) + dt * 15) % 100;
      $("prop-t-slider").value = next.toFixed(1);
      updateTimeProps();
    } else if (activeTab === "workers" && workersInitDone) {
      wGeomView.render(0.005);
      terrainView.render(0.003);
      splatView.render(0.004);
    } else if (activeTab === "renderer" && computeGl) {
      renderer_compute_demo(computeGl, 240, 240, now / 1000);
    }
  }
  requestAnimationFrame(frameLoop);
}

main();
