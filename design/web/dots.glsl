/** @resolution */
uniform vec2 u_resolution;

/**
 * @label Gap (px)
 * @range 6.0, 60.0
 * @default 18.0
 */
uniform float u_gap;

/**
 * @label Dot radius (px)
 * @range 0.5, 4.0
 * @default 1.0
 */
uniform float u_radius;

/**
 * @label Dot color
 * @color
 * @default #3c3c3c
 */
uniform vec3 u_dot;

/**
 * @label Background
 * @color
 * @default #242424
 */
uniform vec3 u_bg;

void main() {
  vec2 p = mod(gl_FragCoord.xy, u_gap);
  vec2 d = p - vec2(u_gap * 0.5);
  float dist = length(d);
  float a = 1.0 - smoothstep(u_radius - 0.5, u_radius + 0.5, dist);
  vec3 c = mix(u_bg, u_dot, a);
  gl_FragColor = vec4(c, 1.0);
}
