/** @resolution */
uniform vec2 u_resolution;

/**
 * @label Slot pitch (px)
 * @range 8.0, 48.0
 * @default 22.0
 */
uniform float u_pitch;

/**
 * @label Divider color
 * @color
 * @default #000000
 */
uniform vec3 u_line;

/**
 * @label Divider alpha
 * @range 0.0, 0.5
 * @default 0.10
 */
uniform float u_alpha;

/**
 * @label Top highlight alpha
 * @range 0.0, 0.4
 * @default 0.14
 */
uniform float u_hi;

/**
 * @label Bottom shade alpha
 * @range 0.0, 0.4
 * @default 0.16
 */
uniform float u_lo;

void main() {
  vec2 p = gl_FragCoord.xy;
  float h = max(u_resolution.y, 1.0);

  // Vertical segmentation slots: a 1px divider every u_pitch px.
  float d = abs(mod(p.x, u_pitch) - u_pitch * 0.5);
  float line = 1.0 - smoothstep(0.0, 1.0, d);

  // Top-down shading: light at the top edge, darker toward the bottom.
  float t = clamp(p.y / h, 0.0, 1.0);
  float hi = u_hi * (1.0 - smoothstep(0.0, 0.45, t));
  float lo = u_lo * smoothstep(0.55, 1.0, t);

  vec3 col = mix(u_line, vec3(1.0), step(0.5, hi));
  float aLine = line * u_alpha;

  // Composite: white highlight, black shade, then the dividers on top.
  float aWhite = hi;
  float aBlack = lo + aLine;

  vec3 rgb = (vec3(1.0) * aWhite + u_line * aBlack) / max(aWhite + aBlack, 0.0001);
  float a = aWhite + aBlack;

  gl_FragColor = vec4(rgb * a, a);
}
