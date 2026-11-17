export interface GlassBackdrop {
  img: HTMLImageElement;
  x: number;
  y: number;
  w: number;
  h: number;
}

interface GlassShape {
  left: number;
  right: number;
  height: number;
  radius: number;
  flare: number;
  extraHeight: number;
  bottomOffset: (x: number) => number;
}

const VERTEX = `
attribute vec2 aPosition;
varying vec2 vUv;
void main() {
  vUv = aPosition * vec2(0.5, -0.5) + 0.5;
  gl_Position = vec4(aPosition, 0.0, 1.0);
}
`;

const FRAGMENT = `
precision highp float;
varying vec2 vUv;
uniform sampler2D uBackdrop;
uniform sampler2D uContour;
uniform vec4 uRegion;
uniform vec4 uSource;
uniform vec4 uShape;
uniform vec2 uProfile;
uniform float uFlare;

float distanceToEdge(vec2 p) {
  float x = p.x - uShape.x;
  float halfWidth = uShape.y;
  float radius = uShape.w;
  float t = clamp((x + halfWidth) / (2.0 * halfWidth), 0.0, 1.0);
  float profileX = mix(0.5 * uProfile.x, 1.0 - 0.5 * uProfile.x, t);
  float bottom = uShape.z + (texture2D(uContour, vec2(profileX, 0.5)).r - uProfile.y) * 96.0;
  // Match the quadratic flare at the top and the rounded corners at the bottom.
  float flareT = max(0.0, 1.0 - sqrt(max(0.0, p.y) / uFlare));
  float side = abs(x) - halfWidth - uFlare * flareT * flareT;
  vec2 q = vec2(side, p.y - bottom) + radius;
  return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - radius;
}

void main() {
  vec2 p = uRegion.xy + vUv * uRegion.zw;
  float edge = distanceToEdge(p);
  float depth = max(0.0, -edge);
  float bevel = 1.0 - smoothstep(0.0, min(34.0, uShape.z * 0.8), depth);
  vec2 gradient = vec2(
    distanceToEdge(p + vec2(1.0, 0.0)) - distanceToEdge(p - vec2(1.0, 0.0)),
    distanceToEdge(p + vec2(0.0, 1.0)) - distanceToEdge(p - vec2(0.0, 1.0))
  );
  vec2 normal = gradient / max(length(gradient), 0.001);
  vec2 ray = normal * bevel;
  vec2 src = (p + 18.0 * ray - uSource.xy) / uSource.zw;
  vec2 fringe = 0.65 * ray / uSource.zw;
  vec3 scene = vec3(
    texture2D(uBackdrop, src + fringe).r,
    texture2D(uBackdrop, src).g,
    texture2D(uBackdrop, src - fringe).b
  );
  // Compress bright backgrounds enough for the white controls to stay legible.
  float luminance = dot(scene, vec3(0.2126, 0.7152, 0.0722));
  vec3 glass = scene * mix(0.78, 0.53, luminance) + vec3(0.027, 0.029, 0.036);
  float key = max(0.0, dot(normal, vec2(-0.78, -0.62)));
  float bounce = max(0.0, normal.y);
  float rim = exp(-depth * 0.75) * (0.09 + 0.32 * key + 0.18 * bounce);
  glass += rim + 0.045 * bevel * key;
  // A capture can briefly be smaller than the animating shape. Fade to the
  // real desktop there instead of stretching the last row or column.
  vec2 fromStart = p - uSource.xy;
  vec2 fromEnd = uSource.zw - fromStart;
  float coverage = min(min(fromStart.x, fromEnd.x), fromEnd.y);
  float opacity = smoothstep(-1.0, 8.0, coverage);
  gl_FragColor = vec4(glass * opacity, opacity);
}
`;

function compile(gl: WebGLRenderingContext, type: number, source: string) {
  const shader = gl.createShader(type)!;
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (gl.getShaderParameter(shader, gl.COMPILE_STATUS)) return shader;
  const message = gl.getShaderInfoLog(shader);
  gl.deleteShader(shader);
  throw new Error(message ?? "Could not compile the glass shader");
}

function texture(gl: WebGLRenderingContext, unit: number) {
  const value = gl.createTexture();
  gl.activeTexture(gl.TEXTURE0 + unit);
  gl.bindTexture(gl.TEXTURE_2D, value);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  return value;
}

/** Draws a refracted background. The caller clips this pass to the island path. */
export class GlassRenderer {
  private canvas = document.createElement("canvas");
  private gl: WebGLRenderingContext;
  private program: WebGLProgram;
  private uniforms: Record<"region" | "source" | "shape" | "profile" | "flare", WebGLUniformLocation | null>;
  private quad: WebGLBuffer | null;
  private backdropTexture: WebGLTexture | null;
  private contourTexture: WebGLTexture | null;
  private image: HTMLImageElement | null = null;
  private profile = new Uint8Array(0);
  private onContextLost = (event: Event) => {
    event.preventDefault();
    this.redraw();
  };

  static create(redraw: () => void, restore: () => void) {
    try {
      return new GlassRenderer(redraw, restore);
    } catch {
      return null;
    }
  }

  private constructor(private redraw: () => void, private restore: () => void) {
    const gl = this.canvas.getContext("webgl", { antialias: false, depth: false, preserveDrawingBuffer: true });
    if (!gl) throw new Error("WebGL unavailable");
    this.gl = gl;
    const vertex = compile(gl, gl.VERTEX_SHADER, VERTEX);
    const fragment = compile(gl, gl.FRAGMENT_SHADER, FRAGMENT);
    const program = gl.createProgram()!;
    gl.attachShader(program, vertex);
    gl.attachShader(program, fragment);
    gl.linkProgram(program);
    gl.deleteShader(vertex);
    gl.deleteShader(fragment);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      const message = gl.getProgramInfoLog(program);
      gl.deleteProgram(program);
      throw new Error(message ?? "Could not link the glass shader");
    }
    this.program = program;
    gl.useProgram(program);
    this.quad = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, this.quad);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
    const position = gl.getAttribLocation(program, "aPosition");
    gl.enableVertexAttribArray(position);
    gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0);
    this.backdropTexture = texture(gl, 0);
    this.contourTexture = texture(gl, 1);
    gl.uniform1i(gl.getUniformLocation(program, "uBackdrop"), 0);
    gl.uniform1i(gl.getUniformLocation(program, "uContour"), 1);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    this.uniforms = {
      region: gl.getUniformLocation(program, "uRegion"),
      source: gl.getUniformLocation(program, "uSource"),
      shape: gl.getUniformLocation(program, "uShape"),
      profile: gl.getUniformLocation(program, "uProfile"),
      flare: gl.getUniformLocation(program, "uFlare"),
    };
    this.canvas.addEventListener("webglcontextlost", this.onContextLost);
    this.canvas.addEventListener("webglcontextrestored", this.restore);
  }

  destroy() {
    const { gl } = this;
    this.canvas.removeEventListener("webglcontextlost", this.onContextLost);
    this.canvas.removeEventListener("webglcontextrestored", this.restore);
    gl.deleteTexture(this.backdropTexture);
    gl.deleteTexture(this.contourTexture);
    gl.deleteBuffer(this.quad);
    gl.deleteProgram(this.program);
    this.canvas.width = this.canvas.height = 1;
  }

  draw(ctx: CanvasRenderingContext2D, backdrop: GlassBackdrop, shape: GlassShape, width: number, height: number, dpr: number) {
    const { gl, canvas, uniforms } = this;
    if (gl.isContextLost()) return false;
    const scale = Math.min(dpr, 1.5);
    const cw = Math.ceil(width * scale);
    const ch = Math.ceil(height * scale);
    if (canvas.width !== cw) canvas.width = cw;
    if (canvas.height !== ch) canvas.height = ch;
    const { left, right, flare } = shape;
    const x = Math.max(0, Math.floor((left - flare - 1) * scale));
    const w = Math.min(cw, Math.ceil((right + flare + 1) * scale)) - x;
    const h = Math.min(ch, Math.ceil((shape.height + shape.extraHeight + 1) * scale));
    if (w <= 0 || h <= 0) return false;

    if (this.image !== backdrop.img) {
      gl.activeTexture(gl.TEXTURE0);
      try {
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, backdrop.img);
      } catch {
        return false;
      }
      if (gl.getError() !== gl.NO_ERROR) return false;
      this.image = backdrop.img;
    }

    const span = right - left;
    const samples = Math.ceil(span / 2) + 1;
    if (this.profile.length !== samples) this.profile = new Uint8Array(samples);
    // Quantize displacements in a shared range to keep a two-pixel sampling step.
    // 64 represents zero, so straight sections have an exact, stable normal.
    const zero = 64;
    for (let i = 0; i < samples; i++) {
      const offset = shape.bottomOffset(left + (i / (samples - 1)) * span);
      this.profile[i] = Math.max(0, Math.min(255, Math.round(zero + (offset * 255) / 96)));
    }
    gl.activeTexture(gl.TEXTURE1);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.LUMINANCE, samples, 1, 0, gl.LUMINANCE, gl.UNSIGNED_BYTE, this.profile);

    const top = canvas.height - h;
    gl.viewport(x, top, w, h);
    gl.uniform4f(uniforms.region, x / scale, 0, w / scale, h / scale);
    gl.uniform4f(uniforms.source, backdrop.x, backdrop.y, backdrop.w, backdrop.h);
    gl.uniform4f(uniforms.shape, (left + right) / 2, span / 2, shape.height, shape.radius);
    gl.uniform2f(uniforms.profile, 1 / samples, zero / 255);
    gl.uniform1f(uniforms.flare, flare);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    ctx.drawImage(canvas, x, 0, w, h, x / scale, 0, w / scale, h / scale);
    return true;
  }
}
