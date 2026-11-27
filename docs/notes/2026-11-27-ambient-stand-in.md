# R-11: a stand-in for the indirect light, so rooms and faces out of the sun are not black

The title lights everything out of direct sun with rays. Without built ray-tracing structures (R-1) its
rays find nothing: rooms, faces and the shade side of things come out black, and in tiles of the picture
the probes' memory shows through as coloured blocks. The same probes light the air of a room, which then
holds the sky's light indoors as a white veil. The goal here is a clean picture, not ray tracing: this
puts a cheap estimate where the rays' result goes, and leaves the rays alone.

New in `crates/leonida-gpu/src/ambient.rs`, hooked after a dispatch in `draw.rs`. Off by default.

## How it works

Right after the title's program that resolves the diffuse light of its screen probes, a few compute
passes raise that light to an estimate and bound what the program left by a multiple of it:

- The light the visible surfaces send out (the title keeps it, from the frame before, for its own screen
  traces) is averaged in 64x36 cells, with a slow mean of the last seconds.
- At 160x90 points every point gathers the cells, weighted by the share of its surroundings they cover,
  as a mean and a direction (first-order harmonics).
- Each pixel takes the points around it by depth and normal (from the depth), shaded in creases from
  the depth, and keeps the larger of that and the title's own light, the latter bounded.
- Probe leftovers that are magenta or violet (green far under red and blue) are dropped first: the
  bound weighs blue at 0.07 only, so they would pass it and glow on bright surfaces.
- With `LEONIDA_AMBIENT_AIR` set, the light the air scatters is bounded by a multiple of the light
  around, and NaN or infinite cells (black or white patches in the air) are cleared.

shortcut: what is not on screen is taken to send out what is, and nothing occludes but the creases.

## Finding the programs

No shader names or hashes of the title are in the code or tests. The user names the programs at run
time in `LEONIDA_AMBIENT_PROGRAMS`, as `<role>:<program>` entries separated by commas: the role is
`resolve`, `radiance` or `air`, the program its 16-digit shader hash as the log prints it, or a name of
8 characters or more that its container holds. Compute programs only; asked once per shader.

After a dispatch of such a program the hook still checks what it is about to write, and records nothing
if any check fails (it logs the reason once): the view at the program's table place is the kind the
program's code reads there; the image is `R32_UINT` (holding R9G9B9E5) for the resolve and
`R11G11B10_FLOAT` otherwise, 3D for the air only; the depth has the image's size and is not a stencil
view (plane 1 of a 2D view, or a stencil-only format). The table places themselves (root parameter and
dword) are constants in `ambient.rs`. The stand-in replaces the list's compute root signature and
pipeline, so the next bind sets them again.

It runs only while the structure builder has not run whole (`pipeline::builder_whole`, new, read-only).

## Switches

| Switch | Default | What it does |
| --- | --- | --- |
| `LEONIDA_AMBIENT` | off | A number: that share of the estimate (1: all). `constant:<level>`: a flat level. |
| `LEONIDA_AMBIENT_PROGRAMS` | empty | The programs above. Empty: nothing is followed (a warning says so). |
| `LEONIDA_AMBIENT_SHADE` | `1,0.6` | Strength and radius in metres of the creases' shade. |
| `LEONIDA_AMBIENT_MOST` | 4 | How many times the estimate the title's own light may keep. Lower it for bright rooms. |
| `LEONIDA_AMBIENT_AIR` | off | The most the air scatters, as a multiple of the light around (against the veil). |
| `LEONIDA_AMBIENT_ONCE` | off | `1`: one bounce of the title's own light, without the stand-in's light fed back. |

A starting point for a clean picture: `LEONIDA_AMBIENT=0.8`, `LEONIDA_AMBIENT_AIR=1.5`,
`LEONIDA_AMBIENT_MOST=2`.

## Cost

Two buffers (about 1 MB) and, per frame, a 64x36 and a 160x90 pass plus one full-resolution pass with 12
depth taps a pixel. Not measured in the game; it should be small next to frames of 140 ms and more, so it
should not put 4 fps out of reach.

## Measured

Nothing in the game yet: no card, scene or before-and-after figures. The passes compile with the system's
HLSL compiler and are created on the test machine's D3D12 device (`the_passes_build`). Unit tests cover
the switch parsing, the program list, and the feedback model of `LEONIDA_AMBIENT_ONCE`.

## Not done

- A stand-in for the reflections the rays would give (the sky's light in a mirror where a ray misses).
- The table places are constants, not found by scanning the tables by format and size.
- `LEONIDA_AMBIENT_PROGRAMS` has to be filled in by hand from the log.

## Credits

That a stand-in can be put after the probes' resolve, and in which image, is beloooo's find in
VIctory-Belobug-AMD (06031237, 4f45ab88, cd9f78ce, GPL-3.0-or-later), which puts a constant there shaded
in creases; the crease shade and the magenta filter follow it. Work on the stand-in's air bound, its
binding checks and the stencil check comes from
[readmet3xt-lab/foxhound](https://github.com/readmet3xt-lab/foxhound).
