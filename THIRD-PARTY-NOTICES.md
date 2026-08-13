# Third-party notices

Solarxy is MIT licensed; see `LICENSE`. This file covers the third-party work Solarxy
**ports code from**, which its licenses require to be carried with every distribution.

It is deliberately not a list of dependencies. Solarxy's Cargo and npm dependencies are
declared in `Cargo.lock` and `web/package-lock.json` and carry their own terms; what is
recorded here is source that was read, translated and adapted into Solarxy's own files,
where the obligation is to reproduce the copyright notice and the license text.

Below the license texts, a second section credits the published algorithms Solarxy
implements. Those are citations rather than obligations: an algorithm is not copyrightable
and no license attaches, but a reader deserves to know what a piece of shader code is an
implementation of.

## three-gpu-pathtracer

The path-traced renderer introduced in 0.9.0 ports substantial portions of this project's
WebGPU implementation into WGSL and Rust: the material response and its layer operators,
next-event estimation with multiple importance sampling, environment importance sampling,
and the pseudo-random generators the estimator draws from. The files that carry the ported
logic are under `crates/solarxy-renderer/src/shaders/pathtrace/`.

Source: https://github.com/gkjohnson/three-gpu-pathtracer

```
MIT License

Copyright (c) 2021 Garrett Johnson

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## three-mesh-bvh

The bounding-volume hierarchy Solarxy builds and traverses follows this project's node
layout and traversal structure. The build and the CPU traversal are in
`crates/solarxy-bvh/`; the WGSL traversal that must agree with it term for term is in
`crates/solarxy-renderer/src/shaders/pathtrace/traverse.wgsl`.

Source: https://github.com/gkjohnson/three-mesh-bvh

```
MIT License

Copyright (c) 2018 Garrett Johnson

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Published algorithms Solarxy implements

No license attaches to any of these. They are recorded so that a reader of the shader can
find the paper it came from.

- **Sampling the GGX distribution of visible normals.** Eric Heitz, "Sampling the GGX
  Distribution of Visible Normals" (Journal of Computer Graphics Techniques, 2018), and the
  bounded-VNDF formulation. Implemented in
  `crates/solarxy-renderer/src/shaders/pathtrace/bsdf.wgsl`.

- **The edge-avoiding a-trous wavelet filter.** Holger Dammertz, Daniel Sewtz, Johannes
  Hanika and Hendrik Lensch, "Edge-Avoiding A-Trous Wavelet Transform for Fast Global
  Illumination Filtering" (High Performance Graphics, 2010). Implemented in
  `crates/solarxy-renderer/src/shaders/pathtrace/denoise.wgsl`.

  Solarxy's denoiser is this filter rather than the bilateral kernel the reference above
  uses. That is a choice about image quality at a given cost, and it is also why no
  BSD-2-Clause notice appears in this file: the filter that would have required one is not
  the filter that shipped.

- **The PCG family of pseudo-random generators.** Melissa O'Neill, https://www.pcg-random.org.
  Solarxy uses the four-dimensional variant. Implemented in
  `crates/solarxy-renderer/src/shaders/pathtrace/rand.wgsl`.

- **The MurmurHash3 finalizer**, by Austin Appleby, released into the public domain. Used to
  scramble per-dimension seeds rather than as a hash. Same file as above.

Two sources the reference credits are deliberately **absent** from this list, because the
code that would have required them was not ported: the Sobol sequence and its blue-noise
dither. Solarxy stratifies its own way, for reasons recorded in `rand.wgsl`, and carrying
credits for machinery it does not contain would make every other entry here less
trustworthy.
