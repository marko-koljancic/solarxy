# Third-party notices

Solarxy is licensed under GPL-3.0-or-later; see `LICENSE`. Releases through 0.8.2 were
published under the MIT license, and that grant cannot be withdrawn, so those versions stay
MIT for anyone who holds them. 0.9.0 is where the present terms begin.

This file carries two different kinds of thing, and the difference matters. The first section
is an **additional permission** granted under section 7 of the GPL. That is a term of Solarxy's
own license rather than an attribution, and it is here because it has to travel with every copy.
Everything after it is the third-party work Solarxy **ports code from**, whose licenses require
the notice to be carried with every distribution.

It is still not a list of dependencies. Solarxy's Cargo and npm dependencies are declared in
`Cargo.lock` and `web/package-lock.json` and carry their own terms; what is recorded below is
source that was read, translated and adapted into Solarxy's own files, where the obligation is
to reproduce the copyright notice and the license text. Two dependencies are named anyway, in
the section 7 permission, because a permission that does not say what it covers grants nothing.

After the license texts, a further section credits the published algorithms Solarxy implements.
Those are citations rather than obligations: an algorithm is not copyrightable and no license
attaches, but a reader deserves to know what a piece of shader code is an implementation of.

## Additional permission under GNU GPL version 3 section 7

Copyright (C) 2026 Marko Koljancic

If you modify Solarxy, or any covered work, by linking or combining it with the works described
below, or with modified versions of them, the licensor of Solarxy grants you additional
permission to convey the resulting work. Corresponding Source for a non-source form of such a
combination shall include the source for the parts of those works used as well as that of the
covered work.

The permission covers two classes of work, and nothing else:

- **Graph-layout software under the Eclipse Public License version 2.0.** The node canvas loads
  such a library on demand to offer a second auto-layout algorithm beside its default one. It is
  used unmodified and remains a separate file in the build output.

- **Font software under the SIL Open Font License version 1.1 or the Ubuntu Font Licence
  version 1.0.** The desktop binary embeds its interface typeface directly, and the web build
  ships web fonts. In both cases the font is data the program renders with rather than part of
  its code.

Both licenses are free software licenses that the Free Software Foundation classifies as
incompatible with the GPL, which is what makes this permission necessary rather than decorative.
Section 7 exists precisely so a copyright holder can authorize a combination the license would
otherwise refuse, and granting it is what let Solarxy adopt copyleft without deleting working
features to get there.

The permission is deliberately narrow. It names two classes of bundled work and weakens the
license in no other respect. It is stated here rather than appended to `LICENSE` so that the
text of the GNU General Public License is reproduced verbatim and unmodified, which is what
license-detection tooling matches on.

Each covered work's own license text lives with that work: the layout library ships its license
in its own package, and the desktop typeface's license is at `res/Lilex/OFL.txt` in the source
tree. Packaging those texts into the distributed artifacts alongside this file is a known gap,
tracked separately, and it predates this license change rather than being introduced by it.

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
