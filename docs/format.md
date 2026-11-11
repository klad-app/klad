_Last updated: 2026-11-11 13:30 (format `klad/7`, stroke files `KLST` version 3, brushes `klad-brush/1`)_

# The klad drawing format

A klad drawing is a folder whose name ends in `.klad`, called a bundle. This document describes everything in it, so that any program can read, write and make klad drawings. It is licensed under the MIT License (`LICENSE-MIT`), unlike klad itself.

From klad 1.0 on, a drawing made by any release opens in every later one with nothing lost: a format change adds a migration that converts, never drops.

klad is the reference: when this text and klad's renders disagree, klad is right and this text is wrong. The data is exact (every key, every byte); how pixels come out of it is described as far as a reader needs to draw a faithful approximation, not to match klad's renders bit for bit.

## The folder

| Path | What | Required |
|---|---|---|
| `doc.json` | the document: canvas, timeline, palette, layers, drawings, vector shapes | yes |
| `strokes/<layer>/<cel>.strokes` | the brush strokes and fills of one paint drawing | for paint drawings with marks |
| `brushes/<id>.json` | every brush a stroke uses, embedded the first time it is used | for every brush id in a stroke file |
| `brushes/images/<name>.png` | pictures that brushes use as tips or grain | when a brush names one klad does not have built in |
| `images/<file>` | pictures placed by `image` nodes (PNG) | for every `src` |
| `fonts/<file>` | fonts used by `text` nodes, with their licence files | for every `font` |
| `weights/<owner>.weights` | how much each bone of a skeleton moves each part of the drawing | for every bones layer with `bones` |
| `.gitignore` | `.cache/`, so a drawing can live in a git repository | no |
| `.cache/` | rebuildable pixels, the undo history, the editor's selection and pinned notes, and `copy.json`: this copy's tag and id counters | no: never read it as data |

Everything outside `.cache/` is the drawing. Copy, version or zip those files and the drawing renders the same on another machine. Read the selection and the notes through `klad selection --json` and `klad notes --json`, not from `.cache/`.

Writers replace files atomically (write a temporary file, then rename), so a reader that watches the folder never sees half a file. Nothing in the drawing changes unless the drawing does: `doc.json` holds no revision or id counter, so an edit to one stroke changes its stroke file and nothing else, and git diffs and merges only what was edited. A program that wants to know whether a drawing changed compares the files (their size and modified time, or their bytes).

## Conventions

- **Coordinates** are canvas pixels, origin at the top left, y down. Angles are degrees; positive turns clockwise on screen.
- **Colours** are `"#rrggbb"` or `"#rrggbbaa"`: sRGB, straight (not premultiplied) alpha.
- **Ids** are unique across the whole document and use letters, digits, `-` and `_`. Ids are case sensitive. klad makes new ones from a prefix (`l` layer, `c` cel, `n` node), a number, and the tag of the copy that made them: `l48qa`. Each copy of a drawing (a clone, a git worktree: any folder without `.cache/copy.json`) picks a two-letter tag the first time klad opens it and keeps it in `.cache/copy.json`, with counters that only go up, so two copies edited apart never make the same id; the copy a drawing was made in has no tag (`l48`). A folder copied by hand with its `.cache/` keeps the tag: delete `.cache/` in the copy to give it its own. Ids already in a drawing never change. Writers that make their own ids should do the same or use a long random part. A drawing with an id used twice does not open; `klad repair` gives every later one a new id.
- **JSON** is UTF-8, written pretty with a stable key order. Keys at their default value are left out; readers must take the defaults listed below for missing keys. Most objects reject keys they do not know, so writers must not add their own.
- **Versions.** `format` names the version of `doc.json` and everything it implies; this is `klad/7`. Each version opens the older `klad/` drawings and migrates them in memory (written back on the next change). A reader that meets a newer version than it knows should refuse to write the bundle. Drawings from before `klad/1` (`.draw` folders, formats `draw-anything/0` to `/9`) do not open.

## doc.json

```json
{
  "format": "klad/7",
  "canvas": { "width": 2048, "height": 2048, "background": "#f4efe6" },
  "timeline": { "fps": 12, "frames": 24, "origin": [1024, 1800],
    "clips": [{ "name": "run", "from": 0, "to": 11, "loop": true, "fps": 24 }] },
  "export": { "godot": "../game/art/hero" },
  "palette": [{ "color": "#1b1b1b", "name": "ink" }, { "color": "#5b6ee1" }],
  "layers": [
    { "id": "l3", "name": "lines", "kind": "paint",
      "cels": [{ "id": "c1", "frame": 0, "hold": 2 }, { "id": "c2", "frame": 2, "link": "c1" }] },
    { "id": "l4", "name": "shapes", "kind": "vector", "blend": "multiply",
      "cels": [{ "id": "c4", "frame": 0, "nodes": [
        { "id": "n5", "type": "rect", "x": 0, "y": 0, "w": 2048, "h": 900,
          "fill": { "linear": { "from": [0, 0], "to": [0, 900], "stops": [[0, "#121d31"], [1, "#4a6a8c"]] } } }
      ] }] }
  ]
}
```

### The document

| Key | Type | Default | Meaning |
|---|---|---|---|
| `format` | string | | `"klad/7"` |
| `canvas` | object | | `width`, `height` (px), `background` (a colour; none is transparent) |
| `timeline` | object | 12 fps, 1 frame | see below |
| `palette` | array | `[]` | swatches: `color`, optional `name` (unique, not starting with `#`), optional `id` (unique; set once a pixel mark uses the swatch) |
| `layers` | array | | the layers, **bottom first** |
| `export` | object | `{}` | `godot`: the folder Godot exports go to, relative to the folder holding the bundle |
| `atmosphere` | object | none | fog by depth: `color` (`#9aa8b8`), `top` (a colour higher up), `density` (0.5), `blur` (1), `desaturate` (0.15) |

Strokes and fills store their colour, so for them the palette is a list of names for colours, never a reference: renaming or recolouring a swatch changes nothing drawn. Pixel marks (below) also name the swatch they were drawn with, by its `id`.

### The timeline

| Key | Type | Default | Meaning |
|---|---|---|---|
| `fps` | integer | 12 | frames per second |
| `frames` | integer | 1 | the length; it always covers every cel's first frame |
| `origin` | `[x, y]` | the canvas centre | the point an engine export puts at the node's position |
| `clips` | array | `[]` | named frame ranges, in order of their first frame |

A clip is `name` (unique), `from` and `to` (both included), `loop` (false) and `fps` (the timeline's when absent). Clips may overlap.

### Layers

| Key | Type | Default | Meaning |
|---|---|---|---|
| `id`, `name` | string | | |
| `kind` | string | | `paint`, `vector`, `group`, `adjust` or `bones` |
| `cels` | array | `[]` | paint and vector layers: the drawings on the timeline |
| `pixel` | bool | false | paint layers: a pixel layer, whose marks are whole pixels (below) |
| `children` | array | `[]` | groups: the layers inside, bottom first |
| `adjust` | object | | adjustment layers: one adjustment (below) |
| `opacity` | number | 1 | |
| `blend` | string | `normal` | `normal`, `multiply`, `screen`, `overlay`, `add`, `color-dodge`, `soft-light`, `hard-light`, `lighten`, `darken`, `color`, `luminosity` |
| `visible` | bool | true | |
| `locked` | bool | false | the editor refuses edits |
| `clip` | bool | false | show only where the base has alpha (below) |
| `lockAlpha` | bool | false | new marks land only where the layer has paint (below) |
| `mask` | layer | none | a paint layer whose alpha hides this one (below) |
| `effects` | array | `[]` | applied in order when the layer composes (below) |
| `depth` | number | none | on a top-level layer or group: how far the plane is (0 near); drives the atmosphere and parallax |
| `reference` | bool | false | shown in the editor, left out of renders and exports (pictures to trace) |
| `bones` | array | `[]` | bones layers: the skeleton (below) |
| `ik` | array | `[]` | bones layers: the skeleton's IK constraints (below) |
| `rig` | object | none | paint layers: the bones layer they move with, and the bone they follow (below) |
| `keys` | object | `{}` | animated properties (below) |

Layers compose bottom to top. Groups compose their children into one image, then blend that with the group's opacity and blend. An **adjustment layer** changes what is under it inside its group (or, with `clip`, only its base).

- **Clipping.** A layer with `clip: true` shows only where its base, the nearest layer below it in the same list without `clip`, has alpha. The base and the clipped layers above it draw as one group: each clipped layer blends onto the base with its own blend and opacity, kept inside the base's alpha; the group then blends with the base's blend and opacity. A hidden base hides its clipped layers; the lowest layer of a list is never clipped.
- **Masks.** `mask` is a paint layer of its own (own id, cels and stroke files, `strokes/<mask id>/...`) with opacity 1 and blend normal. Its alpha hides the layer: marks on the mask hide, eraser strokes on it show the layer again, and a frame without a mask cel hides nothing. `visible: false` on the mask turns it off.
- **Pixel layers.** A paint layer with `pixel: true` is a grid with no anti-aliasing where one pixel is one canvas pixel. Its drawings hold only pixel marks (a fill record whose source is cells, below): each is a set of whole pixels in the colour of one palette swatch, or a set of pixels to erase. klad refuses brush strokes and flood or lasso fills on it.
- **Lock alpha** is recorded on each stroke and fill as it is made (a flag in the stroke file), so turning `lockAlpha` off later never changes what was drawn. A locked mark composites source-atop: it changes colour where the layer has paint and leaves alpha as it was.

### Cels: drawings on the timeline

A cel is a layer's content from `frame` on.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `id` | string | | its marks are in `strokes/<layer>/<id>.strokes` (paint) |
| `frame` | integer | 0 | the first frame it shows on |
| `hold` | integer | until the next cel | how many frames it shows |
| `link` | string | none | show another cel's content (no chains) |
| `nodes` | array | `[]` | vector layers: the shapes |
| `parts` | object | `{}` | named groups of its marks: `{"leaf": [14, 15]}`, mark ids; a mark is in one part at most; unknown ids are ignored |
| `tween` | object | none | how an in-between was made (below) |
| `pose` | object | none | how a posed drawing was made (below) |

A cel that is not a link is a **drawing**; frames no cel covers are blank. Drawing on twos is `hold: 2`.

`tween` records an in-between made from two keys: `from` and `to` (the keys' drawing ids), `t` (0 to 1, after spacing), `match` (`order` or `near` when marks were not paired by id), `pairs` (forced pairs, `[first key mark, second key mark]`), `made` (a hash of the marks as made, so klad can tell whether someone drew on it since) and `unpaired` (`pop`, `pop 0.3`, `fade` or `draw-on`: how marks found in one key only were shown). The marks themselves are stored like any drawing's; the record only says how to remake them.

`pose` records a drawing bent from a rest drawing by its layer's skeleton: `rest` (the rest drawing's id), `angles` (degrees from rest per bone name), `root` (`[dx, dy]`, default `[0, 0]`) and `ik` (per IK constraint, named by its bone: `target`, `[x, y]` in canvas px, and `mix`, 0 to 1, default 1; see Skeletons). The bones' turns are `angles` with the IK solved on top of them. Its marks are stored bent, so a reader can draw it without knowing about bones.

### Vector nodes

Every node has:

| Key | Type | Default | Meaning |
|---|---|---|---|
| `id` | string | | |
| `type` | string | | `rect`, `ellipse`, `path`, `group`, `image`, `text`, `rays`, `scatter`, `repeat`, `mirror` |
| `name` | string | `""` | |
| `fill` | paint | black | as in SVG, a node without `fill` is black; `"none"` for no fill |
| `stroke` | object | none | an outline: `paint` (black), `width` (1), `cap` (`butt`, `round`, `square`; round), `join` (`miter`, `round`, `bevel`; round), `dash` (SVG-style intervals) |
| `transform` | object | identity | `x`, `y`, `rotate` (degrees), `sx`, `sy` (1), `pivot` (`[x, y]`, default the centre of the node's bounds): translate, then rotate and scale about the pivot |
| `opacity` | number | 1 | |
| `blur` | number | 0 | Gaussian sigma in px over the whole node |
| `clip` | string | none | an SVG path in the node's own coordinates that clips it and its children |
| `visible` | bool | true | |
| `keys` | object | `{}` | animated properties (below) |

And by type:

| Type | Keys |
|---|---|
| `rect` | `x`, `y`, `w`, `h`, `radius` (0) |
| `ellipse` | `cx`, `cy`, `rx`, `ry` |
| `path` | `d`: SVG path data |
| `group` | `children`: nodes, drawn in order |
| `image` | `src` (a file in `images/`, named by a hash of its pixels), `x`, `y`, `w`, `h`: the picture stretched into that box |
| `text` | `text` (new lines allowed), `font` (a file in `fonts/`), `x` (the left, centre or right of each line, by `align`: `left`, `center`, `right`), `y` (the top of the first line), `size` (32), `tracking` (extra px between letters, 0), `leading` (line height as a multiple of the size, 1.2), `weight` (a variable font's `wght`, none); filled and outlined like any node |
| `rays` | light rays from a gap: `x`, `y` (the origin), `angle` (90, down), `spread` (40), `count` (7), `length` (600), `start` (the gap's width, 0), `falloff` (1), `noise` (0.5), `seed` (1); the node's fill colours them |
| `scatter` | copies of `children` (drawn around 0,0) over the box `x`, `y`, `w` (400), `h` (200): `count` (12), `seed` (1), `size` and `turn` ranges (`[0.8, 1.2]`, `[-20, 20]`), `avoid` (SVG paths no copy's centre lands in) |
| `repeat` | `count` (4) copies of `children`, each `step` (`[40, 0]`) further, turning `turn` degrees and growing `scale` (1) about `pivot`; or a grid `columns` wide with `row` (`[0, 40]`) between rows |
| `mirror` | `children` and their reflection across the line through `x`, `y` at `angle` |

**Paint** is a colour string, `"none"`, `{"linear": {"from": [x, y], "to": [x, y], "stops": [[offset, colour], ...]}}` or `{"radial": {"center": [x, y], "radius": r, "stops": [...]}}`, in the node's own coordinates.

### Effects

`effects` is a list of one-key objects, applied in order to the layer's composed image: `[{"glow": {"radius": 24}}, {"outline": {"width": 3}}]`. Missing settings take their defaults.

| Effect | Settings (defaults) |
|---|---|
| `blur` | `radius` (4) |
| `glow` | `radius` (12), `strength` (1), `threshold` (0: every pixel glows; about 0.7: only bright ones), `color` (none: the pixels' own), `blend` (`screen`) |
| `outline` | `width` (3), `color` (`#1b1b1b`), `inside` (false) |
| `shadow` | `x` (6), `y` (8), `radius` (6), `color` (`#00000080`) |
| `noise` | `amount` (0.08), `size` (1), `seed` (1), `mono` (true) |
| `displace` | `amount` (8), `size` (48), `seed` (1), `by` (a layer id whose red pushes across and green down, grey staying put; none: a seeded noise) |
| `haze` | `amount` (0.4), `color` (`#9aa8b8`), `top`, `span` (0), `desaturate` (0) |
| `relief` | `strength` (4), `light` (`[-1, -1]`), `highlight` (`#fff4dc`), `shade` (`#1c1826`), `softness` (0.8) |

### Adjustments

An adjustment layer's `adjust` is one of:

| Adjustment | Settings |
|---|---|
| `hsl` | `hue` (degrees), `saturation`, `lightness` (-1 to 1) |
| `levels` | `black` (0), `white` (1), `gamma` (1), `low` (0), `high` (1) |
| `curves` | `points` (`[[in, out], ...]` on brightness, 0 to 1), and `red`, `green`, `blue`, applied first |
| `balance` | `shadows`, `midtones`, `highlights` (each `[cyan-red, magenta-green, yellow-blue]`, -1 to 1), `keepLight` |
| `gradient-map` | `stops` by brightness |
| `fill` | `paint`, drawn with the layer's blend |

For example `{"kind": "adjust", "adjust": {"curves": {"points": [[0, 0], [0.5, 0.3], [1, 0.85]]}}}`.

### Keys: animated properties

Layers, groups and nodes may keep `keys`: per property (`x`, `y`, `rotate`, `sx`, `sy`, `opacity`) a list of keys sorted by frame.

```json
"keys": { "y": [{ "frame": 0, "value": 0, "ease": "in-out" }, { "frame": 12, "value": -40 }] }
```

A key is `frame`, `value` and `ease` (`linear` when absent; `in`, `out`, `in-out`, `step`, or `{"curve": [x1, y1, x2, y2]}`, a cubic curve as in CSS). Between two keys the value follows the first key's ease; it holds before the first key and after the last. A layer or group moves about the canvas centre; a node's keyed values replace its transform and opacity at that frame.

### Skeletons

A skeleton lives on a **bones layer** (`kind` `bones`), which draws nothing and has no cels: its `bones` are each a `name` (unique in the skeleton), a `parent` (a bone listed before it; none for a root), a `head` and a `tail` (`[x, y]` in canvas px, at rest). A paint layer anywhere in the drawing moves with a bones layer through its `rig`: `{"skeleton": "<bones layer id>", "bone": "<name>"}` attaches it to that bone, so posed it moves rigidly with that bone's frame alone, the way a cut-out limb follows its bone, and the weights do not touch it; without `bone` it bends by the skeleton's weights. A paint layer without `rig` is not moved by any pose. Each skeleton has a weights file (below). Posed drawings store their marks already bent (see `pose` above), so bones and weights are only needed to make new poses.

A pose turns each bone about its head by its angle, its children with it, and moves the root bones by `root`. A bone's world turn is its angle plus its parent's world turn; a child's head moves with its parent's frame.

`ik` lists the skeleton's IK constraints, one per bone at most: `bone` (the end of the limb), `chain` (`2`, the default: the bone and its parent, a limb; `1`: the bone alone, an aim), `bend` (`clockwise`, the default, or `counterclockwise`: the way the limb's joint bends on screen, as the bone turns relative to its parent) and `softness` (px, default 0). A pose with an `ik` entry for a constraint solves it after the pose's angles and root, on top of them:

- **Aim** (`chain` 1): the bone turns so its tail points at the target; a target on its head leaves it as it is.
- **Limb** (`chain` 2, parent A, bone B): with s the posed head of A, l1 the rest distance from A's head to B's head (B need not start at A's tail), l2 B's length and d the distance from s to the target, the reach r is d eased by the softness (below) and clamped to between |l1 - l2| and l1 + l2. The joint (B's posed head) is the meeting point of the circle of radius l1 about s and the circle of radius l2 about the point r along the line from s to the target, on the `bend` side: B turns clockwise relative to the line from A's head to B's head for `clockwise`. A then turns so its head-to-B line points at the joint, and B so its tail is at the target (or, out of reach, straight towards it at full reach).
- **Softness** s eases the reach: r = d up to l1 + l2 - s; r = d - (d - l1 - l2 + s)^2 / (4 s) up to l1 + l2 + s; then r = l1 + l2. So the limb slows before full reach and is straight only once the target is s past it.
- **Mix** m blends each turned bone from the pose's angle a to the solved angle b the short way round: a + m wrap(b - a), wrap taking an angle to -180 to 180 degrees.

Constraints are solved parent first, by the position of the limb's top bone (A for a limb, the bone for an aim) in `bones`, then of its bone; each sees the bones as the ones before it left them. The solve is in 64-bit floats with deterministic maths, so it gives the same turns everywhere. A pose without an entry for a constraint leaves that limb to its angles. In-betweens of two poses of one rest drawing mix the angles, the root, each target and each mix by t, then solve. A constraint in one key only is handed over as turns: that key's limb bones take their solved turns, each within 180 degrees of the other key's angle, and the in-between has no entry for it. Pose angles, root and targets must be finite and mixes within 0 to 1.

## Stroke files

`strokes/<layer>/<cel>.strokes` holds the marks of one paint drawing, in the order they are drawn: brush strokes and fills (on a pixel layer, pixel marks). Every mark has an id, unique within its drawing, shared by strokes and fills. A mark's address is `<cel>:s<id>`. An id is a `u32`: its top 10 bits name the copy that made it (0 for none, else 1 + 26 × first letter + second letter, with `a` as 0) and its low 22 bits are a number counted per copy, so `s349qa` is 349 made by the copy tagged `qa`, stored as `(1 + 16 × 26 + 0) << 22 | 349`. Ids from before `klad/5`, and those of the copy a drawing was made in, are plain numbers: `s348`. Addresses accept the number in either form.

The file is one zstd frame (any level; klad uses 3). Inside:

| Bytes | What |
|---|---|
| 4 | `KLST` |
| 1 | the version: 3 (version 1 files read as strokes with nothing attached; version 2 has no pixel marks) |
| varint | the number of marks |
| ... | each mark: a kind byte (0 stroke, 1 fill), then its record |

There must be nothing after the last mark.

Numbers: a **varint** is an unsigned LEB128 integer (7 bits per byte, low bits first, the high bit set on every byte but the last). A **signed varint** is zigzag encoded first (`(v << 1) ^ (v >> 63)`). `u16`, `u32` and `f32` are little-endian. A **colour** is four `u16`: red, green, blue, alpha, sRGB with straight alpha, 0 to 65535. A **string** is a varint length and that many UTF-8 bytes.

### Strokes

| Field | Encoding | Meaning |
|---|---|---|
| id | varint | |
| brush | string | a brush id: `brushes/<id>.json` |
| colour | colour | |
| size | f32 | px, at full pressure |
| opacity | f32 | 0 to 1 |
| flow | f32 | 0 to 1 |
| seed | u32 | the seed of the brush's randomness, so a stroke replays the same |
| channels | u8 | bits: 1 tilt, 2 time, 4 taper, 8 lock alpha, 16 cuts, 32 second colour, 64 attached |
| author | u8 | 0 a person, 1 an agent |
| taper | 2 f32 | if bit 4: px over which the start and end thin to a point, whatever the pressure |
| cuts | cuts | if bit 16 |
| second colour | colour | if bit 32: for brushes whose colour moves between two |
| attached to | varint count, column | if bit 64: the ids of the marks in this drawing the stroke is attached to (see Attachments) |
| samples | varint | n, the number of samples |
| x, y | 2 columns | position in 1/16 px |
| pressure | column | 0 to 4095 |
| tilt x, tilt y | 2 columns | if bit 1: the pen's tilt, -127 to 127 for -1 to 1 |
| time | column | if bit 2: milliseconds since the previous sample |

A **column** is n signed varints, each the difference from the previous value (the first from 0). Without the tilt or time bit those values are 0. Bit 8 has no data: it marks a stroke made with lock alpha on.

### Attachments

An eraser, smudge or blender stroke (a brush with `"blend": "erase"`, or a `paint` of `smudge` or `blend`) records the marks it changed when it was drawn: the earlier marks in its drawing with paint under it. It moves, turns, is copied and is deleted with them, and splits when only some of them move.

An attached eraser erases only its marks, as if they were a layer of their own. From the eraser's first mark up to the eraser, klad keeps three layers over the eraser's reach: what was there before its first mark, its marks alone, and the other marks drawn after its first mark, each painted in order as on the drawing (erasers without attachments erase all three; smudges and blenders act on each). Where the eraser has coverage, the pixel becomes the other marks over (its marks erased, over what was there before); elsewhere it stays as drawn. When an eraser is drawn, all paint under it belongs to its marks, so the picture is the same as an eraser that erases whatever is under it. A mark that is not attached and comes under the eraser later sits below all of its marks if it was drawn before the first of them, and above them otherwise. Where two attached erasers overlap, the later one erases the earlier one's marks layer when they share a mark, and its other two layers when it holds marks the earlier one does not. An eraser with a wet stroke (`paint` of `wet`), or a smudge or blender with lock alpha, between its first mark and itself on the same area erases in order like an unattached one. Ids that no longer exist in the drawing are ignored, and an eraser with none left erases in order. Height (paint with `body`) is erased in order.

A reader that ignores attachments draws every drawing as it was when its erasers were drawn, which is the same picture until something attached moves. Smudge and blender strokes always act on whatever is under them; their attachments only say what they move with.

### Fills

| Field | Encoding | Meaning |
|---|---|---|
| id | varint | |
| colour | colour | |
| opacity | f32 | |
| author | u8 | 0 a person, 1 an agent |
| source | u8 | low bits: 0 a flood fill, 1 a polygon, 2 cells (a pixel mark); bit `0x80` lock alpha, bit `0x40` cuts |
| flood fill | f32 x, f32 y, u8 reference, 3 f32 | the seed point; what it filled from: 0 this layer, 1 everything visible, 2 a layer (then a string, its id); `tolerance`, `gap`, `expand` |
| polygon | varint n, n pairs of f32 | the polygon's points |
| cells | string, u8 | the `id` of the palette swatch the mark is bound to (empty: none), then 1 if the mark erases, else 0 |
| coverage | see below | the pixels the fill covered when it was made |
| cuts | cuts | if bit `0x40` |

A fill is drawn from its stored **coverage**, not recomputed, so it looks the same however the lines around it change (`klad refill` recomputes it on purpose). Coverage is a box and run-length coded alpha: varints `x`, `y`, `width`, `height`, a varint count of runs, then each run as a varint length and a `u8` alpha, row by row through the box.

A **pixel mark** is a fill whose source is cells. Its coverage holds only 0 and 255: the pixels it covers. It paints them in its colour, which is the colour of its swatch when it was made or last recoloured; a mark that erases clears the paint of the marks before it at those pixels and has no colour of its own. Moving one moves it by whole pixels, and scaling, flipping or turning it samples the nearest pixel, so it never gets a soft edge.

### Cuts

When a lasso cuts across a mark, the mark is not split into new geometry: each part keeps the whole mark plus a cut that shows only its side. Cuts are a varint count, then for each a `u8` (0 keep inside, 1 keep outside) and a polygon (a varint n and n pairs of f32). A mark shows only where every one of its cuts keeps it.

## Brushes

`brushes/<id>.json` is the brush that strokes naming `<id>` were drawn with. A bundle keeps every brush its strokes use, and a brush in a bundle never changes once a stroke uses it, so a drawing renders the same on every version of klad.

| Key | Meaning |
|---|---|
| `format` | `"klad-brush/1"` |
| `name` | |
| `tip` | `{"round": {"hardness": h}}`, `{"chalk": {"seed": n}}` or `{"image": {"image": name, "more": [names]}}` (several pictures take turns) |
| `spacing` | distance between dabs, as a share of the size |
| `roundness` | 1 round, less flattens the tip |
| `angle` | `"follow"` the stroke, `"tilt"` the pen, or `{"fixed": degrees}` |
| `flow`, `opacity` | per dab and per stroke (1) |
| `dynamics` | `size`, `flow`, `opacity`, each `{"pressure", "tilt", "speed"}` with curves `{"min", "max" (1), "gamma" (1)}` |
| `jitter` | `size`, `angle`, `scatter` per dab |
| `grain` | a texture: `paper` (`{"seed", "feature"}`, generated) or `image` (a picture name), `strength`, `scale` (1), `contrast` (1), `pressure` |
| `dual` | a second tip that cuts into the first: `tip`, `size` (0.5), `scatter` (1), `count` (3) |
| `color` | colour dynamics: `hue`, `saturation`, `value` per dab, `strokeHue`, `strokeSaturation`, `strokeValue` per stroke, `pressure` (a curve towards the second colour), `along` |
| `paint` | brushes that read the layer: `{"wet": {"wetness", "pickup", "dilution"}}`, `{"smudge": {"strength"}}` or `{"blend": {"strength"}}` |
| `blend` | `normal` or `erase` |
| `taper` | `[in, out]` px, used when a stroke has none of its own |
| `stabilizer`, `smoothing` | how the editor steadies the pen while drawing (strokes store the result) |
| `body` | bristle body for bristle brushes |
| `feather` | 0 to 1 (0): how far in a round tip's edge fades out, as one pass of an airbrush would; overlapping dabs don't build the fade back up, so it looks the same whatever the spacing. It takes the place of a harder `hardness` (the tip is solid up to the smaller of `hardness` and 1 - `feather`) |
| `fringe` | 0 to 1 (0): frays the edge into streaks along the stroke, like a dry brush: the tip reaches out by a different amount for each bristle line across it, varying slowly along the stroke and seeded by the stroke's `seed`; up to about half the radius at 1, the middle stays whole |

A picture named by a tip or grain is `brushes/images/<name>.png` (dark is paint, or the alpha where there is one) when the bundle has it, and otherwise one of the pictures built into klad.

Drawing a stroke: walk the samples, interpolate between them, stamp the tip every `spacing` x size along the way, with size, flow and opacity from the pressure (and tilt and speed) through the dynamics, the taper thinning the ends, and the per-dab jitter from the stroke's `seed`. A program that does not implement klad's brush engine can still draw a fair likeness with a round tip of the stroke's size and colour.

## Weights

`weights/<owner>.weights` holds the weights of the skeleton of layer or group `<owner>`: one map per bone over a grid of 4 px cells. The file is one zstd frame holding a line of JSON, a newline, then the maps:

```
{"width": 512, "height": 256, "bones": ["hip", "thigh", "shin"]}\n
<width x height bytes for "hip"><... for "thigh"><... for "shin">
```

`width` and `height` are the canvas size in cells (rounded up); each map is one byte per cell, 0 to 255, rows top first, in the order of `bones`. Weights are normalised across bones when used, and a place no bone weighs follows the nearest bone.

## Pictures and fonts

`images/` holds the pictures `image` nodes show, as PNG, each named by a hash of its contents so the same picture is stored once. `fonts/` holds the font files `text` nodes name (TrueType or OpenType) with their licence files beside them; text is shaped with HarfBuzz rules (kerning, ligatures, marks) from the file alone, never from fonts installed on the machine.

## Format history

| Format | Change |
|---|---|
| `klad/1` | the first `.klad` format (2026-11). Drawings from before it, `.draw` folders in formats `draw-anything/0` to `/9` with stroke files marked `DAST`, do not open. |
| `klad/2` | skeletons get `ik` constraints and poses their `ik` targets (2026-11-07). `klad/1` drawings open unchanged. |
| `klad/3` | paint layers under a skeleton get `bone`, attaching them to one bone (2026-11-08). `klad/1` and `klad/2` drawings open unchanged. |
| `klad/4` | skeletons move to bones layers, and paint layers join one with `rig` (2026-11-08). Older drawings open without their bones, IK, weights links and the `pose` of their cels; the drawings themselves, posed ones too, stay. |
| `klad/5` | `rev` and `nextId` leave `doc.json`, and new ids carry the tag of the copy that made them, so drawings diff and merge in git (2026-11-09). Older drawings open with their ids unchanged; their counters move to `.cache/copy.json`. |
| `klad/6` | stroke files version 2: eraser, smudge and blender strokes record the marks they are attached to, and attached erasers erase only those (2026-11-09). Older drawings open unchanged, with nothing attached. |
| `klad/7` | pixel layers: paint layers with `pixel`, pixel marks in stroke files (version 3), and palette swatches with an `id` (2026-11-11). Older drawings open unchanged. |
