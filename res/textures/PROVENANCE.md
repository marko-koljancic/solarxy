# Provenance of the sample textures

These are data files that travel with the source tree, not code: nothing in
this directory is covered by the repository's GPL-3.0-or-later license, and
nothing in these files' own terms extends to the code. See
`res/models/PROVENANCE.md` for the registry-publication rule that covers
both directories.

## uv-checker_1k.png, uv-checker_2k.png, uv-checker_4k.png

Generated with UV Checker Map Maker by Jorge Valle
(https://uvchecker.byvalle.com/, hosted at https://uvchecker.atlux.one/ as
of 2026). The tool's stated terms are that it is free to use, with an
optional donation on download; it publishes no formal license text, so
this credit records the source rather than a license.

The 1k map is distributed inside every shipped artifact: compiled into the
desktop binary and the render command, and bundled with the web app
(`web/src/assets/uv-checker_1k.png` is the same file). It is credited in
`THIRD-PARTY-NOTICES.md`, which travels with every artifact. The 2k and 4k
maps ship in the source tree only and are referenced by nothing; they are
kept as manual test subjects.
