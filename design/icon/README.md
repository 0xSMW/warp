# Warp CRT icon asset library

Organized 2026-09-13. Keep all three approved variants independent and unchanged when creating
another option. Duplicate a master into a new variant folder before experimenting.

| Picker name | Editable SVG | Transparent export | macOS icon |
| --- | --- | --- | --- |
| CRT Blue | [Master](blue/warp-terminal.svg) | [1024 PNG](blue/warp-terminal-1024.png) | [ICNS](blue/warp-terminal-blue.icns) |
| CRT Green | [Master](green/warp-terminal-green.svg) | [1024 PNG](green/warp-terminal-green-1024.png) | [ICNS](green/warp-terminal-green.icns) |
| CRT Amber | [Master](amber/warp-terminal-amber.svg) | [1024 PNG](amber/warp-terminal-amber-1024.png) | [ICNS](amber/warp-terminal-amber.icns) |

Each folder also contains its review previews. Blue includes 64/128-pixel exports, Dock
previews, and the geometry overlay. Preview backgrounds and overlays are for comparison only;
use the transparent exports for app icons. All moved assets were checksum-verified unchanged.

## Editable artwork and geometry

`blue/warp-terminal.svg` is the blue vector master. Each variant's named groups separate the metal frame,
bezel, screen, texture, reflection, and original Warp glyph. The glyph paths come from
`app/assets/bundled/svg/warp-logo-light.svg`; no generated image is embedded.

The master and size exports have transparent backgrounds. Files with `preview-dark`,
`dock-preview-dark`, or `dock-preview-light` in their names use backgrounds for comparison only.
The three designs are independent choices in Settings → Appearance → Icon.

| Measurement (1024 px canvas) | Raster trial | Vector master |
| --- | --- | --- |
| Outer background | Opaque charcoal | Transparent |
| Frame bounds | Approximately 160–1090 on a 1254 px image (74%) | 96–928 (81.25%) |
| Screen center | Approximate | 512, 512 |
| Glyph visible-path center | Approximate | 512, 512 |
| Center deviation | Unmeasured | 0 px horizontal and vertical |

Glyph bounds before transformation: x=35–216.155, y=25.5701–170.489.
The canvas-fit group scales the complete design by 13/14 around the canvas center.
This matches Activity Monitor's ICNS artwork bounds: 104×104 at (12,12) on its
128×128 canvas, measured at 50% alpha. The initial vector trial was 112×112 at (8,8).
The transform subtracts their midpoint, scales uniformly by 2.85, then translates to
the screen center. Shadow is excluded from the glyph-path centering measurement.
`blue/geometry-overlay.svg` shows the frame, screen, glyph bounds, and common centerlines.

## Approved finish and colors

- Silver width was increased by 15%: 28 → 32.2 source units before canvas scaling. The inner
  metal rectangle is x/y=96.2, width/height=831.6, corner radius=165.8.
- Preserve the approved top inset. The lower bezel darkens to `#080808`; inset shadow ends
  at black with 0.8 opacity. Do not lighten the bottom inset or enlarge the outer footprint.
- Blue retains its rich blue screen and pale glyph exactly as approved.
- Green uses near-black glass (`#071912`, `#06150f`, `#04100b`), a pale phosphor glyph
  (`#c2ffe0` → `#70eeb0`), subtle horizontal scanlines, and a restrained green halo.
- Amber uses warm near-black glass (`#1b1407`, `#171005`, `#110c03`) and an orange-amber
  glyph (`#ffb126` → `#ffa016`). The narrow glyph gradient is intentional. Avoid a pale/white
  top or bright yellow screen. Preserve the scanlines and restrained halo.

The SVGs are authoritative for remaining gradients, filters, and geometry. These are editable
vector groups, not embedded AI-generated raster artwork.

## Re-exporting

Run from the repository root with librsvg's `rsvg-convert`. Render a candidate first; compare
at actual Dock size before replacing an approved export. Renderer versions can affect pixels.

```sh
rsvg-convert design/icon/blue/warp-terminal.svg -o /tmp/warp-crt-blue-candidate.png
rsvg-convert -w 128 -h 128 design/icon/blue/warp-terminal.svg -o /tmp/warp-crt-blue-128.png
```

Substitute the green or amber master for those variants. To generate a candidate ICNS on macOS:

```sh
iconset_dir=$(mktemp -d /tmp/warp-crt-amber.XXXXXX)/WarpCRT.iconset
mkdir "$iconset_dir"
for size in 16 32 128 256 512; do
  rsvg-convert -w "$size" -h "$size" design/icon/amber/warp-terminal-amber.svg \
    -o "$iconset_dir/icon_${size}x${size}.png"
  rsvg-convert -w "$((size * 2))" -h "$((size * 2))" design/icon/amber/warp-terminal-amber.svg \
    -o "$iconset_dir/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$iconset_dir" -o /tmp/warp-crt-amber-candidate.icns
```

Stored ICNS files remain preserved approved artifacts; they were not regenerated during
organization. Check candidate transparency and footprint against the stored previews on both
dark and light backgrounds.

## App integration

| Choice | Rust enum variant | Runtime PNG in `app/DockTilePlugin/Resources/` |
| --- | --- | --- |
| CRT Blue | `CrtBlue` | `crt_blue.png` |
| CRT Green | `CrtGreen` | `crt_green.png` |
| CRT Amber | `CrtAmber` | `crt_amber.png` |

- `app/src/settings/app_icon.rs`: enum, display labels, resource names, persisted preference.
- `app/src/settings_view/appearance_page.rs`: picker labels; options enumerate the enum.
- `app/src/appearance.rs`: applies changes and the startup selection. Local builds load
  alternate PNGs from `Contents/Resources/app-icons/`; Default uses the bundle's `AppIcon.icns`.
- `app/DockTilePlugin/WarpDockTilePlugin.m`: maps the new choices for plugin-based builds.
- `script/prepare_bundled_resources`: copies runtime PNGs into macOS bundles.

The approved 1024 PNGs match the runtime copies byte for byte at handoff. After an approved
artwork edit, update its runtime PNG too; editing an SVG alone does not change the running app.
Do not overwrite Default's `AppIcon.icns` with a CRT: these now have separate selectable
identities. Existing icon choices and saved preferences are preserved.

## Release and acceptance handoff

The picker release compiled successfully on 2026-09-13, with existing warnings remaining.
All icon resources were packaged, the signature verified, and the exact release bundle launched:

`target/release/bundle/osx/WarpLocal.app`

Startup then waited on macOS's “WarpLocal would like to access data from other apps” prompt.
Live picker switching and persistence across restart still need user acceptance. The code uses
the existing preference and startup/change handlers; live UI acceptance has not been claimed.
The official `/Applications/Warp.app` was not modified.

The successful compile command was:

```sh
CARGO_PROFILE_RELEASE_DEBUG=0 cargo build --release -p warp --bin warp --features gui
```

This compiles only; it does not refresh the `.app` bundle. Package the new binary and resources,
verify the bundle, then launch that exact release path. Never open an older build during
compilation or use a debug build for visual review. This folder organization requires no
rebuild: runtime resources remain at their existing paths.
