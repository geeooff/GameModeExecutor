# Icons

Three states by two taskbar themes, from one controller silhouette. The states
are told apart by **brightness, not shape**, so the outline stays recognisable
at 16 pixels and the difference survives greyscale and colour blindness.

| State | Means | Treatment |
| --- | --- | --- |
| `active` | a game is detected | saturated green |
| `idle` | the watcher is running, no game | muted grey, **no slash** |
| `error` | nothing is wrong yet — see below | red, with a slash |

`-dark` is for a **dark taskbar**, `-light` for a light one. The suffix names
the theme it is for, not the colour of the artwork: `-dark` is the *brighter*
drawing, because it sits on a dark background.

## Why idle carries no slash

A diagonal slash means *disabled* in Windows iconography. Idle is the state the
program spends nearly all its time in, and it means the opposite: running,
watching, nothing to do. An earlier draft used a slash there and it read as
"GameModeExecutor is switched off". The slash moved to `error`, where it says
something true.

Measured luminance (Rec. 709, alpha ≥ 200, on the 256 px renders), verified
independently of the design notes:

| | active | idle | error |
| --- | --- | --- | --- |
| `-dark` | 166 | 98 | 132 |
| `-light` | 151 | 62 | 105 |

The active-to-idle gap is 41 % on dark and 59 % on light, which is what keeps
the two apart without colour.

Worth knowing before anyone tunes this: `idle-dark` at 166 against a taskbar
around 32 is comfortable, but at 16 pixels on a 100 % display it is faint. It
has only been judged at 24 pixels on a 150 % display so far.

## What ships here

Only the `.ico` files and their `.svg` sources. Each `.ico` carries eight PNG
frames at 32-bit alpha — 16, 20, 24, 32, 40, 48, 64 and 256 pixels — covering
Windows scale factors from 100 % to 300 %.

**Pass the `.ico` and let Windows pick the frame.** Never resize one in code:
the frames were rendered at 4× and downsampled, not scaled from a single size,
and scaling them again throws that away.

The standalone PNGs are deliberately not here. They carry about 5.7 KB of C2PA
provenance metadata each — 92 % of a 16 px file — which is harmless on a web
page and pure waste compiled into an executable. The `.ico` frames are clean:
702 bytes at 16 px.

## Where they are used

- **`gamemode-active-light.ico` is the executable icon**, compiled into every
  binary by `build.rs` through the Windows SDK's `rc.exe`. An executable icon
  cannot follow the theme, and the darker green holds its definition on the
  white Explorer background Windows ships with.
- The notification area icon picks its file at runtime from the state and the
  taskbar theme.

## Reloading at runtime

Two messages, and missing either leaves the icon wrong rather than absent:

- **`WM_SETTINGCHANGE`** with `lParam` equal to `ImmersiveColorSet` — re-read
  `SystemUsesLightTheme` under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`
  (`0` means a dark taskbar) and swap the suffix.
- **`WM_DPICHANGED`** — reload from the `.ico` at the new `SM_CXSMICON`, or the
  icon stays blurry at the old size.

## Re-tinting

Each SVG carries five colour values, six for `error`: the two gradient stops of
the body, the outline and bumpers, the stick rings, the ink of the controls,
and for `error` the slash. After editing, re-render each PNG at 4× the target
size, downsample, and repack the eight frames into the `.ico`.

Made with Claude Design. The full export, including the rejected treatments,
stays in `.claude/design/icons/`, which is not tracked.
