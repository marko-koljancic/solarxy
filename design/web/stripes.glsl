/** @resolution */
uniform vec2 u_resolution;

/**
 * @label Stripe period (px)
 * @range 2.0, 40.0
 * @default 10.0
 */
uniform float u_period;

/**
 * @label Stripe color
 * @color
 * @default #000000
 */
uniform vec3 u_color;

/**
 * @label Stripe alpha
 * @range 0.0, 1.0
 * @default 0.08
 */
uniform float u_alpha;

void main() {
  float d = gl_FragCoord.x + gl_FragCoord.y;
  float m = mod(d, u_period);
  float a = m < u_period * 0.5 ? u_alpha : 0.0;
  gl_FragColor = vec4(u_color * a, a);
}
