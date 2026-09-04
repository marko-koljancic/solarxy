# Provenance of the sample models

These are data files that travel with the source tree, not code: nothing in
this directory is covered by the repository's GPL-3.0-or-later license, and
nothing in these files' own terms extends to the code. Each entry below
records where a file came from and on what terms it is here, which is what
stops that research being repeated.

The Stanford models below may be redistributed for free with credit, but
they are "not to be used for commercial purposes, nor should they appear in
a product for sale" without the Stanford Computer Graphics Laboratory's
permission. Before any publication of this repository to a package registry
such as crates.io, this directory and `res/textures/` must therefore be
excluded from the published archive (the root manifest currently sets no
exclude list; a registry archive is permanent).

## xyzrgb_dragon.obj

The XYZ RGB Dragon from the Stanford 3D Scanning Repository
(https://graphics.stanford.edu/data/3Dscanrep/), scanned by XYZ RGB Inc.
Credit: data courtesy of the Stanford Computer Graphics Laboratory. The
repository asks that uses of this model stay in good taste: no morphing,
boolean operations, or simulated harm. Used as the untextured subject of
the golden-capture gate, the desktop QA checklist's headline model, and
the raycast performance test.

## armadillo.obj

The Armadillo from the same Stanford repository, scanned by the Stanford
Computer Graphics Laboratory on a Cyberware 3030 MS. Credit as above; no
per-model restriction. Used as the standard subject of the render command's
test suite.

## happy.obj

The Happy Buddha from the same Stanford repository, scanned by the Stanford
Computer Graphics Laboratory. Credit as above, and the same good-taste
clause as the dragon, on a model of a religious figure. Referenced by no
test or script; kept as a manual test subject.

## knot/

First-party. Generated, outputs and all, by `knot/gen.py` in this
repository: a trefoil torus knot with a banded, speckled diffuse texture,
deterministic from a fixed seed. It replaced an earlier textured sample
whose origin could not be established. Regenerate with
`python3 res/models/knot/gen.py`; the outputs are byte-identical on any
platform. Used as the textured subject of the golden-capture gate and the
multi-file import tests.
