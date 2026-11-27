# Fork notes

The detailed log of what this fork measured and changed, moved here from the README.
The status summary is in [README.md](../README.md).

**Closed to new entries since 2026-11-27.** New notes go in [notes/](notes/), one file per change
([how to write one](notes/README.md)), so pull requests stop conflicting at the top of this file.

## Newest (2026-11-27)

**R-9: preserve cube-gradient LOD units and the minimum clamp.**
Adapted only the cube-gradient correction from
[DexNom/VIctory 921d8d3](https://github.com/DexNom/VIctory/commit/921d8d3b43b44f3a6ca4b44095edd3c4baf10a87).
DXIL's `Log` operation already returns log base 2; multiplying it by `LOG2_E`
selected overly coarse mip levels. The cube path now uses that value directly
and applies the instruction's minimum LOD for `image_sample_d_cl` before sampling.
This preserves the existing longest-gradient approximation; it does not add
anisotropic filtering. The correction follows the
[DXIL operation definition](https://github.com/microsoft/DirectXShaderCompiler/blob/main/docs/DXIL.rst)
and the [minimum-LOD clamp contract](https://learn.microsoft.com/en-us/windows/win32/direct3dhlsl/tcube-samplegrad-s-float-float-float-float-uint-).

An authored one-thread regression fills each cube mip with its own level number.
On the unchanged translator, a two-texel footprint reads 1.4414063 instead of 1,
a sixteen-texel footprint reads 5.7695313 instead of 4, and a minimum clamp of 3
reads 0. After the correction, all ten cases return the expected levels. These
include zero and tiny footprints, minimum clamps above and below the computed
LOD, and explicit-LOD/level-zero controls. Both emitted programs pass Microsoft's
DXIL validator. Measured on RTX 5090, NVIDIA 580.173.02, Pop!_OS 22.04,
Proton 11's Wine and vkd3d-proton 3.2 (`ec20719452e916b`), with Vulkan core and
synchronization validation and no unexpected test errors. This establishes the
sampled LOD correction, not a gameplay picture or frame-rate improvement.

The optional device test skips before device creation unless
`LEONIDA_TEST_CUBE_GRADIENT_LOD=hardware` or `warp` is explicitly selected:
`cargo test -p leonida-shader guest_cube_gradient_selects_mip_by_footprint -- --nocapture`.
Only the hardware mode was measured here. Its fence wait is bounded to five
seconds and terminates on failure while submitted resources remain owned.

## Newest (2026-11-26)

**Preserve null constant buffers when rebuilding descriptor tables.** A null or
empty constant-buffer view now stays a zero-address, zero-size CBV. It allocates
no upload scratch and schedules no copy. If a nonempty CBV's upload allocation
fails, the fallback is also a CBV, rather than a null texture SRV. Nonempty
uploads keep their existing byte count and 256-byte native alignment.

The [DirectX resource binding specification](https://microsoft.github.io/DirectX-Specs/d3d/ResourceBinding.html#null-descriptors)
makes a zero-sized CBV unbound and ignores its address. The old rebuilt-table
path needlessly allocated scratch for that view, then passed its nonzero address
with size zero. That is an allowed D3D12 representation, but vkd3d-proton's
embedded-descriptor path turned it into a Vulkan uniform-buffer descriptor with
zero range. Canonicalizing the null preserves D3D12 semantics and avoids that
Proton compatibility problem; growing an empty view to 256 bytes would not.
This is contract preservation, not a new rendering or performance mode.

Five CPU tests cover original title descriptor records through the production
upload helper, null/empty variants, no allocation or fill for them, allocation
failure, and unchanged nonempty uploads. The original upload behavior failed
the null allocation/descriptor regression. An additional device test uses the
full table walker and an authored one-thread shader to check zero reads from
three null variants and known reads from a nonempty control. All four cases
passed locally on an RTX 5090, NVIDIA 580.173.02, Proton 11, and official
vkd3d-proton 3.2.0 (`ec20719452e916b`), with Vulkan core/synchronization validation
enabled and no unexpected validation errors. The runtime's deliberate validation
activation canary is excluded. This device test skips by default in CI; to run it
explicitly in PowerShell:

```powershell
$env:LEONIDA_TEST_NULL_CBV = '1'
cargo test -p leonida-gpu fresh_table_null_cbvs_read_zero_on_the_device
```

No gameplay picture, stability, or frame-rate improvement is claimed by these checks.

**Wait for texture readbacks before preparing dependent submissions.**
`LEONIDA_READBACK_ORDER=1`, or the `readback-order` opt-in graphics profile,
enables this fix. It is **off by default** until a paired
`scripts\measure-run.ps1` comparison establishes its gameplay frame-time cost.
With it off, the old texture-back ranges and waits are preserved, including
the host-only treatment of rectangle expansion; no new readback waits apply.

Ordinary texture-to-buffer copies were missing from the ranges tracked while
submissions were in flight. A later submission's CPU-side buffer copy could
therefore consume stale title memory before the earlier texture readback landed.
The pending ranges now include the bytes actually written by each readback row,
excluding pitch padding. Overlapping CPU reads and writes wait for these definite
writes even when speculative shader fills are enabled; unrelated work can proceed.
The ranges remain pending until CPU copy-back completes, including when the relay
lands only part of the queue.

An original one-texel D3D12 regression holds the first submission behind a fence
and queues a dependent copy in a second submission. It releases the fence when
the consumer enters the dependency wait, or after the old implementation has
copied stale bytes; timing does not choose the outcome. On `63fb0e8` with the test
only, the destination is 0 instead of 37 in all three `LEONIDA_RELAY=0/1/2` modes.
With the fix it is 37 in all three modes, with `LEONIDA_LATE_WAIT=3`, queue2 and
the default speculative-fill path enabled. Measured on an Intel Core i9-14900KS,
RTX 5090, NVIDIA 580.173.02, Pop!_OS 22.04, Proton 11's Wine and vkd3d-proton 3.2.
Five CPU tests also cover row padding, destination overwrites, empty and
overflowing footprints, and retaining later flights' pending ranges.

Read-only CPU helper work also declares its title source range. Rectangle expansion
used to be marked host-only even though it reads title vertices; with the switch
enabled it waits for
both pending readbacks and shader writes overlapping exactly those bytes. It still
writes only host scratch memory, so it does not invalidate title-memory refreshes.
Other host-only callbacks remain independent. Overflowing access ranges conservatively
wait rather than claiming they are disjoint.

A second one-texel regression consumes 37 through an annotated host-read callback.
After merging current main at `dcc8263`, the old annotation reads 0 in relay modes
0, 1 and 2; the fixed annotation reads 37 in each. Both copy and host-read tests also
pass together in each mode, serialized by their shared test mutex even with two test
threads. Seven CPU tests pass, including the actual host-step classification and
overlap, disjoint, empty and overflowing ranges. This uses the same hardware and
runtime above and no shader dispatches or game files.

Run the optional tests in a fresh diagnostics test process with
`LEONIDA_TEST_READBACK_ORDER=1`, `LEONIDA_READBACK_ORDER=1`,
`LEONIDA_LATE_WAIT=3`, the desired
`LEONIDA_RELAY` value, and either `LEONIDA_D3D12_WARP=1` or the explicit hardware
opt-in `LEONIDA_TEST_READBACK_HARDWARE=1`:
`cargo test -p leonida-gpu --features diagnostics --locked
host::readback_order_tests -- --nocapture --test-threads=2`.

**R-3 follow-up: land a readback before preparing a dependent list.**
With the same opt-in switch enabled, this also handles lists grouped into one
submission, including lists joined by the queue. Previously all CPU preparation
ran before any of that submission's GPU work, so a later list's CPU copy could
read the old bytes despite correct ordering between separate submissions.

Only a boundary with an actual readback/CPU overlap splits the submission.
Every dependent prefix completes its GPU work, shader copy-back, texture readbacks
and completion writes before the next list is prepared with a fresh batch and card
plan. The final segment keeps the caller's asynchronous behavior. The submission
lock spans all segments; original list objects remain owned until the tail finishes,
while completed prefixes are excluded from the tail's readback landing order.
Repeated occurrences retain their execution order and are recycled once per object.

With the switch off, one submission is retained without inspecting deferred lists
or adding a split. CPU tests cover that disabled path, empty work, row padding,
independent lists, overwritten ranges, unknown accesses, overflowing ranges,
dependency chains and repeated occurrences. The optional one-texel GPU tests above
also exercise synchronous and asynchronous same-call consumers, annotated host
reads, prefix completion words, a tail overwrite that must survive, and both reads
of a repeated producer/consumer pair.

Dependencies inside one already-recorded native list still require recording-time
splitting and are not handled here. These synthetic correctness results do not
establish the cause of the washed-out game frames or claim a gameplay or frame-rate
improvement; the switch remains off by default pending paired gameplay measurements.

**R-7: an unread input takes the mode read in its register.**
`PS_RasterizeWaterVolumeExposedSurface` and the two
`PS_RasterizeVehicle*InterfaceDepth` read one input of a packed register per
vertex (`attributeAtVertex`, so constant mode), beside an element they never
read that keeps the linear mode the console's `PSV0` gives the whole
register. The console does not mix modes; the DXIL validator refused ours
("interpolation mode that differs from another element packed into the
same row"). `container::share_row_modes`, called by `Shader::finish` for
pixel inputs, gives an unread arbitrary element the mode its register's read
elements agree on (an integer only a constant one). No register or mask
changes, and the elements it changes are not read, so the picture cannot
change. A unit test covers a register read one way, one read two ways (left
alone) and an unread uint.

**R-7: a level query on an array leaves the slice undefined.**
`PS_VfxVBuffer_AlphaClip`, `PS_DeferredTextured_AlphaClip` and `PS_MirrorReflectionDeferredTextured_AlphaClip` failed the DXIL validator with "out of bound coord must be undef": `image_get_lod` on a 2D array became a `calculateLOD` that passed the slice as its third coordinate. Only the spatial coordinates are passed now, undef after them; the level does not depend on the slice, so the result is the same.

**R-7: buffer view addresses by view heap slot.**
The 5 `CS_TraceShadowRays_*` compute programs ("a descriptor read only as
data") load a buffer descriptor from the view heap at an index they compute
and only take its address, to walk the memory behind it. The host's table of
element strides by heap slot (`SLOT_STRIDES`) now goes on after its 2^21
strides with two dwords per slot, the title address of the slot's buffer view
(0: none or not a buffer, `SLOT_VIEW_ADDRESSES`), written with the stride
whenever the slot is. `view_address` reads it for a descriptor picked from
the heap, as it took the address from a user-data dword for one in a table.
The table's upload buffer grows from 8 to 24 MiB per view heap.
`CS_TraceShadowRays_FullAdaptive` then stopped on "a lane mask holding lanes
elsewhere read as data": the compiler reuses an SGPR that holds a compare's
mask on one path and a dword from `s_buffer_load` on the other, and masks the
join as a number. Structurisation took the join for a lane mask; a join with
an argument read by a scalar memory load is now no mask. With
`translate --list --validate` limited to these programs, all 5 translate and
validate.

**R-7: signature interpolation modes from the console's `PSV0`.**
The translated signatures took their interpolation modes from a table keyed
by type: every float linear, every integer constant. The console packs a
constant float (`TEXCOORD9`, `COLOR3`, beside `SV_IsFrontFace` or a uint)
into one register, which the table split into two modes, and the DXIL
validator refused the program ("interpolation mode that differs from
another element packed into the same row"). The console container's `PSV0`
lists each element's real mode; `container::take_modes` now applies it to
the vertex or mesh outputs and the pixel inputs, by register and column,
and `rect.rs` keeps it for the rectangle-list geometry stage. No register or
mask changes, and both stages of a link read the modes the console linked
them with. On `f3c94ca` that refusal stopped 70 programs (36 pixel, 21 mesh,
13 vertex); 67 now pass it. A full `translate --list --validate` against
`f3c94ca` shows 61 more programs validated and none failing that passed; the
6 others then fail with "Container part 'Program Input/Output Signature'
does not match", the refusal R-7's sort by register (#64) addresses. Not
fixed: 3 pixel programs (`PS_RasterizeWaterVolumeExposedSurface`,
`PS_RasterizeVehicle*InterfaceDepth`). A unit test reads back the modes of a
`PSV0` written by `psv`.

**R-7: signature rows in the order the DXIL validator expects.**
`container::signature` sorted the rows of one register by column. The
validator expects them in element order, as the console's containers list
them, for example `TEXCOORD16` in column 3 before `TANGENT0` in column 0.
The rows are now sorted by register only, a stable sort that keeps element
order; no register or mask changes. On `2f952dc` that mismatch made 72
programs fail with "Container part 'Program Input/Output Signature' does not
match" (33 pixel, 26 vertex, 13 mesh); their pipelines were never created,
so their draws were lost.

Not fixed: 70 programs pack elements of different interpolation modes into
one register (a constant `COLOR` or `SV_IsFrontFace` beside linear
`TEXCOORD`s), which the validator refuses. Moving an element to another
register in one program alone is not safe: D3D12 links a stage's outputs to
the next stage's inputs by register and mask too (the debug layer's
`SHADER_LINKAGE_REGISTERINDEX` and `REGISTERMASK` checks), so both sides of
a link would have to move the same way. 71 mesh programs exceed the
32,768-byte vertex plus primitive output limit, and 3 pixel programs fail
with "out of bound coord must be undef". A unit test covers the row order.
No gameplay or frame-rate improvement is claimed.

**`scripts/measure-run.ps1` reads frame lines under Windows PowerShell 5.1.**
The colour codes of `stderr.log` were stripped with the `` `e `` escape,
which only PowerShell 7 knows; under 5.1 no frame line matched and
`summary.txt` gave `lines: 0` and `NaN` for every figure. The escape is now
`[char]27`, the same under both.

**R-7: a sampler chosen by control flow between immediate samplers.**
40 programs (cascade shadows, terrain, `PS_TexturedLit*`,
`PS_TexturedDeferred`, `PS_PuddleDetect`...) failed with a descriptor
chosen by control flow: the join was between two immediate samplers (one
clamping, one wrapping), and `descriptor_phi` only joins heap loads. The
join now picks a static sampler number; the sample is made with each
sampler it can be and its channels are selected by that number. Exact, at
the cost of one more sample per affected read. A join of an immediate and a
loaded sampler is still refused. All 40 translate and pass the DXIL validator.

**R-7: a lane mask copied back one half at a time.**
`CS_TransmissionVolumetrics`, `CS_TransmissionVolumetricsSubmerged` and
`PS_VoxelizeVisiblity` park a 64-bit lane mask in VGPR lanes and read it
back with two `v_readlane_b32`, which `ir::unspill` turns into two
`s_mov_b32`. Structurisation only knew a mask whose halves one instruction
wrote, so the pair was opaque (an opaque lane mask, or a lane mask holding
lanes elsewhere read as data). A pair whose halves are copied from `s[n]`
and `s[n+1]` of one pair now takes the mask of that pair (no `shortcut:`).
All 3 translate and pass the DXIL validator; a full `translate --list
--validate` shows no other program changing.

**UP-1: keep a shader-cache miss's translation.**
Adapted from upstream VIctory
[`bca3ecd`](https://github.com/Marvisak/VIctory/commit/bca3ecd00a3e0cf4814e0061cafc510f0a286ba2).
A cache miss keeps the shader it translated instead of reading and decoding
the entry it just wrote. If publication fails, the usable translation is
returned with one warning per process about the cache problem. Previously,
that I/O failure discarded the shader and could prevent its pipeline from
being created. Translation failures still return errors; cache hits still
decode and validate the stored entry, with damaged entries translated again.
The cache format and its atomic publication are unchanged. The existing build
fingerprint includes this source file, so this update invalidates old entries
once; warm the cache again before comparing gameplay performance.

Original synthetic regression tests cover a cache folder replaced by a file,
a directory blocking publication at the entry's destination, staging-file
cleanup, translation errors, cache hits and corrupt-entry recovery. Both
publication regressions fail on `a141614` before the fix. No game data is used
and no gameplay or frame-rate improvement is claimed. UP-1 remains open for
other useful upstream changes.

Measured `a141614`'s cache implementation (with tests only, `d0981c1`)
against `3b4834f` on an Intel Core i9-14900KS, Pop!_OS 22.04, ext4,
Proton 11's Wine 11.0 in Steam's offline Flatpak runtime, Rust 1.98.1
Windows release builds. The PC has an RTX 5090, NVIDIA 580.173.02; this
benchmark does no GPU work. An external harness embeds both exact source
files with identical adapters and the real pipeline cache. It times the
whole `cached()` call using a synthetic clone callback, excluding setup,
result checks, destruction and the caller's build-fingerprint calculation.

Twelve paired rounds alternate implementation order and rotate payload
sizes, after two warm-up passes. Each sample has 128 lookups. Misses use
unique absent entries; the OS filesystem cache is not flushed. Every
returned field, every stored byte and the translation/hit/miss counts are
checked. All 144 samples are retained, including two slower patched 8 KiB
miss pairs. Miss times in microseconds per lookup:

| Synthetic payload | Before, median (range) | After, median (range) | Reduction in median |
|-------------------|------------------------|-----------------------|---------------------|
| 8 KiB | 276.3 (254.6–307.5) | 204.8 (177.6–419.5) | 25.9% |
| 64 KiB | 297.5 (274.6–478.0) | 220.9 (199.0–299.1) | 25.7% |
| 256 KiB | 541.5 (489.8–1394.2) | 330.2 (312.3–378.7) | 39.0% |

Pre-existing cache-hit controls never translated. Their median paired
changes were -1.0%, -1.7% and +1.1% respectively: no cache-hit improvement
is claimed. These measurements cover synthetic cache overhead on this
host under Wine, not actual translation time, native Windows filesystem
performance, gameplay loading time or FPS.

**U-6: report missing required DLLs before title initialization.**
The startup check previously examined only the executable's direct imports.
It now follows static imports through the mapped title DLLs, names each
missing dependency once, and refuses execution before binding or running
title code. Case aliases and cycles are visited once; host and HLE modules
remain supplied by their existing resolvers. Only paths of static imports
from the executable are required: a delay-only DLL remains optional.
Inspection without `--execute` and the explicit `--unimplemented zero`
override retain their behavior. Dynamic `LoadLibrary` handling and eager
mapping of available delay-import DLLs are unchanged.

An original generated executable imports a DLL that requires a missing
second DLL. Its initializer calls `ExitProcess(73)` if reached. On baseline
`f4c0707`, the public CLI reached that initializer; with the check it returns
the missing-DLL error before binding. The test uses no game files.

All ten public CLI startup tests passed in a Windows release build under
Proton 11's Wine 11.0 on Pop!_OS 22.04, Intel Core i9-14900KS, Rust 1.98.1.
They also cover a complete chain whose exports require DLL initialization,
deeper missing imports, case aliases, cycles, shared dependencies, both
delay-only cases, mixed required/optional paths, inspection, the explicit
zero override, and host/HLE imports. Run them with
`cargo test -p leonida --test startup --release --locked`.

**U-3: graphics profiles are selected by the executable.**
Direct `leonida run` and translation warm-up now resolve the same measured
base profile that the launch scripts used, before the runtime caches its
switches. The catalog is embedded at build time; an optional
`gpu-profiles.local.json` beside the selected config is applied afterwards.
The base profile still contains the same eight values, and `card-windows`
and the optional one-record clamp remain opt-in. This moves existing
optimizations into direct Windows/Wine launches. No new gameplay frame-rate
measurement is claimed.

`[graphics]` in `leonida.toml` provides typed settings for the eleven
options in the catalog. Explicit environment values override TOML, and
TOML overrides profiles. `profiles = []` or `["none"]` disables profile
selection while retaining explicit settings. Named selections include
base and apply in catalog order, with null cancelling earlier profile
values. `LEONIDA_PROFILE` still overrides selection; its existing value
`1` remains the diagnostics CPU sampler and does not select a graphics
profile. Malformed settings and unknown profile names report errors.

Adapter discovery uses the first DXGI adapter, matching the runtime's
`D3D12CreateDevice(None)`, rather than the scripts' largest-VRAM heuristic.
It respects diagnostics WARP selection and loads an explicitly requested
RenderDoc hook before querying DXGI. If discovery fails, only unqualified
profiles match. Profiles cannot set paths used for config/game/cache/data,
logging, adapter selection, or RenderDoc's startup controls.

`leonida config --graphics` reports the effective plan and each value's
source as JSON without game files. Launchers use this result for reporting
and let the child apply it. They preserve caller overrides and restore
their temporary environment changes afterwards. `-SpeedSwitches off`
disables both profile and TOML settings through `LEONIDA_GRAPHICS_OFF=1`
and clears runtime overrides, preserving adapter and capture-hook selection.
Shareable switch reports redact paths and other arbitrary string values.

Validation uses original synthetic inputs: 32 core tests and 13 default /
14 diagnostics CLI integration tests, run on native Windows and through
Wine. Each integration suite runs three generated PE programs through the
real loader, checking that profile, TOML and environment values reach the
title. The PowerShell checks also cover Unicode profile names, empty
overrides, state restoration, report redaction and baseline selection.
They inspect the complete environment block: Windows PowerShell 5.1's
single-variable lookup can mistake an empty value for an absent one.

On an Intel Core i9-14900KS / RTX 5090, NVIDIA 580.173.02, Pop!_OS 22.04
and Proton 11's Wine 11.0 in Steam's offline Flatpak runtime, the physical
adapter parity test passed and a direct CLI query selected exactly the
eight base settings. DXVK reports an AMD vendor ID for this NVIDIA card;
profiles intentionally use the adapter identity exposed to Direct3D.
No game files were used in these checks.

**R-7: untranslated shaders counted again; ordered-pixel shaders translate.**
`leonida translate --list` on this tree: 26,148 programs, 282 failures
(614 duplicates skipped). 195 are `GS_VoxelizeGeneric` geometry programs
("an address in an SGPR that is no root parameter"); they drop no draws,
since only vertex, pixel, compute and mesh programs are translated. 40 are
"a descriptor chosen by control flow", 29 containers have no DXIL part.
The 10 "a hardware SGPR input" were pixel shaders that wait for earlier
waves of overlapping pixels (POPS): they read the SGPR after the primitive
mask and `src_pops_exiting_wave_id`. Both now read as "no overlap"
(`shortcut:`: the host orders nothing), and all 10 translate and validate.
The 5 `CS_TraceShadowRays_*` ("a descriptor read only as data") load a
buffer view from the resource heap at a computed index and walk the memory
behind its address; that needs view addresses by heap slot from the host,
as `SLOT_STRIDES` gives strides (not done).

**P-5: less clock work in emulated wait hints.**
Each `monitorx` / `mwaitx` pair previously read and converted the performance
counter three times: once for each rate-line check and once for wait
accounting. The `mwaitx` rate line now reuses its wait-accounting sample,
leaving two reads per pair. The wait's sample stays at the same point;
only that instruction's rate-line timestamp moves earlier, before the
accounting and spin hint. Other instructions still sample after their work.
At a 10 MHz counter frequency, microseconds are exactly `ticks / 10`, so
that conversion avoids the wide multiply/divide. Other frequencies retain
the wide calculation. Emulation, counters and wait boundaries are unchanged.

Measured baseline `e39a313` against `abe9bf8` on an Intel Core i9-14900KS,
Pop!_OS 22.04, Proton 11's Wine 11.0 through Steam's Flatpak runtime,
Rust 1.98.1 release builds. The measured QPC frequency was 10,000,000 Hz.
The PC has an RTX 5090, driver 580.173.02; this benchmark does no GPU work.
Both revisions ran the same original synthetic wait-hint benchmark through
the production exception handler, with exact exception-count and unchanged
watched-memory assertions. The baseline received only the same benchmark
frequency print and longer warmup/timed loops, with no runtime changes.

Eight runs per version alternated order in one fresh, offline Wine prefix,
with only benchmark clients pinned to physical P-cores 0, 2, 4 and 6. Each
worker warmed up for 20,000 pairs and timed 500,000 pairs; one process per
version also warmed up before the measured runs. No measured samples were
removed. Medians and full ranges, in nanoseconds per exception:

| Workers | Before, median (range) | After, median (range) | Median time reduction |
|---------|------------------------|-----------------------|-----------------------|
| 1 | 1228.7 (1197.3-1392.3) | 1209.1 (1177.2-1432.2) | 1.6% |
| 2 | 841.7 (751.8-904.3) | 740.2 (681.5-857.0) | 12.1% |
| 4 | 594.9 (503.2-692.7) | 572.8 (516.9-603.0) | 3.7% |

Multiple workers measure aggregate throughput, not individual exception
latency. Two workers improved in seven of eight paired runs; one and four
workers each improved in five, with small median gains and substantial
variation. A preliminary series with 50,000 timed pairs per worker had
median reductions of 3.9%, 8.4% and 4.6%; the longer series above was run
because those short samples varied widely. These are synthetic Wine
measurements, not measured Windows gameplay or frame-rate gains. P-5 stays
open: unsupported instructions still take exceptions.

Regression tests compare conversion against wide arithmetic across nine
frequencies and long uptimes, preserve wait transitions at the 2 ms boundary
and on monitor changes, and check that both hints preserve the entire saved
CPU context except the instruction pointer. To repeat the longer benchmark
on an Intel CPU without native MONITORX, run it alone in release mode:

```powershell
cargo test -p leonida-loader --lib --release --locked benchmark_monitor_wait_exceptions -- --ignored --nocapture --test-threads=1
```

**P-3: exact copy-back write runs are scanned eight bytes at a time.**
After merging the GPU's changes with current title memory, `back_piece`
walked its XOR mask byte by byte to find the runs to write. It now packs
eight exact nonzero markers using portable integer operations and joins
only adjacent runs. Every unchanged gap stays out of the writes, including
where a title store lands after the read. Title-wins conflict resolution,
the read span, and failed-read handling are unchanged. Mask scratch is no
longer cleared before the merge overwrites every byte. No unsafe code,
shader, synchronization or profile changes were needed.

Measured on an Intel Core i9-14900KS, Pop!_OS 22.04, Rust 1.98.1 Windows
MSVC release builds, using Proton 11's Wine 11.0 in Steam's Flatpak runtime.
The machine has an RTX 5090, NVIDIA 580.173.02; this benchmark does no GPU
work. The identical ignored benchmark was added to baseline `1e8e6a0`,
retaining its original `back_piece`. It measures the whole helper,
including input restoration and a synthetic title-memory read, with reused
scratch. Each case doubles its warm-up batch until it takes at least 50 ms,
then times the same iteration count. Inputs are our own generated bytes.

Two batches of eight alternating before/after pairs were run on physical
P-core 0. The second keeps one Wine server running, warms both executables
before the measured pairs, and pins only the benchmark clients. Its 64 KiB
piece results, in microseconds (median and full range), were:

| Changes | Before, us (range) | After, us (range) |
|---------|--------------------|-------------------|
| None | 2.11 (1.99-2.99) | 1.98 (1.87-2.40) |
| First and last byte | 28.39 (24.84-32.24) | 12.44 (10.36-29.19) |
| One byte per 64, plus last | 35.42 (27.32-44.21) | 15.69 (13.79-19.17) |
| Every byte | 28.46 (23.85-32.53) | 20.86 (18.78-29.93) |
| Alternating bytes | 69.28 (58.73-84.52) | 60.44 (52.50-84.39) |
| Every byte, alternating CPU conflicts | 65.80 (58.46-81.30) | 59.06 (52.81-68.94) |

Across both batches, the sparse 64 KiB cases took 49-56% less time and the
fully changed case 18-27% less. Fragmented runs had smaller or noisier
gains; unchanged inputs showed no consistent change. The second batch's
smaller cases below are medians in nanoseconds, before -> after:

| Changes | 64 bytes | 4 KiB |
|---------|----------|-------|
| None | 7.0 -> 7.0 | 123.5 -> 106.7 |
| First and last byte | 48.2 -> 29.2 | 1852.2 -> 661.2 |
| One byte per 64, plus last | 46.8 -> 27.6 | 1792.5 -> 978.4 |
| Every byte | 42.0 -> 28.3 | 1491.9 -> 1185.4 |
| Alternating bytes | 85.7 -> 72.9 | 3984.4 -> 3615.0 |
| Every byte, alternating CPU conflicts | 84.2 -> 70.1 | 4740.4 -> 3611.5 |

These are cache-hot synthetic helper timings, not frame-rate measurements.
They exclude real title-memory read/write calls, GPU work, and streaming;
P-3 remains open. Tests compare exact runs for every eight-byte zero pattern
and every nonzero byte value, full merge outputs against a scalar model,
scratch reuse, failed reads, and a title store arriving after the read.
To reproduce the helper benchmark on Windows:

```powershell
cargo test -p leonida-gpu --lib --release --locked benchmark_copyback_pieces -- `
  --ignored --nocapture --test-threads=1
```

**AMD's shader compiler crashing on a pipeline worker is caught**
(`LEONIDA_COMPILER_GUARD=0`: not; after Belobug/VIctory-Belobug-AMD
81ebb7bdc). A tester's RX 7700 XT (Adrenalin 26.11.2) ended the run in the
loading screen with an access violation reading 0x8 inside `amdxc64.dll`,
called from `D3D12Core.dll` while a pipeline was made; Belobug saw the same
on an RX 9070 XT, not on the same pipelines in every run. A worker whose
compile reads one of the first 64 KiB is now parked for good, its pipeline
is answered with no host pipeline (its draws are dropped, as an
untranslated one's), the key goes into `compiler-crashed.txt` beside the
pipeline library so later runs skip it, and another worker starts.
Crashes outside a worker's compile, or at other addresses, end the run as
before. Risks: a crash inside a library load leaves that name's load lock
held, and any lock the driver held stays held; deleting
`compiler-crashed.txt` gives the listed pipelines another try. Not yet run
on an AMD card; on an RTX 3080 the guard never fires.

**C-2: indirect-budget tests allow either thread group to reserve first.**
The saturation test submits 128 records as two 64-thread groups. Each
group reserves its whole charge atomically and admits records in lane
order, but separate groups in one dispatch have no ordering guarantee.
The inherited test required surviving records 1 and 2; records 64 and 65
are equally valid when the other group wins. Commit `f85101b` introduced
that assertion alongside group-level charging, overconstraining the test.

The test now accepts exactly either pair, while still requiring the budget
to saturate without wrapping, exactly two admitted records, zero counts
for every rejected record, unchanged source bytes, and all preserved draw
fields and stride padding. It repeats the two-group dispatch 16 times.
A separate test submits the halves in each explicit order with one shared
budget, requiring the corresponding pair. Both tests use the unchanged
production shader and barriers; no runtime ordering or budget logic changes.

On an RTX 5090 (NVIDIA 580.173.02), i9-14900KS, Pop!_OS 22.04 and Proton
11's Wine 11.0 with vkd3d-proton, the old assertion failed when the controlled
reverse order produced records 64 and 65 at the exact budget cap. The fixed
hardware test passed all 16 unordered and both ordered cases. The unordered
RTX runs all selected records 1 and 2; the controlled reverse case is not a
claim of naturally observed reversed scheduling on that card. All inputs
are original synthetic data. This repairs regression coverage, not gameplay
performance. To run the explicit hardware test:

```powershell
cargo test -p leonida-gpu --lib --release --locked `
  host_budget_saturates_without_wraparound_and_preserves_draw_fields -- `
  --ignored --nocapture --test-threads=1
```

**P-5: repeated console-instruction warnings no longer take a shared lock.**
Every decoded console-only instruction used to lock a vector just to check
whether its kind had already been logged. A bit per kind now records that
first observation atomically; repeated instructions only load the bits.
Different operands of the same kind still share one warning, concurrent
first observers still produce one warning, and `ud2` stays unlogged.
Instruction emulation and wait accounting are unchanged.

Measured on an Intel Core i9-14900KS, Pop!_OS 22.04, Proton 11's Wine 11.0
through Steam's Flatpak runtime, Rust 1.98.1 release builds. The same
synthetic benchmark was added to baseline `0a1a17b` and the changed tree.
It executes real `monitorx` / `mwaitx` instructions through the production
exception handler, checks the exact exception count and unchanged watched
memory, and uses no game files. Each worker warms up for 2,000 pairs before
50,000 timed pairs. Eight runs per version alternated their order, with
both processes restricted to the same four physical P-cores.

| Workers | Before, median ns/exception (range) | After, median ns/exception (range) |
|---------|------------------------------------|-----------------------------------|
| 1 | 1243.8 (1224.5-1718.5) | 1251.7 (1219.0-1278.7) |
| 2 | 790.6 (749.2-1309.6) | 742.0 (728.5-891.0) |
| 4 | 667.8 (589.0-1014.8) | 538.0 (499.2-855.6) |

The multi-worker numbers describe aggregate throughput, not individual
exception latency. Median time per exception fell 6.1% with two workers
and 19.4% with four; one worker was essentially unchanged (+0.6%). These
are synthetic measurements under Wine, not a frame-rate claim. P-5 stays
open: the exceptions themselves remain, and Windows gameplay is unmeasured.
To reproduce on Windows, run the ignored test alone in release mode:

```powershell
cargo test -p leonida-loader --lib --release --locked benchmark_monitor_wait_exceptions -- --ignored --nocapture --test-threads=1
```

**P-2: an opt-in single-record indirect clamp.** With
`LEONIDA_INDIRECT_SINGLE=1`, a clamp of one record uses a cached one-thread
pipeline instead of launching 64 threads, filling shared arrays, and
synchronizing the group twice. The shared source keeps the same argument
limits and stores; the single thread charges the same saturating budget
and mesh diagnostics. Multi-record windows and gathered records keep the
grouped shader. The resource and UAV barriers are unchanged. If the new
pipeline cannot compile, the existing one remains available.

Measured on an RTX 5090 32 GB, NVIDIA 580.173.02, Intel Core i9-14900KS,
Pop!_OS 22.04, with Rust 1.98.1 Windows MSVC release binaries. The isolated,
offline Steam Flatpak runtime uses Proton 11's Wine 11.0, bundled DXVK and
vkd3d-proton, and Microsoft's `d3dcompiler_47.dll` from Windows SDK package
10.0.28000.2957. Wine's built-in compiler could not compile the baseline
shader. The physical Vulkan device and DXGI adapter name were verified as
the RTX 5090; these are hardware GPU timestamps, not software-renderer times.

The synthetic benchmark executes 3,200 of our own triangle draws per frame,
with the current grouped shader and its specialization in the same binary.
It keeps barriers and budget placement equal, resets the budget before the
timestamp pair, and verifies the exact GPU work charge after each frame.
Each process warms every case, then takes nine samples per case, reversing
case order on alternate rounds. Eight process runs produced 72 samples per
case. Times below are the median of the eight per-run medians, in ms; ranges
are the smallest and largest of those medians.

| Clamp / budget | Grouped shader, ms (range) | Specialization, ms (range) |
|----------------|----------------------------|----------------------------|
| One record / GPU memory | 26.532 (26.217-28.281) | 21.392 (20.178-23.175) |
| One record / CPU-mapped | 26.236 (26.227-28.445) | 26.553 (26.218-28.319) |
| Windows of 32 / GPU memory | 0.857 (0.847-0.874) | 0.854 (0.847-0.882) |
| Windows of 32 / CPU-mapped | 1.026 (0.991-1.048) | 1.007 (0.993-1.224) |

The one-record GPU-memory case took 19.4% less GPU time (5.14 ms less per
synthetic frame); CPU recording stayed about 4.9 ms. There was no clear gain
with a CPU-mapped budget. Both window controls execute the grouped shader.
Individual samples had outliers, as high as 208 ms in the
CPU-mapped case; no samples were discarded. This does not establish a
gameplay frame-rate gain or results on other cards, so the switch stays off
by default and no automatic graphics-card profile is changed. P-2 remains
open. See the README's optional-clamp testing steps for a local profile.

The hardware parity test executes both pipelines for 1,624 generated cases
covering draw, indexed draw, compute and mesh records, count buffers,
ahead-of-draw clamping, budget boundaries, zero-cost work and diagnostic
wraparound. It compares every output and budget byte, untouched source and
count buffers, inactive records, and padding sentinels. The same suite runs
on Windows WARP. The explicit hardware test and benchmark fail if a hardware
adapter or the specialized pipeline is unavailable; they cannot silently
skip or fall back. No game files are needed:

```powershell
cargo test -p leonida-gpu --lib --release --locked `
  host_single_records_match_grouped_arguments_and_budget -- --ignored --nocapture
cargo test -p leonida-gpu --lib --release --locked `
  bench_single_record_clamp -- --ignored --nocapture --test-threads=1
```

**The passes over copies in the card's memory hash a piece with 64
threads.** The compare after a submission and the rehash after ranges go
up walked each 64 KiB piece with one thread, 4,096 steps one after the
other. A group of 64 threads now walks it side by side, and one of them
hashes their 64 results (after avatarmanz/VIctory-7900XTX f669746). RTX
3080 12 GB (driver 621.24), Ryzen 7 7800X3D, first rooms, with
`LEONIDA_CARD_WINDOWS_MB=3072`, `LEONIDA_WINDOW_UPLOAD=1` and
`LEONIDA_TEXTURE_ROOM_PERCENT=70` in both runs: our send-and-hash and
compare lists 21 -> 3.6 ms of card time a frame, the usual frame 149-158
-> 129-135 ms. Not behind a switch: the hashes are made and compared by
the same shader, so only their values change, not which pieces differ.

## Newest (2026-11-25, later the same day)

**Update, 2026-11-25 afternoon: what a frame is made of, measured, and two
changes that hold.** First rooms, about 3,850 draws a frame: 104 ms a frame
(9.6 frames a second) with the first two changes below, 97 ms (10.3)
with all of them, from about 120 ms in the morning. Still far from the console's 33 ms.
- How the title paces itself (new meters in `fence.rs` and
  `console_insn.rs`): it never asks for an event. Each frame begins with
  one small submission and a signal, and a title thread then waits for
  that fence value by reading it in memory with `monitorx` / `mwaitx`
  (AMD-only: about 280,000 emulated exceptions a second on an Intel
  processor, all from this wait). The value is set when the layer has
  finished the whole frame before. So a frame is the time the layer needs
  for one frame's burst of submissions plus about 20 ms of the title's
  own; putting presents behind the line, or giving the title's threads
  more processors, changes nothing (both measured).
- Two walls stand side by side and both are full: the layer's submit
  thread (about 95 ms of a 120 ms frame in the morning) and the card
  (about 102 ms busy, by a timestamp pair around every host list:
  `cardmeter.rs`). Taking time off one alone moved the frame by 0 to 7 %.
- `LEONIDA_JOIN_PARTS=1` (off by default): the title's lists wait for
  memory values, which cut its ~15 submissions a frame into ~29 parts;
  parts that follow one another now go to the host as one submission
  (9 a frame), the same lists in the same order. About 6 %.
- `LEONIDA_BACK_OWN_VIEW=1` (off by default): what shaders wrote goes
  back into title view memory through a view of the layer's own, so the
  copy that wrote it is not read in again. About 9 % (submit thread 93 ->
  81 ms a frame, 104 -> 80 MB a frame checked). A CORRECTION: an earlier
  run seemed to show washed-out frames with this switch; the same frames
  turn up in about 6 of 21 runs with no switch at all (cause not known),
  and `LEONIDA_BACK_OWN_CHECK=1` counted 0 stores gone astray in 1.36
  million.
- Where the card's time goes now (`LEONIDA_CARD_METER_PIPELINES=1`, a
  table by pipeline): no pass is large. About 82 ms of the title's lists
  are spread over ~3,400 pipeline bindings a frame; ~2,400 of the
  indirect commands still get a clamp pass of the layer's with draw
  windows on (960 are compute dispatches; about 1,000 are mesh records
  the title keeps scattered through a 100 MB argument buffer, which a
  window of records at one step cannot serve).
- `LEONIDA_INDIRECT_GATHER=1` (off by default): a single indirect record
  that misses its window gets a slot in a scratch buffer of its list, and
  the list's prelude clamps all such records in one pass by a table of
  their offsets (the clamp shader with a gather bit), where each draw had
  a compute pass of the layer's in front of it. Proved by clamping every
  gathered record again at its draw and comparing on the card
  (`LEONIDA_INDIRECT_WINDOW_CHECK=1`): 7.9 million records, none differed.
  About 600 of the ~1,600 draws that missed a window are served; the
  others come after a dispatch or a storing draw in their list. With
  `LEONIDA_CARD_REHASH_RUNS=1`: 104 -> 97 ms a frame (10.3 frames a
  second at 3,800 draws).
- What the card's time is now, added up: of ~87 ms busy in a 97 ms
  frame, the layer's own lists are ~19 ms and its clamp passes 10-20 ms;
  about 50 ms are the title's own ~30 compute passes and its geometry at
  2560x1440 as translated. With every pass of the layer's gone, this
  scene would still be near 20 frames a second on this card: the
  translated shaders have to get cheaper too.
- The frame rate is drawn into the top left corner of every frame shown
  (`fpsbox.rs`; `LEONIDA_FPS_COUNTER=0`: not): the window's caption has
  it too, but a capture of the picture does not carry the caption.
- `LEONIDA_COPY_CHANGED=1` (off by default): a refresh of a copy in the
  card's memory sends on only the 256-byte chunks that differ from the
  copy's CPU side. The layer's sync lists on the card fall from 12 to 8
  ms a frame, the submit thread pays 3.6 ms for the compare and is then
  the limit: no net gain yet.
- HONEST LIMIT OF ALL THE ABOVE: those numbers are the scene standing
  still. While the camera moves the title streams assets, and about half
  the time goes to frames of ~490 ms in which the card is idle for ~330
  ms: some 850 remaps of title memory, 260 texture uploads and 450 disk
  reads in one frame. That is the next thing to measure and cut.
- The dips while moving, measured (a sampling profile of the opening
  cutscene, and a 370 s play session): the mean is 4.5 frames a second,
  with half the time in frames of ~490 ms in which the card is idle for
  ~330 ms. In those frames the title's render thread waits for its own
  jobs, not for the layer. One of those jobs is the title's page-mapping
  copies (`CopyPageMappingsBatchX` -> `xmem::map_alias`): the title maps
  every streamed texture page into its tiled heap, a range of one page
  each (over 4,096 different pool pages in a minute, about 850 pages in
  a frame that streams), and each is a view taken away and a view mapped
  in the kernel: 32 ms of every cutscene frame, 246 ms of a streaming
  frame, nearly all of it waiting on the process's address space.
- Two attempts on that, both taken back out: leaving alone pages that
  already show what is asked (counted: there are none, every remap is a
  real change), and mapping pool pages in a row as one view (1.8 to 4.2
  pages a view, remap time 140 -> 79 ms a frame, BUT faces came out black
  or red and one run drew garbage in 3 launches of 3, against none in 6
  launches without it). Reverted; why a longer view breaks texture
  streaming is not known yet.
- Measured and found to give nothing (switches left in, off):
  `LEONIDA_REFRESH_ONCE`, `LEONIDA_ASYNC_PRESENT`, `LEONIDA_VIEW_KNOWN`,
  `LEONIDA_SPREAD`, `LEONIDA_QUEUE2=3`, `LEONIDA_WINDOW_JUMP`. Each has an
  `=alt` form (twenty seconds on, twenty off, each change said in the
  log) so that one run measures both in the same scene.

**Update, the same night: the first room now renders right on the test
machine** (walls, textures, people, lamps), at 190 to 450 ms a frame (2 to 5
frames a second): the picture work has caught up, the frame rate has not.
What did it, on top of the list below:
- Descriptor records the title copies into its heap as plain bytes are
  mirrored before every submission (pull request #305 by Flyander, after
  upstream VIctory 86a5ed6). Measured here: about 40,000 records a frame
  that no call had announced; their host slots were empty or stale.
- A buffer view that does not start on an element of its buffer's host
  copy gets a copy of its own bytes instead of being refused (after
  upstream 86a5ed6): 3 to 6 such views a frame.
- Room on the card is kept for the render targets and buffer copies a
  world makes in its first frame: 98 buffer copies are made (945 MB) where
  the runs before made 1, and the processor no longer compares 550 to
  710 MB a frame looking for shader writes (25 MB now).
- Levels on demand ran on the card for the first time
  (`LEONIDA_LEVELS_ON_DEMAND=1`, still off by default; with the fixes of
  pull request #747 by AscilCH): 3.1 GB of the card not allocated for
  texture levels the title does not hold, no device loss.
- Guarded blocks under storage writes are opened by default; 14 more
  shader programs translate; a program is translated with first-half
  interpolation pairs only if it was refused for one (with that on for
  every program, 8,551 working pixel programs came out changed).
- Frame rate, first cuts on the processor side: the descriptor sync uses a
  cheap fingerprint and leaves draw-table slots to their draws (the pass
  over the heap went from 70-220 ms a frame to about 3), and memory the
  guards already hold is not asked of the write watch (pull request #670
  by Jonirulah). Card copies' pieces can come down inside their own
  flights (pull request #648 by Jonirulah, `LEONIDA_CARD_FETCH_INFLIGHT=1`,
  off by default: the copy-back thread's wait went from 181 ms a frame to
  0 here, but the card got busier, so no gain is claimed yet). The frame is
  still 250 to 380 ms: the card is the limit now, most of it the
  per-record clamp before each of some 6,700 indirect draws a frame.
- Pull request #220 (cheaper indirect draws) is IN this tree now, its
  windows off by default (`LEONIDA_INDIRECT_WINDOW=1`). History, so nobody
  repeats it: with windows on it lost the device three times here. Its own
  checker (`LEONIDA_INDIRECT_WINDOW_CHECK=1`) then showed one list drawing
  90 records of 2^24 vertices from a window, where the per-draw path lets
  eight through; a record of 2^23 vertices an instance or more now runs
  empty. Since that fix: eight runs with windows on, seven without a
  device loss and one that lost the device 44 s into the world (cause not
  found; see the driver watch below). One earlier loss after the fix had
  the in-flight fetch (#648) on as well; do not combine the two. The
  in-flight fetch itself measured slower here in two runs and stays off.
- Frame rate, second night (an RTX 4070; the first room went from about
  3.5-4 to 6-7 frames a second with ALL of the switches below; the
  defaults are unchanged and give about 4.5-5):
  - `LEONIDA_ASYNC_SUBMIT=1`: submissions are prepared and sent by a
    thread of the layer, not by the title's render thread, which a
    profile showed inside the submit path for 257 of 292 ms a frame.
  - `LEONIDA_QUEUE2=1`: fetches of what shaders wrote go to a copy queue
    (brought back, with the fix for a piece fetched beside a later
    flight); the copy-back thread's wait fell from about 125 to 7 ms a
    frame.
  - `LEONIDA_INDIRECT_WINDOW=1`: see above.
  - `LEONIDA_TEXTURE_NO_WAIT=1`: a texture refresh no longer waits while
    the title remaps memory; the loading phase had 5 readings over a
    second where it had 18.
  - `LEONIDA_MERGE_GAP=64 LEONIDA_MERGE_RUNS=4096`: title bytes put in
    under flights' shader writes are joined over small gaps (NOT exact:
    read the comment at `merge_gap` before using it).
  - On by default: fills of upload memory and read-only buffer copies no
    longer wait for flights and are checked against what the flights then
    wrote (`LEONIDA_FILL_WAIT=1` waits as before; no mismatch in any run
    so far); up to 512 idle host lists and texture staging buffers are
    reused instead of made and freed every frame.
  - `LEONIDA_PROFILE=1` (diagnostics builds): a sampling profiler inside
    the layer; it is how the above was found.
- Frame rate, third session: measuring first (same RTX 4070, first room).
  - A frame log (`LEONIDA_FRAME_LOG`, on in diagnostics builds): every
    frame's length, and for the usual frame and each long one what it was
    made of (the render thread's time in the present call, the submit
    thread's work, copies, copy-back, recording, texture work, faults,
    disk). A card meter (`LEONIDA_CARD_METER`) adds the card's time by
    kind of list from a timestamp at each list's start and end.
  - What they showed, windows off: a usual frame of 136 ms; the title's
    render thread waits 77 ms of it in the present call for the submit
    thread, which works 118 ms (copies of title memory 41, copy-back 40);
    the card is 85-98 % busy, of about 130 ms a frame 16 ms in our lists
    that bring card copies up to date, 10 ms in our compare lists and 102
    ms in the title's own lists with the per-record clamp. Frame time is
    linear in draws (about 32 microseconds a draw all-in): the title draws
    about twice as much on some frames, which are the long ones.
  - The regular dips of earlier test runs were the test harness: a saved
    frame (`LEONIDA_CAPTURE_WORLD`, every 8th world frame) is read back
    and written as a PNG inside the present, 235 ms each.
    `LEONIDA_CAPTURE_WORLD_EVERY=64` for runs that measure speed.
  - `LEONIDA_INDIRECT_WINDOW=1` measured again with those instruments. A
    CORRECTION: the first figures given here (6.0 against 8.9 frames a
    second) compared two runs that stood in different scenes during the
    measured window (about 4,300 against 1,550 draws a frame) and are
    withdrawn. Compared at equal draws, by a line fitted to each run's
    frame times against its draws per frame: windows off 145 ms at 4,500
    draws and 271 ms at 10,000; windows on with `LEONIDA_COPY_PLAIN=1`
    124 ms and 211 ms. On screen: 7-8 frames a second in the first rooms,
    10-12 in their lighter views.
  - A driver watch (on by default; `LEONIDA_DRIVER_WATCH=0` off): the
    layer subscribes to the system's event log and logs every entry of a
    display driver as it is recorded; the first one turns draw windows
    off for the rest of the run. Every device loss here had such entries
    2.7 to 55 seconds before the device was removed, and no run without
    them lost its device.
  - A copy between two title textures that share one host texture is no
    longer recorded (the host does not allow a texture copied onto
    itself): 402 of them in a five-minute run.
  - The list of shader-visible heaps is a reader-writer lock (recording
    threads waited 48 ms a frame, summed, behind the heap sync); the heap
    sync looks at a changed page's records under one lock, fingerprint
    first; guarded view memory is left out of a submission's write-watch
    askings ahead (after triippywhale).
  - `LEONIDA_COPY_PLAIN=1`: copies of title memory made in place by this
    process, with a handler that falls back to the kernel's copy on a
    missing page. Its first form also took the layer's one-byte "is this
    page there" reads, 1,000 to 1,300 faults a frame, and won nothing;
    reads under 256 bytes now stay with the kernel and a source that
    faulted is left alone for two seconds (4 to 9 faults a frame). The
    submit thread's kernel time fell from 44 to 20 ms a frame; the
    picture is the same.
  - The layer's submit and copy-back threads ask for the fast cores of a
    hybrid processor (`LEONIDA_FAST_CORES=0`: not): the submit thread was
    found on a slow core in 6 % of its looks, now in none of 21,059.
    `LEONIDA_DISK_CORES=slow` (off, not measured) asks the slow cores for
    the DirectStorage workers, which are as many as the logical
    processors and free to run anywhere.
  - Behind switches, built and unit-tested, not yet shown to pay:
    `LEONIDA_REFRESH_ONCE=1`, `LEONIDA_VIEW_KNOWN=1`,
    `LEONIDA_BACK_OWN_VIEW=1` (picture to be checked),
    `LEONIDA_ASYNC_PRESENT=1` (the present goes behind the queue's line;
    no gain while the card is the limit).
  - Tried and refused: `LEONIDA_BVH_HOLD=1` holds back the 64 programs
    that walk ray-tracing structures nothing has built; the picture loses
    its indirect light with it, so it stays off.
  - The process opts out of the system's efficiency mode
    (`LEONIDA_EFFICIENCY_MODE=1` leaves it); on a hybrid processor the
    frame log found the title's render and recording threads on fast
    cores in every look of a run, and the layer's submit thread on a slow
    core in 6 % of its looks.
  Still open: the per-record clamp is most of the card's time, and the
  copies between the PC's memory and the card's are most of the submit
  thread's; the cause of the one device loss with windows is not known.
  A first play session past the first rooms reached the open world and
  lost the device there with no driver entry (reason "device removed"):
  the card had no room for new textures from the opening cutscene on
  (9.2 of a 10.3 GB budget on a 12 GB card; 2,073 passes made no texture
  for lack of room; 44,613 textures let go and made again in 13 minutes).
  Cause of that removal not known either.
- Known and counted, not fixed yet: an indirect compute pass asks for
  about 77,500 thread groups in x where one dispatch runs 65,535 (the
  rest is cut off).

Memory sync between the title's memory and the copies on the card:
- **A write through one view of a physical page is seen by the copies taken
  through its other views** (`LEONIDA_VIEW_GUARD=0` turns it off). The title
  shows one page at several addresses and a copy was guarded at one of them
  only. Measured on the test machine: 0.8 to 1.0 million such writes in a
  five-minute run, every one of which used to leave a copy stale. Since it
  went in, the first room had its walls in the later frames of 4 runs out of
  4 (2 of 5 before); the room's very first shot still comes up without walls
  in some runs.
- Kernel reads into title memory are read beside and copied in, so a block
  guarded again in between no longer fails the read.
- Writes of the storage layer into physical pages are counted where the title
  has those pages mapped (10,000 to 50,000 guarded blocks of copies lay under
  them in every run) and can open the guards there:
  `LEONIDA_PAGE_WRITES=guards` (off by default; `1`, `slots`, `all` do more).
- Failed page-destination reads are retried (`LEONIDA_DSTORAGE_RETRIES=0` = as before).

Card memory (the test card is full from the first frame of the world: 85 to
99 % of its budget for a whole run, about 26,000 texture copies let go and
3,400 made again in five minutes, 2.8 GB of render targets that are all in
use):
- Levels on demand, **on by default** (`LEONIDA_LEVELS_ON_DEMAND=0` = as
  before): a texture the title fills takes card memory only for the levels
  the title holds (2.2 to 3.0 GB of the copies were levels it does not
  hold). Ran on an RTX 3080 12 GB (3.1 GB saved) and on an RTX 5060 Laptop
  8 GB with 16 GB of RAM (two runs into the first cutscene, no device loss).
- A skin-bake draw's own textures can be uploaded before it
  (`LEONIDA_BAKE_SOURCES=1`, **off by default**; fxceless/victory-fork pull
  request #1264 by UwUCammie, which had it on). RTX 3080 12 GB, driver
  621.24, Ryzen 7 7800X3D, one unattended run each on `main` with #33
  (`scripts/measure-run.ps1`): the most textures one bake draw was missing
  fell from 18 to 4, bake draws that uploaded their own textures first
  0 -> 3 of 8 logged, textures still waiting at the end 13,353 -> 9,302;
  usual frame 157 -> 150 ms (the run without it had a spike in preparing,
  so no speed is claimed). In the shot 600 frames into the world, hands
  that were missing were drawn, but one face had wrong colours it did not
  have without the switch. Cause not known. A guess: the forced pass also
  uploads a bake's texture that is only partly mapped (`draw.rs`, the
  `partly` path lets a bake's own texture through when its slot shows
  something else), so the bake reads bytes that are not there yet and
  keeps what it made. Off until that is found.
- Refused texture slots are held back for a few seconds so that a pass
  reaches the slots behind them (`LEONIDA_FULL_CARD_PASS=0` = as before);
  room can be kept for the card copies of title buffers
  (`LEONIDA_CARD_ROOM_FOR_BUFFERS=1`, off).

Tools and diagnostics:
- `leonida translate <shader file> --warm all --jobs N` translates every
  shader ahead of a run, on the processor only (about 4.5 to 6.5 minutes
  here); the shader cache is keyed on the translator's source, so a new
  build that does not touch the translator starts warm.
- Counters, all off or diagnostics-only: `LEONIDA_MERGE_RACE=1` (title bytes
  a copy-back is about to put back to older values: none found in 138,000
  copy-backs), `LEONIDA_WALLS_TRACE`, `LEONIDA_CARD_ACCOUNTING`, and a census
  of the copies the texture sweep never lets go.

## What this fork changed (summary)

Rendering:
- Meshlet culling read its scene buffers as the wrong kind of view, so the world drew almost nothing; fixed.
- Per-draw descriptor tables could run out, after which draws used slots never made for them and the card hung; fixed.
- Light packed by the game's compute shaders was read back through empty copies (sky, probes, fog); fixed for every mip level and layer.
- The fog reconstruction, shadow-mask, hair, blend-shape skinning, fire and several material programs did not translate; fixed.
- Gathers at a named mip level (with and without an offset) read level 0; fixed.
- Implicit-level samples outside pixel shaders produced invalid DXIL in 730 compute programs; fixed.
- Rectangle lists, depth and stencil twins for depth textures written by compute, texture read-backs.
- Ray tracing, in progress and off by default: the game's own structure-builder programs translate, title memory can be read by raw address from shaders, the hardware ray/box and ray/triangle test is emulated in DXIL (checked against a reference on the GPU), and 59 of the 64 trace programs translate. Nothing is lit by it yet. See [docs/fork-ray-tracing.md](docs/fork-ray-tracing.md).

Speed and memory:
- Waits for memory values, write-watch queries and page queries no longer walk page tables per call.
- Large title buffers get a copy in card memory, with shader writes found by hashing on the card.
- Submissions can be sent ahead of the wait for the card (`LEONIDA_LATE_WAIT`), with a relay thread that lands results as the card finishes (`LEONIDA_RELAY`): in a first five-minute run the card's idle time behind the submitting thread fell from about 136 to 30 ms per frame.
- Title memory can be opened as a graphics heap so that buffers are used in place instead of copied (`LEONIDA_INPLACE`, first stage only; a probe confirmed the approach on the test GPU).
- The bindless texture queue no longer uploads stale views; the view table no longer grows without bound; the pipeline library is mapped instead of read into memory.
- An intermittent device loss was traced to the pipeline library file being replaced while mapped; fixed.

- Translated shaders get clean-up passes over the finished module (dead code, constant folding, reuse of values, branch facts); the pack's host instruction count falls by about 43 %, worth about 20 ms of GPU time per frame on the test machine (`LEONIDA_SHADER_OPT=0` turns them off).

Most of the newer speed changes sit behind environment switches that are off
by default (`LEONIDA_LATE_WAIT`, `LEONIDA_RELAY`, `LEONIDA_WIDE_BACK`,
`LEONIDA_CARD_BUFFERS_MIN_MB`, `LEONIDA_INPLACE`, `LEONIDA_BVH_BUILDER`, ...).
Each is described in a comment where it is read.

## Upstream status (2026-11-23)

Upstream VIctory's own status when this fork was taken, with its switches renamed.

**Not playable yet.** The title boots, runs its menus at its own 30 fps and
loads the world, where rendering fails.

| Stage | State |
|-------|-------|
| Boot, first-boot screens (accessibility, language, settings, legal) | works, 30 fps |
| Intro video with sound | works |
| Start screen, profile and saves | works |
| Loading into the world (about 50 s) | works |
| The world (first mission's intro) | reached and runs with indirect draws; its title card renders, the scene after it is still dark; 0.5-1.8 s per frame |

Tested on one machine: Windows 11, AMD RX 6700 XT.

> **Warning.** Entering the world on a real GPU can currently hang it, and
> Windows then resets the display driver (every monitor goes black for a
> moment). To look further, use a diagnostics build with
> `LEONIDA_D3D12_WARP=1`, which renders on the software adapter.

What each part does today and what it still needs:

| Subsystem | Today | Needs |
|-----------|-------|-------|
| Loader | Runs the title's executable and its DLLs in our process; crash reports name the faulting code and are kept in `crashes.log` | – |
| Runtime services | One signed-in offline user, licences, packages, saves that survive a kill at any step, text entry dialog | Speech is silent; the dialog and relaunch are untested live |
| Network | Offline by design: the title cannot reach the network | – |
| Files and streaming | Archives and streamed data read, zlib and BCPack decoded in parallel | Loading is limited by writes into title memory |
| Input | XInput pads, keyboard, keyboard stand-in pad | Trigger rumble |
| Audio | Menu, video and loading sound; XMA voices and Opus streams decode | Reverb is silent; Opus results unverified against the title |
| GPU: menus | Draws, compute and presentation on host D3D12; only changed title memory is copied | Frame pacing drifts on one screen in release builds |
| GPU: world | Mesh pipelines, shader-written buffers, descriptor lifetimes, indirect execution (arguments clamped), pipelines made by worker threads and kept on disk | Most of the scene is still missing (untranslated shaders drop draws); frames take seconds; real queries, depth bounds, HDR and scaling are missing |
| Shaders | Console GPU code recompiled to DXIL at first use, cached on disk. Menus: all. World: mesh 8,191 of 8,506, vertex/pixel 10,617 of 12,346, compute 1,643 of 2,539 | 64-bit image atomics, merged geometry stages, the ray-tracing builder's shaders; some compute programs need a GPU with 64-lane waves |
| Textures | Console tiled layouts and detiling for the modes the title uses | Render-target metadata planes |
| Series S mode | Runs through the menus to the loading screen | The world, as above |

## Run scripts: switches by graphics card (2026-11-26)

`run-gta6.ps1`, `measure-gta6.ps1` and `shot-run.ps1` no longer set the
frame-rate switches themselves. `scripts/leonida-settings.ps1` reads the
card with the most memory from the display adapters' registry keys,
clears every `LEONIDA_*` switch an earlier run left in the PowerShell
window (the paths, `LEONIDA_PROFILE` and `LEONIDA_LOG` stay;
`LEONIDA_KEEP_WINDOW=1` keeps them all), and applies the profiles of
`gpu-profiles.json` that match the card. Each profile says where its
numbers come from.

- `base`, every card: the switches the scripts set before.
- `card-windows`, opt-in (`LEONIDA_PROFILE=card-windows`): windows of
  title memory in the card's memory. RTX 3080 12 GB, Ryzen 7 7800X3D,
  first rooms: usual frame 169-173 -> 149-158 ms, card time 103 -> 88 ms.
  But the windows take about 3.7 GB of the card, and host copies of
  textures picked by index fell from about 12,100 to 6,500.
- `LEONIDA_PROFILE=<name>[,<name>...]` applies `base` and those whatever
  the card; `LEONIDA_PROFILE=none` applies none. Local changes go in
  `gpu-profiles.local.json`, which git ignores.

On the same RTX 3080 textures stayed black while the card windows were on
and the AMD fork's `play.ps1` had left `LEONIDA_TEXTURE_NO_WAIT=1` and
`LEONIDA_RELAY=2` in the window; they came back once both were gone.
Which of the two did it was not separated. `LEONIDA_RELAY=2` is what the
code does when the switch is not set (the relay is on by default, at
stage 2), so of the two only `LEONIDA_TEXTURE_NO_WAIT=1` changed
anything. That is read from the code, not measured with card windows on.

Switches measured one at a time on top of `base` (RTX 3080 12 GB, Ryzen 7
7800X3D, driver 621.24, 2026-11-26): one unattended diagnostics run each,
Enter pressed every 8 s from 20 s on (the world from about 70 s), the
frame-rate lines from 170 s to 330 s, 4,650 to 4,950 draws a frame. One
run each, so read differences of a few ms as nothing.

| On top of `base` | Usual frame (ms) | Card | Preparing | Copying back |
|---|---|---|---|---|
| nothing | 126 | 48.8 | 58.4 | 34.8 |
| `LEONIDA_TEXTURE_NO_WAIT=1` | 125.5 | 52.0 | 58.7 | 35.4 |
| `LEONIDA_JOIN_COPIES=1` | 127 | 51.7 | 58.3 | 34.7 |
| `LEONIDA_LEVELS_ON_DEMAND=1` | 127 | 50.0 | 58.8 | 35.2 |
| `LEONIDA_JOIN_MOST=8` | 126 | 51.5 | 56.4 | 35.2 |
| `LEONIDA_RELAY=0` | 274 | 96.2 | 68.0 | 92.8 |
| `LEONIDA_WIDE_BACK=0` | 126 | 49.5 | 59.5 | 34.2 |

No run lost the device or wrote a crash report.

- The relay (on by default) pays: without it the frame takes twice as
  long. Wide copy-back (on by default) measured at nothing here, but
  costs nothing either, so it stays on.
- `TEXTURE_NO_WAIT`, `JOIN_COPIES` and `JOIN_MOST=8` (the AMD fork joins
  up to 8 parts) gave nothing on this card and stay off.
- Levels on demand made no frame faster but changed the picture: card
  memory in use 9,818 -> 9,206 MB, host copies of filled textures alive
  11,714 -> 17,417 (4,799 -> 3,689 MB), made again since the start
  2,948 -> 1,877 MB. Near-black pixels in the shot of frame 2600 fell
  from 15.9 % to 1.6 %, and a wall the tester had never seen drawn
  showed during that run. On by default since #33 (which also measured
  an RTX 5060 Laptop 8 GB).

More runs the same way, with levels on demand on (same PC, same day):

- `--console series-s`: a display driver entry, then the device was
  removed about 70 s in, at the world's first frames (7 submissions in
  the air). Series S stays out (TODO A-5).
- `card-windows`, with levels on demand and without `TEXTURE_NO_WAIT`:
  usual frame 124 ms (127 without), but host copies of filled textures
  alive fell from 17,417 to 9,247, 387 passes made none for lack of room,
  and near-black pixels in the frame 2600 shot rose from 1.6 % to 17.7 %.
  Textures still go missing with card windows, so `TEXTURE_NO_WAIT` was
  not the only cause; card windows stay opt-in.
- `LEONIDA_DISPLAY_MODE=1920x1080`: the title is told of a 1080p display
  but draws its scene into the same 2560x1440 targets as at 4K; usual
  frame 127 ms, card time the same. Its inner size does not follow the
  display mode, so a smaller output does not make the card's work less.
- `LEONIDA_BVH_HOLD=1`, only to name the dearest list: 16 walks held a
  frame, usual frame 124 ms, and the picture goes dark (near-black 1.6 %
  to 63 %), as found before. The card meter's dearest list, about 16 ms
  a frame of compute (64 pipelines, first bound when the world starts),
  stays at 15.4 ms (60 pipelines) with it: it is not the ray walk. What
  it is, is still open.

## Texture keep rule (2026-11-26)

`LEONIDA_TEXTURE_KEEP=1` (off; opt-in profile `texture-keep`): the
sweep of host copies of textures the title fills lets go only what the
card needs. Under 85 % of its budget only copies idle for ten minutes go;
from 85 % those idle for 5 s, the longest idle first, until 80 %; from
93 % those idle for 3 s until 84 % (`host::vram_keep`, after
Belobug/VIctory-Belobug-AMD 8d79927). Without it every copy idle for 1 s
goes from 85 % on, and a 12 GB card sits there all the time in the first
rooms.

RTX 3080 12 GB, Ryzen 7 7800X3D, driver 621.24, two pairs of unattended
runs (`scripts/measure-run.ps1`), on `e8aa6152`:

| Run | Usual frame (ms) | Median (ms) | Made again | No room |
|---|---|---|---|---|
| `main` | 156.5 | 160 | 14,986 | 96 |
| keep | 142 | 152 | 8,545 | 319 |
| `main` | 145 | 158 | 17,149 | 12 |
| keep | 130.5 | 132.5 | 5,674 | 390 |

"Made again": host copies of filled textures let go and made again since
the start; "no room": passes that made none for lack of room. Frames
were 14 to 15 ms shorter in both pairs, and copies made again fell by 43
and 67 %. But passes without room grew, and the first keep run's shot of
world frame 600 had 4.6 % near-black pixels against 2.0 %; in the second
pair the shots were alike (0.6 % against 0.0 %). No run lost the device
or wrote a crash report. Opt-in until it is measured on more cards and
the passes without room are understood.

## Water read-back on by default (2026-11-27)

`LEONIDA_TEXTURE_BACK` (`texback.rs`) copies the ocean's height and
displacement maps, which compute passes write, back into title memory,
where the title's boat physics reads them. It was off unless set; it is
now on unless set to `0` (a list of kinds still picks others). Without it
the boat at the start of the first fishing mission falls through the
water and the mission fails; Belobug/VIctory-Belobug-AMD has it on by
default for the same reason ("sank in every one" without it, its
`docs/findings.md`). Played on an RTX 3080 12 GB, Ryzen 7 7800X3D,
driver 621.24: with `LEONIDA_TEXTURE_BACK=1` the boat stayed up and the
mission went on. Cost: about a megabyte copied back per list that writes
one of the four kinds (`shortcut:` in `texback.rs`).

## P-4: page catalog lookups during remapping (2026-11-27)

DirectStorage used the title mapping mutex just to resolve page numbers to section
slices. An unrelated alias remap held that mutex across host mapping calls, delaying
workers whose own views never touch the title addresses being changed. Page numbers
now have a separate catalog lock. Mapping snapshots retain section ownership across
a concurrent page free; native mapping calls run after releasing the catalog lock.

Title view extents and reverse aliases stay under their existing mutex. DirectStorage
written-page publication takes that mutex before the catalog and releases both before
its GPU notifications. Map/remap notifications still hold their original protection.
The texture-read/remapping guard, native mapping calls, mapping granularity and
valid-prefix behavior on an unknown page are unchanged. This complements PR #34's
texture range locks; it does not implement those locks or complete P-4.

The synthetic `page_windows_do_not_wait_for_unrelated_title_remapping` regression
times out after five seconds on the prior implementation and returns the expected
bytes with the split. All 15 xmem tests passed in an isolated, headless Proton 11.0
prefix with GPU devices disabled, including pending-section lifetime after page free,
unknown-page prefix mapping and 2,000 remaps with a protected concurrent reader.
These are CPU memory tests; no gameplay frame-rate improvement is claimed.

## Debugging

A release build has no debugging aids. Build them in with

```powershell
cargo build --profile diag -p leonida --features diagnostics   # target\diag\leonida.exe
```

| Variable | Effect |
|----------|--------|
| `LEONIDA_LOG=<target>=debug,...` | debug and trace output |
| `LEONIDA_STACKS=20,60` | every thread's stack at those seconds |
| `LEONIDA_WATCH=<rva>,...` | logs a title u32 when it changes |
| `LEONIDA_KEYS=30:Enter,35:X` | presses keys at those seconds, focus or not |
| `LEONIDA_CRASH_SAVE=<rva>:<len>,...` | saves title memory to the cache folder on a crash |
| `LEONIDA_CAPTURE=<dir>` | saves presented frames (powers of two, or `LEONIDA_CAPTURE_FRAMES=<list>`) |
| `LEONIDA_DUMP_TARGETS=1` | with the two above, saves every render target of those frames |
| `LEONIDA_D3D12_DEBUG=1` | host debug layer |
| `LEONIDA_D3D12_GBV=1` | with the above, GPU-based validation (bad descriptor use reported, not faulted; slow) |
| `LEONIDA_D3D12_DRED=1` | on a GPU failure, the allocation hit and the commands in flight |
| `LEONIDA_PIPELINE_WAIT_MS=24` | reduce the per-frame wait for background pipeline creation from the default of 500 ms. A draw can be omitted until its pipeline is ready |
| `LEONIDA_SKIP_PIPELINES=<hash>,...` | omit draws and dispatches, including indirect ones, whose pipeline contains a listed 16-digit shader hash |
| `LEONIDA_GPU_TIMING=1` | with the above, each list of a submission run and timed on its own; slow lists logged with their slowest commands |
| `LEONIDA_D3D12_WARP=1` | host device on the software adapter (slow; cannot reset the driver) |
| `LEONIDA_SHADER_LOOP_LIMIT=<n>` | translated shader loops stop after `n` iterations per lane instead of 2^20 (own shader cache entries); if a lower `n` makes world frames much faster, loops hitting the limit are what makes them slow |
| `LEONIDA_AUDIO_CAPTURE=<file.wav>` | records what the title plays (our process only) |
| `LEONIDA_XMA_DUMP=<dir>` | saves the first 32 XMA voices' context and input |

Every build appends crashes and host GPU failures to `crashes.log` in the
title's data folder.

## R-7: large code-literal tables on NVIDIA (2026-11-26)

Six compute pipelines were refused by NVIDIA's Vulkan compiler under Proton even though
Microsoft's DXIL validator and `spirv-val --target-env vulkan1.3` accepted them. Their
runtime-indexed constant arrays were 16,640–18,368 bytes. A joined code address can have a
zero fixed offset, so its table must retain instruction bytes as well as appended data;
trimming to the final decoded instruction is not generally equivalent.

`LEONIDA_CODE_TABLE_CHUNKS=1` splits tables larger than 16 KiB into constant arrays of at
most 1,024 dwords. Reads preserve the original wrapping subtraction, dword indexing and
last-word clamp. Every array load is in bounds, including unselected loads from the short
final chunk. Tables of at most 16 KiB keep their original single-array form. The switch is
off by default because selecting a chunk can require more loads on other cards. The
translator's source and switch fingerprint keep its cached translations separate.

On an RTX 5090, driver 580.173.02, i9-14900KS, Pop!_OS with Proton 11.0 and vkd3d-proton
3.2.0, the six refused pipelines went from 0/6 to 6/6 created in a compile-only comparison.
All six still passed DXIL and SPIR-V validation, and their arrays retained every original
byte. Synthetic tests execute the emitted address/load/select instructions against an
independent original-array oracle, including chunk edges, unaligned addresses, underflow,
32-bit wrap and both switch modes. Pipeline creation alone does not establish correct
rendering or improved frame times; R-7 remains open.

## Conditional branches back to two loop headers (2026-11-27)

A conditional latch can return to either of two nested loop headers. The emitter used
to guard only its first destination, leaving the other counter without a back-edge phi
input. Reaching the first loop's limit also redirected execution into the other loop.
Each selected back edge now has its own guard and counter update; at the limit it uses
the existing abort path (discard for a pixel, return otherwise). Register and lane-mask
phis use the guard as their incoming predecessor. Equal destinations share one guard.

This repairs the existing bounded-loop approximation, without adding a switch or changing
its limits, wave size, barriers, or the forward exit of an ordinary single-back-edge loop.
Synthetic native programs verify both branch choices, nested-loop register results,
self-loops, aliases, phi predecessors and pixel discard by executing emitted integer
control flow on the CPU. The regression fails before the correction. This establishes
the guard defect and its repair; it does not establish the cause of a live driver timeout
or claim a frame-rate improvement.

## Cards measured: RTX 3060 Ti 8 GB (2026-11-27)

Frame figures are `ms/frame` from the game's title bar, sampled on world frames only (the menus sit at 33.3 ms),
after the shader warm-up and not on the first run after a shader change. Nothing here quotes game data.

| Card | Driver | OS | Profile applied | Where | Frame | Notes |
|---|---|---|---|---|---|---|
| RTX 3060 Ti 8 GB | 621.40 (32.0.16.2140) | Windows 11 26H2, build 26300 | `base`, selected automatically | opening prologue, Series X | 514–1,064 ms, 510–756 draws (about 2 fps) | Ryzen 5 5600, 16 GB RAM, 48 GB commit. Before the shader warm-up the first gameplay room was not drawn at all (R-2); after it the prologue rendered but washed out (R-3). |
| RTX 3060 Ti 8 GB | 621.40 | Windows 11 26H2 | `base` | account picker, language, brightness, legal notices, title art | 33.3 ms (30 fps) | All of these draw, take input and hold 30 fps. |

Notes for this card:

- `leonida config --graphics` resolves `base` here with nothing set by hand: `ASYNC_SUBMIT`, `QUEUE2`,
  `INDIRECT_WINDOW`, `COPY_PLAIN`, `JOIN_PARTS`, `BACK_OWN_VIEW`, `INDIRECT_GATHER`, `CARD_REHASH_RUNS`.
- The shader warm-up is not optional on this card. Without it, the handover from the cutscene to the first room
  produced a black frame at 1 fps, 845–899 ms and 1,029–1,252 draws issued, which reads as R-2. With it, the
  prologue drew at 514–1,064 ms but washed out, which reads as R-3.
- `leonida translate --list` reports 198 shaders that do not translate on this dump: 195 `GS_VoxelizeGeneric`
  geometry programs and 3 lane-mask cases, matching the counts above.
- Working set while the title screens are up: about 5.3 GB, on 16 GB of RAM with a 48 GB commit limit.
- Build: release profile, commit f0e7a9b.

### Where the 8 GB goes (diagnostics build, Series X, after the warm-up)

`measure-gta6.ps1` in the first gameplay room, `target\diag` with the frame log and card meter:

| Figure | Value |
|---|---|
| video memory budget / in use | 7,076 MB / 6,963 MB (98.4 %) |
| title textures with no host copy | 15,563 (8,646 MB of title memory) |
| "no room for new ones" lines | 70 (the line is throttled; refusals are more frequent) |
| private memory | 22,008 MB, 1,429 MB of RAM free, 9,710 MB of commit left |
| usual frame (mean of 17) | 312 ms |

Two attempts to buy room, both worse for the picture:

- `LEONIDA_TEXTURE_ROOM_PERCENT=97`: textures with no host copy rise to 17,301 (10,314 MB), the "no room" lines
  to 191, and the usual frame falls 312 -> 271 ms. Faster and blacker, the trade the notes above describe.
- `--console series-s`: it loads on this card (unlike A-5's note) but the first room renders black at 1 fps,
  1,446 ms. Not usable.

So on 8 GB the missing-texture count is structural: the title asks for 8.6-10.3 GB of texture data with a 7.1 GB
card, and no exposed switch changes that. `LEVELS_ON_DEMAND` (on by default) already does the cheap part.

## Cards measured: RTX 4070 Ti SUPER 16 GB, RTX 4060 Ti 8 GB (2026-11-27)

Two testers' reports, as they gave them. Nothing here quotes game data.

RTX 4070 Ti SUPER 16 GB, Ryzen 9 7900X, driver 621.40, Windows, tester ondra. Build e2f881f,
Series X, first rooms, six unattended runs of 330 s, frames counted from 170 s; median ms/frame,
`base` against `base` and the switch; 2,032 to 2,698 draws a frame; no device lost and no crash
report in the six runs.

| On top of `base` | Pair 1 (ms) | Pair 2 (ms) |
|---|---|---|
| `LEONIDA_TEXTURE_KEEP=1` | 223.0 -> 227.0 | 260.8 -> 268.0 |
| `LEONIDA_COPY_CHANGED=1` | 223.0 -> 353.2 | 260.8 -> 448.6 |

- Texture keep gave nothing on this card: 4 and 7 ms longer, while the baseline itself moved 38
  ms between the pairs. A 16 GB card is not near its budget in the first rooms, which is where
  the rule lets go less (a guess); it shortened frames only on the RTX 3080 12 GB so far. It
  stays opt-in.
- Copy changed made frames 60 to 70 % longer. It stays off.
- The baseline here (223 to 261 ms) is slower than the RTX 3080's (about 140 ms) with fewer draws;
  the report has no card or preparing times to say why.

RTX 4060 Ti 8 GB, Core i5-14400F, driver 621.40, tester amineourabi0. First gameplay area after
the opening cutscenes, one run with the run script's speed switches and one without:

| Speed switches | Last minute | Whole run |
|---|---|---|
| on | 2.7 fps | 6.6 fps |
| off | 4.6 fps | 8.3 fps |

- On this card the switches made the game slower, about 370 against 220 ms a frame in the last
  minute. One run each, so this needs a second pair, and the switches one at a time, before
  `base` changes for 8 GB cards. If it holds, the switch that costs goes into a profile that
  leaves it out for such cards, not out of `base` for every card.
