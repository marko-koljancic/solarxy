/** @resolution */
uniform vec2 u_resolution;

/**
 * @label Cell size (px)
 * @range 2.0, 64.0
 * @default 10.0
 */
uniform float u_size;

/**
 * @label Color A
 * @color
 * @default #3a3a3a
 */
uniform vec3 u_a;

/**
 * @label Color B
 * @color
 * @default #2e2e2e
 */
uniform vec3 u_b;

void main() {
  vec2 cell = floor(gl_FragCoord.xy / u_size);
  float check = mod(cell.x + cell.y, 2.0);
  gl_FragColor = vec4(mix(u_a, u_b, check), 1.0);
}
