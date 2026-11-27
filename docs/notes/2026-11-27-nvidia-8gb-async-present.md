# P-1: texture staging and asynchronous present on 8 GB

`[graphics].async_present` maps to `LEONIDA_ASYNC_PRESENT`, which `queue.rs` already reads. It is off by
default; the opt-in profile `nvidia-8gb-async-present` (NVIDIA, 7,000-8,600 MB VRAM) turns it on.

Measured on an RTX 3060 Ti 8 GB, Ryzen 5 5600, 16 GB, driver 621.40, Windows 11 26H2, Series X, first
gameplay room after the shader warm-up.

`LEONIDA_TEXTURE_STAGE_MB`:

| Staging a frame | Textures with no host copy | "No room" lines | Usual frame |
|---|---|---|---|
| 192 (default) | 15,563 (8,646 MB) | 70 | 312 ms |
| **512** | **12,145 (8,500 MB)** | 190 | **288 ms** |
| 1,024 | 15,064 (10,381 MB) | 194 | 1,007 ms |

Staging more a frame drains the title's backlog before the card fills, so 3,400 fewer textures stay without a host
copy; 1,024 overshoots and thrashes. Opt-in profile `nvidia-8gb-texture-stage` (NVIDIA, 7,000-8,600 MB) sets 512.
In the room frame used for these counts the picture was a lit prison yard through a window at 212.8 ms and 5,096
draws.

`LEONIDA_ASYNC_PRESENT=1`, two pairs of runs in the same room (512 MB staging), six readings a run, one minute after
the world appeared:

| Pair | Usual frame without | Usual frame with |
|---|---|---|
| 1 | 175 ms | 127 ms |
| 2 | 183 ms | 139 ms |

The frame is about a quarter shorter. Of four switches tried on the copy path it is the only one whose three figures
(frame, card idle, copy-back wait) all moved the same way; in the single runs before the pairs the copy-back thread's
wait with nothing to land fell from 82.5 ms a frame to 54.5 ms. `LEONIDA_RELAY=0` reproduced the upstream finding
(645 ms against 489 ms), so the relay stays on.

Not measured: other 8 GB cards, so the profile stays opt-in.
