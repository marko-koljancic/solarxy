# Third-party notices

Solarxy is licensed under GPL-3.0-or-later; see `LICENSE`. Releases through 0.8.2 were
published under the MIT license, and that grant cannot be withdrawn, so those versions stay
MIT for anyone who holds them. 0.9.0 is where the present terms begin.

This file carries three different kinds of thing, and the differences matter. The first section
is an **additional permission** granted under section 7 of the GPL. That is a term of Solarxy's
own license rather than an attribution, and it is here because it has to travel with every copy.
After it comes the third-party work Solarxy **ports code from**, whose licenses require the
notice to be carried with every distribution. After that come the typefaces Solarxy **bundles
as data**, whose licenses ask the same of their own texts, and which are a different kind of
thing again because a font is rendered with rather than built from.

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

Each covered work's own license text travels with it: the layout library ships its license in
its own package, and every bundled typeface's text is reproduced in the Bundled typefaces
section below, which this file carries into every distributed artifact.

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

## Bundled typefaces

Everything above this section is code Solarxy ports; what follows is font software Solarxy
**bundles as data**. The two obligations differ, which is why they are not blended: a ported
file carries another author's code into Solarxy's own, while a bundled font is rendered with
and never becomes part of the program. What the font licenses ask is that their text and
copyright travel with the font software, and this file, which ships inside every artifact,
is where they do.

The section 7 additional permission at the top of this file is the other half of this
section: it is what authorizes distributing these fonts inside a GPL-licensed whole, and it
names the two font licenses involved. A reader who has found either should know the other
exists.

Which artifact carries which family:

- **Lilex** (Copyright 2019 The Lilex Project Authors, https://github.com/mishamyrt/Lilex;
  SIL Open Font License 1.1). The desktop monospace typeface, fronting the code-and-numbers
  surfaces (the console, numeric readouts, shortcut keys and file paths), compiled into the
  `solarxy` binary from `res/Lilex/static/Lilex-Medium.ttf`, beside which `res/Lilex/OFL.txt`
  carries the same grant in the source tree. The renderer's committed glyph atlas,
  `crates/solarxy-renderer/src/shaders/label_atlas.r8`, is a signed-distance bitmap baked
  from the same face by `examples/gen_glyph_atlas.rs`: it is treated as a derivative of the
  Font Software and travels under the same license recorded here. Lilex declares no
  Reserved Font Name, and the atlas presents no font name to anyone, so the naming clause
  constrains nothing either way.

- **Ubuntu** (Copyright 2010, 2011 Canonical Ltd; Ubuntu Font Licence 1.0), **Hack**
  (Copyright 2018 Source Foundry Authors; MIT, over the Bitstream Vera and DejaVu lineage
  its license text below records), **Noto Emoji** (Copyright 2013 Google LLC; SIL Open Font
  License 1.1) and **emoji-icon-font** (Copyright 2014 John Slegers; MIT). The interface
  toolkit's default faces, compiled into the desktop binary, and into the command-line
  binary when it is built with the `watch` feature. They stay enabled deliberately: Inter
  and Lilex sit first in their families and these supply the symbol and emoji coverage the
  interface leans on, the review category glyphs among it, so disabling them would trade
  four notices for missing glyphs. The decision is recorded beside the dependency that would undo it, on
  the `egui` line of `crates/solarxy-app/Cargo.toml`.

- **Inter** (Copyright 2016 The Inter Project Authors, https://github.com/rsms/inter; SIL
  Open Font License 1.1) and **IBM Plex Mono** (Copyright 2017 IBM Corp.; SIL Open Font
  License 1.1). Inter is the interface typeface of both shells: the web application bundles
  it into the build as web-font subsets, and the desktop compiles the static Medium weight
  from the Inter 4.1 release into the `solarxy` binary from `res/Inter/Inter-Medium.ttf`,
  beside which `res/Inter/OFL.txt` carries the same grant in the source tree. IBM Plex Mono
  is the web application's monospace face, bundled as web-font subsets. IBM declares the
  Reserved Font Name "Plex"; Solarxy renames nothing and presents the family under its own
  name, so the clause constrains nothing.

- **Space Grotesk** (Copyright 2020 The Space Grotesk Project Authors,
  https://github.com/floriankarsten/space-grotesk), **Space Mono** (Copyright 2016 The
  Space Mono Project Authors, https://github.com/googlefonts/spacemono) and **Instrument
  Serif** (Copyright 2022 The Instrument Serif Project Authors,
  https://github.com/Instrument/instrument-serif); all SIL Open Font License 1.1. The
  public pages' faces, served as web fonts from the site's own origin, from
  `web/public/fonts/`.

The license texts follow, once each. The copyright lines above are the per-family headers
they attach to.

### SIL Open Font License, Version 1.1

Covers Lilex, Noto Emoji, Inter, IBM Plex Mono, Space Grotesk, Space Mono and Instrument
Serif, each under its copyright line above.

```
SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007
-----------------------------------------------------------

PREAMBLE
The goals of the Open Font License (OFL) are to stimulate worldwide
development of collaborative font projects, to support the font creation
efforts of academic and linguistic communities, and to provide a free and
open framework in which fonts may be shared and improved in partnership
with others.

The OFL allows the licensed fonts to be used, studied, modified and
redistributed freely as long as they are not sold by themselves. The
fonts, including any derivative works, can be bundled, embedded, 
redistributed and/or sold with any software provided that any reserved
names are not used by derivative works. The fonts and derivatives,
however, cannot be released under any other type of license. The
requirement for fonts to remain under this license does not apply
to any document created using the fonts or their derivatives.

DEFINITIONS
"Font Software" refers to the set of files released by the Copyright
Holder(s) under this license and clearly marked as such. This may
include source files, build scripts and documentation.

"Reserved Font Name" refers to any names specified as such after the
copyright statement(s).

"Original Version" refers to the collection of Font Software components as
distributed by the Copyright Holder(s).

"Modified Version" refers to any derivative made by adding to, deleting,
or substituting -- in part or in whole -- any of the components of the
Original Version, by changing formats or by porting the Font Software to a
new environment.

"Author" refers to any designer, engineer, programmer, technical
writer or other person who contributed to the Font Software.

PERMISSION & CONDITIONS
Permission is hereby granted, free of charge, to any person obtaining
a copy of the Font Software, to use, study, copy, merge, embed, modify,
redistribute, and sell modified and unmodified copies of the Font
Software, subject to the following conditions:

1) Neither the Font Software nor any of its individual components,
in Original or Modified Versions, may be sold by itself.

2) Original or Modified Versions of the Font Software may be bundled,
redistributed and/or sold with any software, provided that each copy
contains the above copyright notice and this license. These can be
included either as stand-alone text files, human-readable headers or
in the appropriate machine-readable metadata fields within text or
binary files as long as those fields can be easily viewed by the user.

3) No Modified Version of the Font Software may use the Reserved Font
Name(s) unless explicit written permission is granted by the corresponding
Copyright Holder. This restriction only applies to the primary font name as
presented to the users.

4) The name(s) of the Copyright Holder(s) or the Author(s) of the Font
Software shall not be used to promote, endorse or advertise any
Modified Version, except to acknowledge the contribution(s) of the
Copyright Holder(s) and the Author(s) or with their explicit written
permission.

5) The Font Software, modified or unmodified, in part or in whole,
must be distributed entirely under this license, and must not be
distributed under any other license. The requirement for fonts to
remain under this license does not apply to any document created
using the Font Software.

TERMINATION
This license becomes null and void if any of the above conditions are
not met.

DISCLAIMER
THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT
OF COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL THE
COPYRIGHT HOLDER BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL
DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM
OTHER DEALINGS IN THE FONT SOFTWARE.
```

### Ubuntu Font Licence, Version 1.0

Covers the Ubuntu face.

```
-------------------------------
UBUNTU FONT LICENCE Version 1.0
-------------------------------

PREAMBLE
This licence allows the licensed fonts to be used, studied, modified and
redistributed freely. The fonts, including any derivative works, can be
bundled, embedded, and redistributed provided the terms of this licence
are met. The fonts and derivatives, however, cannot be released under
any other licence. The requirement for fonts to remain under this
licence does not require any document created using the fonts or their
derivatives to be published under this licence, as long as the primary
purpose of the document is not to be a vehicle for the distribution of
the fonts.

DEFINITIONS
"Font Software" refers to the set of files released by the Copyright
Holder(s) under this licence and clearly marked as such. This may
include source files, build scripts and documentation.

"Original Version" refers to the collection of Font Software components
as received under this licence.

"Modified Version" refers to any derivative made by adding to, deleting,
or substituting -- in part or in whole -- any of the components of the
Original Version, by changing formats or by porting the Font Software to
a new environment.

"Copyright Holder(s)" refers to all individuals and companies who have a
copyright ownership of the Font Software.

"Substantially Changed" refers to Modified Versions which can be easily
identified as dissimilar to the Font Software by users of the Font
Software comparing the Original Version with the Modified Version.

To "Propagate" a work means to do anything with it that, without
permission, would make you directly or secondarily liable for
infringement under applicable copyright law, except executing it on a
computer or modifying a private copy. Propagation includes copying,
distribution (with or without modification and with or without charging
a redistribution fee), making available to the public, and in some
countries other activities as well.

PERMISSION & CONDITIONS
This licence does not grant any rights under trademark law and all such
rights are reserved.

Permission is hereby granted, free of charge, to any person obtaining a
copy of the Font Software, to propagate the Font Software, subject to
the below conditions:

1) Each copy of the Font Software must contain the above copyright
notice and this licence. These can be included either as stand-alone
text files, human-readable headers or in the appropriate machine-
readable metadata fields within text or binary files as long as those
fields can be easily viewed by the user.

2) The font name complies with the following:
(a) The Original Version must retain its name, unmodified.
(b) Modified Versions which are Substantially Changed must be renamed to
avoid use of the name of the Original Version or similar names entirely.
(c) Modified Versions which are not Substantially Changed must be
renamed to both (i) retain the name of the Original Version and (ii) add
additional naming elements to distinguish the Modified Version from the
Original Version. The name of such Modified Versions must be the name of
the Original Version, with "derivative X" where X represents the name of
the new work, appended to that name.

3) The name(s) of the Copyright Holder(s) and any contributor to the
Font Software shall not be used to promote, endorse or advertise any
Modified Version, except (i) as required by this licence, (ii) to
acknowledge the contribution(s) of the Copyright Holder(s) or (iii) with
their explicit written permission.

4) The Font Software, modified or unmodified, in part or in whole, must
be distributed entirely under this licence, and must not be distributed
under any other licence. The requirement for fonts to remain under this
licence does not affect any document created using the Font Software,
except any version of the Font Software extracted from a document
created using the Font Software may only be distributed under this
licence.

TERMINATION
This licence becomes null and void if any of the above conditions are
not met.

DISCLAIMER
THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL THE
COPYRIGHT HOLDER BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL
DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM OTHER
DEALINGS IN THE FONT SOFTWARE.
```

### Hack

The license stack Hack itself ships, reproduced whole because the face carries three
lineages with three sets of terms.

```
The work in the Hack project is Copyright 2018 Source Foundry Authors and licensed under the MIT License

The work in the DejaVu project was committed to the public domain.

Bitstream Vera Sans Mono Copyright 2003 Bitstream Inc. and licensed under the Bitstream Vera License with Reserved Font Names "Bitstream" and "Vera"
MIT License

Copyright (c) 2018 Source Foundry Authors

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
BITSTREAM VERA LICENSE

Copyright (c) 2003 by Bitstream, Inc. All Rights Reserved. Bitstream Vera is a trademark of Bitstream, Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy of the fonts accompanying this license ("Fonts") and associated documentation files (the "Font Software"), to reproduce and distribute the Font Software, including without limitation the rights to use, copy, merge, publish, distribute, and/or sell copies of the Font Software, and to permit persons to whom the Font Software is furnished to do so, subject to the following conditions:

The above copyright and trademark notices and this permission notice shall be included in all copies of one or more of the Font Software typefaces.

The Font Software may be modified, altered, or added to, and in particular the designs of glyphs or characters in the Fonts may be modified and additional glyphs or characters may be added to the Fonts, only if the fonts are renamed to names not containing either the words "Bitstream" or the word "Vera".

This License becomes null and void to the extent applicable to Fonts or Font Software that has been modified and is distributed under the "Bitstream Vera" names.

The Font Software may be sold as part of a larger software package but no copy of one or more of the Font Software typefaces may be sold by itself.

THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL BITSTREAM OR THE GNOME FOUNDATION BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM OTHER DEALINGS IN THE FONT SOFTWARE.

Except as contained in this notice, the names of Gnome, the Gnome Foundation, and Bitstream Inc., shall not be used in advertising or otherwise to promote the sale, use or other dealings in this Font Software without prior written authorization from the Gnome Foundation or Bitstream Inc., respectively. For further information, contact: fonts at gnome dot org.
```

### emoji-icon-font

```
MIT License

Copyright (c) 2014 John Slegers

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
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
