import { decodeBase64, type CameraFrame, type CesiumEngine } from "./engine";
import { EARTH_JPEG_BASE64 } from "./generated/earth";

const RAD = Math.PI / 180;
const RADIUS = 6378137;
const FOV_Y = 50 * RAD;
const MIN_RANGE = 250e3;
const MAX_RANGE = 90000e3;
const MIN_PITCH = -90 * RAD;
const MAX_PITCH = -12 * RAD;

export type CameraState = {
  /** Radians. */
  lon: number;
  lat: number;
  heading: number;
  pitch: number;
  /** Meters from the focus point. */
  range: number;
};

export type Layers = {
  graticule: boolean;
  tiles: boolean;
  tileScheme: 0 | 1;
  sunLighting: boolean;
};

export type Marker = { lon: number; lat: number; color: [number, number, number]; size: number };

type Flight = {
  from: CameraState;
  to: CameraState;
  start: number;
  duration: number;
  lift: number;
};

const GLOBE_VS = `
attribute vec3 a_position;
attribute vec3 a_normal;
attribute vec2 a_uv;
uniform mat4 u_viewProjection;
uniform float u_scale;
varying vec3 v_normal;
varying vec3 v_position;
varying vec2 v_uv;
void main() {
  vec3 p = a_position * u_scale;
  v_normal = a_normal;
  v_position = p;
  v_uv = a_uv;
  gl_Position = u_viewProjection * vec4(p, 1.0);
}`;

// v = 0 is the north edge of the image. The texture is uploaded without a
// vertical flip, so it is sampled as-is.
const GLOBE_FS = `
precision highp float;
uniform sampler2D u_texture;
uniform vec3 u_eye;
uniform vec3 u_sun;
uniform float u_sunLighting;
varying vec3 v_normal;
varying vec3 v_position;
varying vec2 v_uv;
void main() {
  vec3 n = normalize(v_normal);
  vec3 toEye = normalize(u_eye - v_position);
  vec3 color = texture2D(u_texture, v_uv).rgb;
  float facing = max(dot(n, toEye), 0.0);
  float head = 0.62 + 0.38 * facing;
  float sun = dot(n, u_sun);
  float day = smoothstep(-0.12, 0.18, sun);
  float lit = mix(0.12, 1.05, day) * (0.85 + 0.15 * facing);
  color *= mix(head, lit, u_sunLighting);
  float rim = pow(1.0 - facing, 3.0);
  color = mix(color, vec3(0.42, 0.66, 1.0), rim * 0.55);
  gl_FragColor = vec4(color, 1.0);
}`;

const HALO_FS = `
precision highp float;
uniform vec3 u_eye;
varying vec3 v_normal;
varying vec3 v_position;
varying vec2 v_uv;
void main() {
  vec3 n = normalize(v_normal);
  vec3 toEye = normalize(u_eye - v_position);
  float edge = clamp(-dot(n, toEye) / 0.3, 0.0, 1.0);
  float glow = pow(edge, 2.2) * 0.85;
  gl_FragColor = vec4(vec3(0.35, 0.6, 1.0) * glow, glow);
}`;

const LINE_VS = `
attribute vec3 a_position;
uniform mat4 u_viewProjection;
uniform float u_scale;
uniform float u_pointSize;
void main() {
  gl_Position = u_viewProjection * vec4(a_position * u_scale, 1.0);
  gl_PointSize = u_pointSize;
}`;

const LINE_FS = `
precision mediump float;
uniform vec4 u_color;
uniform float u_round;
void main() {
  if (u_round > 0.5) {
    vec2 d = gl_PointCoord - vec2(0.5);
    float r = length(d);
    if (r > 0.5) discard;
    float ring = smoothstep(0.32, 0.38, r);
    gl_FragColor = vec4(mix(u_color.rgb, vec3(0.03, 0.04, 0.05), ring), u_color.a);
    return;
  }
  gl_FragColor = u_color;
}`;

// Stars sit at infinity: only the view rotation applies, and depth is pinned
// to the far plane.
const STAR_VS = `
attribute vec4 a_star;
uniform mat4 u_view;
uniform mat4 u_projection;
uniform float u_pixelRatio;
varying float v_brightness;
void main() {
  vec3 dir = mat3(u_view[0].xyz, u_view[1].xyz, u_view[2].xyz) * a_star.xyz;
  gl_Position = u_projection * vec4(dir, 1.0);
  gl_Position.z = gl_Position.w * 0.99999;
  gl_PointSize = (1.0 + a_star.w * 1.6) * u_pixelRatio;
  v_brightness = a_star.w;
}`;

const STAR_FS = `
precision mediump float;
varying float v_brightness;
void main() {
  float r = length(gl_PointCoord - vec2(0.5));
  if (r > 0.5) discard;
  gl_FragColor = vec4(vec3(0.85, 0.9, 1.0) * v_brightness, 1.0);
}`;

function compile(gl: WebGLRenderingContext, vs: string, fs: string): WebGLProgram {
  const program = gl.createProgram()!;
  for (const [type, src] of [
    [gl.VERTEX_SHADER, vs],
    [gl.FRAGMENT_SHADER, fs],
  ] as const) {
    const shader = gl.createShader(type)!;
    gl.shaderSource(shader, src);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      throw new Error(`shader: ${gl.getShaderInfoLog(shader)}`);
    }
    gl.attachShader(program, shader);
  }
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    throw new Error(`program: ${gl.getProgramInfoLog(program)}`);
  }
  return program;
}

type Program = {
  program: WebGLProgram;
  attrib: (name: string) => number;
  uniform: (name: string) => WebGLUniformLocation | null;
};

function makeProgram(gl: WebGLRenderingContext, vs: string, fs: string): Program {
  const program = compile(gl, vs, fs);
  const uniforms = new Map<string, WebGLUniformLocation | null>();
  return {
    program,
    attrib: (name) => gl.getAttribLocation(program, name),
    uniform: (name) => {
      if (!uniforms.has(name)) uniforms.set(name, gl.getUniformLocation(program, name));
      return uniforms.get(name)!;
    },
  };
}

type LineBuffer = { buffer: WebGLBuffer; count: number };

function wrapLon(lon: number): number {
  return ((((lon + Math.PI) % (2 * Math.PI)) + 2 * Math.PI) % (2 * Math.PI)) - Math.PI;
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

function ease(t: number): number {
  return t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
}

function julianDate(ms: number): number {
  return ms / 86400000 + 2440587.5;
}

export class GlobeView {
  readonly camera: CameraState;
  layers: Layers = { graticule: true, tiles: false, tileScheme: 0, sunLighting: false };

  private gl: WebGLRenderingContext;
  private globeProgram: Program;
  private haloProgram: Program;
  private lineProgram: Program;
  private starProgram: Program;
  private globe: { vertices: WebGLBuffer; indices: WebGLBuffer; count: number };
  private graticule: LineBuffer;
  private stars: LineBuffer;
  private texture: WebGLTexture;
  private arc: LineBuffer | null = null;
  private tiles: LineBuffer | null = null;
  private tileKey = "";
  private markers: Marker[] = [];
  private markerBuffer: WebGLBuffer;
  private frame: CameraFrame | null = null;
  private flight: Flight | null = null;
  private dirty = true;
  private raf = 0;
  private pixelRatio = 1;
  private pointers = new Map<number, { x: number; y: number }>();
  private gesture: {
    kind: "orbit" | "tilt" | "pinch";
    startX: number;
    startY: number;
    startTime: number;
    moved: boolean;
    pinchDist: number;
    pinchAngle: number;
    pinchMidY: number;
  } | null = null;
  private disposers: (() => void)[] = [];

  onChange: (camera: CameraState) => void = () => {};
  onTap: (hit: { lon: number; lat: number } | null) => void = () => {};
  onTileLevel: (level: number) => void = () => {};

  constructor(
    private canvas: HTMLCanvasElement,
    private engine: CesiumEngine,
    initial: CameraState,
  ) {
    const gl = canvas.getContext("webgl", { antialias: true, alpha: false });
    if (!gl) throw new Error("WebGL is not available in this browser");
    this.gl = gl;
    if (!gl.getExtension("OES_element_index_uint")) {
      throw new Error("WebGL is missing 32-bit index support");
    }
    this.camera = { ...initial };

    this.globeProgram = makeProgram(gl, GLOBE_VS, GLOBE_FS);
    this.haloProgram = makeProgram(gl, GLOBE_VS, HALO_FS);
    this.lineProgram = makeProgram(gl, LINE_VS, LINE_FS);
    this.starProgram = makeProgram(gl, STAR_VS, STAR_FS);

    const mesh = engine.globe(128, 256);
    this.globe = {
      vertices: this.upload(mesh.vertices),
      indices: this.uploadIndices(mesh.indices),
      count: mesh.indices.length,
    };
    this.graticule = this.lines(engine.graticule(15));
    const stars = engine.stars(2400, 7);
    this.stars = { buffer: this.upload(stars), count: stars.length / 4 };
    this.markerBuffer = gl.createBuffer()!;

    this.texture = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array([24, 48, 84, 255]));
    void this.loadTexture();

    this.attachInput();
    this.resize();
    const observer = new ResizeObserver(() => this.resize());
    observer.observe(canvas);
    this.disposers.push(() => observer.disconnect());
    this.loop();
  }

  dispose(): void {
    cancelAnimationFrame(this.raf);
    for (const d of this.disposers) d();
  }

  private async loadTexture(): Promise<void> {
    const gl = this.gl;
    const blob = new Blob([decodeBase64(EARTH_JPEG_BASE64)], { type: "image/jpeg" });
    // createImageBitmap keeps image row 0 (the north edge) first, and
    // UNPACK_FLIP_Y stays off, so texture row 0 is +90° latitude.
    const bitmap = await createImageBitmap(blob);
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGB, gl.RGB, gl.UNSIGNED_BYTE, bitmap);
    gl.generateMipmap(gl.TEXTURE_2D);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.REPEAT);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    const aniso = gl.getExtension("EXT_texture_filter_anisotropic");
    if (aniso) {
      gl.texParameterf(gl.TEXTURE_2D, aniso.TEXTURE_MAX_ANISOTROPY_EXT, Math.min(8, gl.getParameter(aniso.MAX_TEXTURE_MAX_ANISOTROPY_EXT)));
    }
    bitmap.close();
    this.invalidate();
  }

  private upload(data: Float32Array): WebGLBuffer {
    const gl = this.gl;
    const buffer = gl.createBuffer()!;
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW);
    return buffer;
  }

  private uploadIndices(data: Uint32Array): WebGLBuffer {
    const gl = this.gl;
    const buffer = gl.createBuffer()!;
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, buffer);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, data, gl.STATIC_DRAW);
    return buffer;
  }

  private lines(data: Float32Array): LineBuffer {
    return { buffer: this.upload(data), count: data.length / 3 };
  }

  private freeLines(lines: LineBuffer | null): void {
    if (lines) this.gl.deleteBuffer(lines.buffer);
  }

  invalidate(): void {
    this.dirty = true;
  }

  setLayers(layers: Partial<Layers>): void {
    this.layers = { ...this.layers, ...layers };
    this.tileKey = "";
    this.invalidate();
  }

  setArc(arc: Float32Array | null): void {
    this.freeLines(this.arc);
    this.arc = arc && arc.length ? this.lines(arc) : null;
    this.invalidate();
  }

  setMarkers(markers: Marker[]): void {
    this.markers = markers;
    this.invalidate();
  }

  flyTo(target: Partial<CameraState>, duration = 1600): void {
    const from = { ...this.camera };
    const to = { ...from, ...target };
    to.range = clamp(to.range, MIN_RANGE, MAX_RANGE);
    to.pitch = clamp(to.pitch, MIN_PITCH, MAX_PITCH);
    // Fly the short way around the antimeridian and back to north-up.
    to.lon = from.lon + wrapLon(to.lon - from.lon);
    to.heading = from.heading + wrapLon(to.heading - from.heading);
    const angle = Math.acos(
      clamp(
        Math.sin(from.lat) * Math.sin(to.lat) + Math.cos(from.lat) * Math.cos(to.lat) * Math.cos(to.lon - from.lon),
        -1,
        1,
      ),
    );
    const lift = Math.max(0, angle * RADIUS * 0.9 - Math.max(from.range, to.range) * 0.5);
    this.flight = { from, to, start: performance.now(), duration, lift };
    this.invalidate();
  }

  resetNorth(): void {
    this.flyTo({ heading: 0, pitch: MIN_PITCH }, 700);
  }

  private resize(): void {
    this.pixelRatio = Math.min(window.devicePixelRatio || 1, 2);
    const w = Math.max(1, Math.round(this.canvas.clientWidth * this.pixelRatio));
    const h = Math.max(1, Math.round(this.canvas.clientHeight * this.pixelRatio));
    if (this.canvas.width !== w || this.canvas.height !== h) {
      this.canvas.width = w;
      this.canvas.height = h;
    }
    this.invalidate();
  }

  private loop = (): void => {
    this.raf = requestAnimationFrame(this.loop);
    if (this.flight) this.stepFlight();
    if (!this.dirty) return;
    this.dirty = false;
    this.render();
    this.onChange({ ...this.camera });
  };

  private stepFlight(): void {
    const f = this.flight!;
    const t = clamp((performance.now() - f.start) / f.duration, 0, 1);
    const k = ease(t);
    const c = this.camera;
    c.lon = wrapLon(f.from.lon + (f.to.lon - f.from.lon) * k);
    c.lat = f.from.lat + (f.to.lat - f.from.lat) * k;
    c.heading = f.from.heading + (f.to.heading - f.from.heading) * k;
    c.pitch = f.from.pitch + (f.to.pitch - f.from.pitch) * k;
    c.range = Math.exp(Math.log(f.from.range) + (Math.log(f.to.range) - Math.log(f.from.range)) * k) + f.lift * Math.sin(Math.PI * t);
    if (t >= 1) {
      c.lon = wrapLon(f.to.lon);
      c.heading = wrapLon(f.to.heading);
      this.flight = null;
    }
    this.invalidate();
  }

  /** Like Cesium's PerspectiveFrustum, the field of view spans the narrower side. */
  private fovY(): number {
    const aspect = this.canvas.width / this.canvas.height;
    return aspect >= 1 ? FOV_Y : 2 * Math.atan(Math.tan(FOV_Y / 2) / aspect);
  }

  private computeFrame(): CameraFrame {
    const c = this.camera;
    const aspect = this.canvas.width / this.canvas.height;
    const height = c.range * Math.sin(-c.pitch);
    const eyeDistance = RADIUS + height;
    const near = Math.max(50, height * 0.4);
    const far = eyeDistance + RADIUS * 1.1;
    return this.engine.lookAt({
      lon: c.lon,
      lat: c.lat,
      height: 0,
      heading: c.heading,
      pitch: c.pitch,
      range: c.range,
      fovY: this.fovY(),
      aspect,
      near,
      far,
    });
  }

  private render(): void {
    const gl = this.gl;
    const frame = this.computeFrame();
    this.frame = frame;
    this.updateTiles();

    gl.viewport(0, 0, this.canvas.width, this.canvas.height);
    gl.clearColor(0.027, 0.035, 0.047, 1);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);

    // Stars.
    gl.disable(gl.DEPTH_TEST);
    gl.disable(gl.BLEND);
    {
      const p = this.starProgram;
      gl.useProgram(p.program);
      gl.uniformMatrix4fv(p.uniform("u_view"), false, frame.view);
      gl.uniformMatrix4fv(p.uniform("u_projection"), false, frame.projection);
      gl.uniform1f(p.uniform("u_pixelRatio"), this.pixelRatio);
      gl.bindBuffer(gl.ARRAY_BUFFER, this.stars.buffer);
      const loc = p.attrib("a_star");
      gl.enableVertexAttribArray(loc);
      gl.vertexAttribPointer(loc, 4, gl.FLOAT, false, 16, 0);
      gl.drawArrays(gl.POINTS, 0, this.stars.count);
      gl.disableVertexAttribArray(loc);
    }

    const sun = this.engine.sun(julianDate(Date.now())).direction;

    // Globe.
    gl.enable(gl.DEPTH_TEST);
    gl.depthFunc(gl.LEQUAL);
    gl.enable(gl.CULL_FACE);
    gl.cullFace(gl.BACK);
    gl.frontFace(gl.CCW);
    this.drawGlobe(this.globeProgram, frame, 1, sun);

    // Atmosphere: back faces of a slightly larger shell, added on top.
    gl.cullFace(gl.FRONT);
    gl.depthMask(false);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE);
    this.drawGlobe(this.haloProgram, frame, 1.025, sun);
    gl.cullFace(gl.BACK);
    gl.disable(gl.CULL_FACE);

    // Overlays: depth-tested against the globe, but never written.
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    if (this.layers.graticule) this.drawLines(this.graticule, frame, 1.0006, [0.86, 0.91, 0.96, 0.22]);
    if (this.layers.tiles && this.tiles) this.drawLines(this.tiles, frame, 1.0008, [0.38, 0.85, 0.95, 0.55]);
    if (this.arc) {
      gl.lineWidth(2);
      this.drawLines(this.arc, frame, 1.001, [0.894, 0.639, 0.133, 1]);
      gl.lineWidth(1);
    }
    this.drawMarkers(frame);
    gl.depthMask(true);
    gl.disable(gl.BLEND);
  }

  private drawGlobe(p: Program, frame: CameraFrame, scale: number, sun: [number, number, number]): void {
    const gl = this.gl;
    gl.useProgram(p.program);
    gl.uniformMatrix4fv(p.uniform("u_viewProjection"), false, frame.viewProjection);
    gl.uniform1f(p.uniform("u_scale"), scale);
    gl.uniform3f(p.uniform("u_eye"), frame.eye[0], frame.eye[1], frame.eye[2]);
    gl.uniform3f(p.uniform("u_sun"), sun[0], sun[1], sun[2]);
    gl.uniform1f(p.uniform("u_sunLighting"), this.layers.sunLighting ? 1 : 0);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.uniform1i(p.uniform("u_texture"), 0);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.globe.vertices);
    const attribs: [string, number, number][] = [
      ["a_position", 3, 0],
      ["a_normal", 3, 12],
      ["a_uv", 2, 24],
    ];
    const enabled: number[] = [];
    for (const [name, size, offset] of attribs) {
      const loc = p.attrib(name);
      if (loc < 0) continue;
      gl.enableVertexAttribArray(loc);
      gl.vertexAttribPointer(loc, size, gl.FLOAT, false, 32, offset);
      enabled.push(loc);
    }
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.globe.indices);
    gl.drawElements(gl.TRIANGLES, this.globe.count, gl.UNSIGNED_INT, 0);
    for (const loc of enabled) gl.disableVertexAttribArray(loc);
  }

  private drawLines(lines: LineBuffer, frame: CameraFrame, scale: number, color: [number, number, number, number]): void {
    this.drawPrimitives(lines.buffer, lines.count, this.gl.LINES, frame, scale, color, 1, false);
  }

  private drawPrimitives(
    buffer: WebGLBuffer,
    count: number,
    mode: number,
    frame: CameraFrame,
    scale: number,
    color: [number, number, number, number],
    pointSize: number,
    round: boolean,
  ): void {
    const gl = this.gl;
    const p = this.lineProgram;
    gl.useProgram(p.program);
    gl.uniformMatrix4fv(p.uniform("u_viewProjection"), false, frame.viewProjection);
    gl.uniform1f(p.uniform("u_scale"), scale);
    gl.uniform4f(p.uniform("u_color"), color[0], color[1], color[2], color[3]);
    gl.uniform1f(p.uniform("u_pointSize"), pointSize);
    gl.uniform1f(p.uniform("u_round"), round ? 1 : 0);
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    const loc = p.attrib("a_position");
    gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 3, gl.FLOAT, false, 12, 0);
    gl.drawArrays(mode, 0, count);
    gl.disableVertexAttribArray(loc);
  }

  private drawMarkers(frame: CameraFrame): void {
    const gl = this.gl;
    for (const m of this.markers) {
      gl.bindBuffer(gl.ARRAY_BUFFER, this.markerBuffer);
      gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(this.engine.toCartesian(m.lon, m.lat, 0)), gl.DYNAMIC_DRAW);
      this.drawPrimitives(this.markerBuffer, 1, gl.POINTS, frame, 1.002, [...m.color, 1], m.size * this.pixelRatio, true);
    }
  }

  private updateTiles(): void {
    if (!this.layers.tiles) return;
    const c = this.camera;
    const level = this.engine.level(c.range);
    const key = `${level}:${this.layers.tileScheme}:${Math.round(c.lon / RAD)}:${Math.round(c.lat / RAD)}`;
    if (key === this.tileKey) return;
    this.tileKey = key;
    this.freeLines(this.tiles);
    this.tiles = this.lines(this.engine.tiles(level, this.layers.tileScheme, c.lon, c.lat, 3));
    this.onTileLevel(level);
  }

  /** Meters on the ground per CSS pixel at the focus point. */
  private metersPerPixel(): number {
    return (2 * this.camera.range * Math.tan(this.fovY() / 2)) / Math.max(1, this.canvas.clientHeight);
  }

  private orbit(dx: number, dy: number): void {
    const c = this.camera;
    const k = this.metersPerPixel() / RADIUS;
    const sh = Math.sin(c.heading);
    const ch = Math.cos(c.heading);
    // Dragging moves the ground with the finger, so the focus moves the other way.
    const north = (dx * sh + dy * ch) * k;
    const east = (-dx * ch + dy * sh) * k;
    c.lat = clamp(c.lat + north, -89.5 * RAD, 89.5 * RAD);
    c.lon = wrapLon(c.lon + east / Math.max(Math.cos(c.lat), 0.05));
  }

  private tilt(dx: number, dy: number): void {
    const c = this.camera;
    c.heading = wrapLon(c.heading + dx * 0.006);
    c.pitch = clamp(c.pitch - dy * 0.005, MIN_PITCH, MAX_PITCH);
  }

  private zoom(factor: number): void {
    this.camera.range = clamp(this.camera.range * factor, MIN_RANGE, MAX_RANGE);
  }

  private pick(clientX: number, clientY: number): { lon: number; lat: number } | null {
    if (!this.frame) return null;
    const rect = this.canvas.getBoundingClientRect();
    const x = ((clientX - rect.left) / rect.width) * 2 - 1;
    const y = 1 - ((clientY - rect.top) / rect.height) * 2;
    // Rebuild the frame so the Rust pick uses this exact camera.
    this.computeFrame();
    const hit = this.engine.pick(x, y);
    return hit ? { lon: hit.longitude, lat: hit.latitude } : null;
  }

  private attachInput(): void {
    const el = this.canvas;
    const on = <K extends keyof HTMLElementEventMap>(type: K, fn: (e: HTMLElementEventMap[K]) => void, opts?: AddEventListenerOptions) => {
      el.addEventListener(type, fn as EventListener, opts);
      this.disposers.push(() => el.removeEventListener(type, fn as EventListener, opts));
    };

    const pinchInfo = () => {
      const [a, b] = [...this.pointers.values()];
      return {
        dist: Math.hypot(b.x - a.x, b.y - a.y),
        angle: Math.atan2(b.y - a.y, b.x - a.x),
        midY: (a.y + b.y) / 2,
      };
    };

    on("contextmenu", (e) => e.preventDefault());

    on("pointerdown", (e) => {
      el.setPointerCapture(e.pointerId);
      this.pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
      this.flight = null;
      if (this.pointers.size === 2) {
        const p = pinchInfo();
        this.gesture = {
          kind: "pinch",
          startX: e.clientX,
          startY: e.clientY,
          startTime: performance.now(),
          moved: true,
          pinchDist: p.dist,
          pinchAngle: p.angle,
          pinchMidY: p.midY,
        };
        return;
      }
      if (this.pointers.size > 2) return;
      const tiltButton = e.button === 2 || e.button === 1 || e.ctrlKey || e.shiftKey;
      this.gesture = {
        kind: tiltButton ? "tilt" : "orbit",
        startX: e.clientX,
        startY: e.clientY,
        startTime: performance.now(),
        moved: false,
        pinchDist: 0,
        pinchAngle: 0,
        pinchMidY: 0,
      };
    });

    on("pointermove", (e) => {
      const prev = this.pointers.get(e.pointerId);
      if (!prev || !this.gesture) return;
      const dx = e.clientX - prev.x;
      const dy = e.clientY - prev.y;
      this.pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
      const g = this.gesture;
      if (Math.hypot(e.clientX - g.startX, e.clientY - g.startY) > 5) g.moved = true;
      if (g.kind === "pinch" && this.pointers.size >= 2) {
        const p = pinchInfo();
        if (g.pinchDist > 0 && p.dist > 0) this.zoom(g.pinchDist / p.dist);
        let turn = p.angle - g.pinchAngle;
        turn = Math.atan2(Math.sin(turn), Math.cos(turn));
        this.tilt(-turn / 0.006, 0);
        this.tilt(0, (p.midY - g.pinchMidY) * 1.2);
        g.pinchDist = p.dist;
        g.pinchAngle = p.angle;
        g.pinchMidY = p.midY;
      } else if (g.kind === "orbit") {
        this.orbit(dx, dy);
      } else if (g.kind === "tilt") {
        this.tilt(dx, dy);
      }
      this.invalidate();
    });

    const end = (e: PointerEvent) => {
      const g = this.gesture;
      this.pointers.delete(e.pointerId);
      if (g && g.kind !== "pinch" && !g.moved && e.type === "pointerup" && performance.now() - g.startTime < 500) {
        this.onTap(this.pick(e.clientX, e.clientY));
      }
      if (this.pointers.size === 0) {
        this.gesture = null;
      } else if (g?.kind === "pinch") {
        // One finger left: carry on orbiting from where it is.
        const [rest] = [...this.pointers.values()];
        this.gesture = { ...g, kind: "orbit", startX: rest.x, startY: rest.y, moved: true };
      }
    };
    on("pointerup", end);
    on("pointercancel", end);

    on(
      "wheel",
      (e) => {
        e.preventDefault();
        this.flight = null;
        const delta = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY;
        this.zoom(Math.exp(clamp(delta, -200, 200) * 0.0015));
        this.invalidate();
      },
      { passive: false },
    );
  }
}

export const DEFAULT_CAMERA: CameraState = {
  lon: -86.71 * RAD,
  lat: 30.05 * RAD,
  heading: 0,
  pitch: -90 * RAD,
  range: 20000e3,
};

export { RAD };
