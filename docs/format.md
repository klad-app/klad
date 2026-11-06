_Last updated: 2026-11-06 (format `draw-anything/9`, stroke files version 6, brushes `draw-anything-brush/1`)_

# The klad drawing format

A klad drawing is a folder whose name ends in `.draw`, called a bundle. This document describes everything in it, so that any program can read, write and make klad drawings. It is licensed under the MIT License (`LICENSE-MIT`), unlike klad itself.

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
| `weights/<owner>.weights` | how much each bone of a skeleton moves each part of the drawing | for every layer or group with `bones` |
| `.gitignore` | `.cache/`, so a drawing can live in a git repository | no |
| `.cache/` | rebuildable pixels, the undo history, the editor's selection and pinned notes | no: never read it as data |

Everything outside `.cache/` is the drawing. Copy, version or zip those files and the drawing renders the same on another machine. Read the selection and the notes through `klad selection --json` and `klad notes --json`, not from `.cache/`.

Writers replace files atomically (write a temporary file, then rename), so a reader that watches the folder never sees half a file. `rev` in `doc.json` grows by one on every write.

## Conventions

- **Coordinates** are canvas pixels, origin at the top left, y down. Angles are degrees; positive turns clockwise on screen.
- **Colours** are `"#rrggbb"` or `"#rrggbbaa"`: sRGB, straight (not premultiplied) alpha.
- **Ids** are unique across the whole document and use letters, digits, `-` and `_`. klad makes new ones from a prefix (`l` layer, `c` cel, `n` node) and `nextId`. Ids are case sensitive.
- **JSON** is UTF-8, written pretty with a stable key order. Keys at their default value are left out; readers must take the defaults listed below for missing keys. Most objects reject keys they do not know, so writers must not add their own.
- **Versions.** `format` names the version of `doc.json` and everything it implies. klad opens every older version and migrates it in memory (written back on the next change). A reader that meets a newer version than it knows should refuse to write the bundle.

## doc.json

```json
{
  "format": "draw-anything/9",
  "rev": 42,
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
  ],
  "nextId": 7
}
```

### The document

| Key | Type | Default | Meaning |
|---|---|---|---|
| `format` | string | | `"draw-anything/9"` |
| `rev` | integer | | grows by one on every write |
| `canvas` | object | | `width`, `height` (px), `background` (a colour; none is transparent) |
| `timeline` | object | 12 fps, 1 frame | see below |
| `palette` | array | `[]` | swatches: `color`, optional `name` (unique, not starting with `#`) |
| `layers` | array | | the layers, **bottom first** |
| `export` | object | `{}` | `godot`: the folder Godot exports go to, relative to the folder holding the bundle |
| `atmosphere` | object | none | fog by depth: `color` (`#9aa8b8`), `top` (a colour higher up), `density` (0.5), `blur` (1), `desaturate` (0.15) |
| `nextId` | integer | 0 | the counter new ids are made from |

Strokes and fills store their colour, so the palette is a list of names for colours, never a reference: renaming or recolouring a swatch changes nothing drawn.

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
| `kind` | string | | `paint`, `vector`, `group` or `adjust` |
| `cels` | array | `[]` | paint and vector layers: the drawings on the timeline |
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
| `bones` | array | `[]` | a skeleton (paint layers and groups, below) |
| `keys` | object | `{}` | animated properties (below) |

Layers compose bottom to top. Groups compose their children into one image, then blend that with the group's opacity and blend. An **adjustment layer** changes what is under it inside its group (or, with `clip`, only its base).

- **Clipping.** A layer with `clip: true` shows only where its base, the nearest layer below it in the same list without `clip`, has alpha. The base and the clipped layers above it draw as one group: each clipped layer blends onto the base with its own blend and opacity, kept inside the base's alpha; the group then blends with the base's blend and opacity. A hidden base hides its clipped layers; the lowest layer of a list is never clipped.
- **Masks.** `mask` is a paint layer of its own (own id, cels and stroke files, `strokes/<mask id>/...`) with opacity 1 and blend normal. Its alpha hides the layer: marks on the mask hide, eraser strokes on it show the layer again, and a frame without a mask cel hides nothing. `visible: false` on the mask turns it off.
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

`pose` records a drawing bent from a rest drawing by its layer's skeleton: `rest` (the rest drawing's id), `angles` (degrees from rest per bone name) and `root` (`[dx, dy]`, default `[0, 0]`). Its marks are stored bent, so a reader can draw it without knowing about bones.

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

A paint layer or a group may have `bones`: each a `name` (unique in the skeleton), a `parent` (a bone listed before it; none for a root), a `head` and a `tail` (`[x, y]` in canvas px, at rest). A group's skeleton moves every paint layer in it together. Each skeleton has a weights file (below). Posed drawings store their marks already bent (see `pose` above), so bones and weights are only needed to make new poses.

## Stroke files

`strokes/<layer>/<cel>.strokes` holds the marks of one paint drawing, in the order they are drawn: brush strokes and fills. Every mark has an id, unique within its drawing, from one counter shared by strokes and fills. A mark's address is `<cel>:s<id>`.

The file is one zstd frame (any level; klad uses 3). Inside:

| Bytes | What |
|---|---|
| 4 | `DAST` |
| 1 | the version, 6 now (klad reads 1 to 6) |
| varint | the number of marks |
| ... | each mark: a kind byte (0 stroke, 1 fill; from version 4, before that every mark is a stroke), then its record |

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
| channels | u8 | bits: 1 tilt, 2 time, 4 taper, 8 lock alpha, 16 cuts, 32 second colour |
| author | u8 | 0 a person, 1 an agent (from version 2) |
| taper | 2 f32 | if bit 4: px over which the start and end thin to a point, whatever the pressure (from version 3) |
| cuts | cuts | if bit 16 (from version 5) |
| second colour | colour | if bit 32: for brushes whose colour moves between two (from version 6) |
| samples | varint | n, the number of samples |
| x, y | 2 columns | position in 1/16 px |
| pressure | column | 0 to 4095 |
| tilt x, tilt y | 2 columns | if bit 1: the pen's tilt, -127 to 127 for -1 to 1 |
| time | column | if bit 2: milliseconds since the previous sample |

A **column** is n signed varints, each the difference from the previous value (the first from 0). Without the tilt or time bit those values are 0. Bit 8 has no data: it marks a stroke made with lock alpha on.

### Fills

| Field | Encoding | Meaning |
|---|---|---|
| id | varint | |
| colour | colour | |
| opacity | f32 | |
| author | u8 | 0 a person, 1 an agent |
| source | u8 | low bits: 0 a flood fill, 1 a polygon; bit `0x80` lock alpha, bit `0x40` cuts (from version 5) |
| flood fill | f32 x, f32 y, u8 reference, 3 f32 | the seed point; what it filled from: 0 this layer, 1 everything visible, 2 a layer (then a string, its id); `tolerance`, `gap`, `expand` |
| polygon | varint n, n pairs of f32 | the polygon's points |
| coverage | see below | the pixels the fill covered when it was made |
| cuts | cuts | if bit `0x40` |

A fill is drawn from its stored **coverage**, not recomputed, so it looks the same however the lines around it change (`klad refill` recomputes it on purpose). Coverage is a box and run-length coded alpha: varints `x`, `y`, `width`, `height`, a varint count of runs, then each run as a varint length and a `u8` alpha, row by row through the box.

### Cuts

When a lasso cuts across a mark, the mark is not split into new geometry: each part keeps the whole mark plus a cut that shows only its side. Cuts are a varint count, then for each a `u8` (0 keep inside, 1 keep outside) and a polygon (a varint n and n pairs of f32). A mark shows only where every one of its cuts keeps it.

## Brushes

`brushes/<id>.json` is the brush that strokes naming `<id>` were drawn with. A bundle keeps every brush its strokes use, and a brush in a bundle never changes once a stroke uses it, so a drawing renders the same on every version of klad.

| Key | Meaning |
|---|---|
| `format` | `"draw-anything-brush/1"` |
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
| 0, 1 | the first versions: brushes were not embedded (klad embeds the brushes of that time when it opens one) |
| 2 | every brush a stroke uses is embedded in `brushes/` |
| 3 | `palette`; paint drawings may hold fills (stroke files version 4) |
| 4 | `mask`, `clip`, `lockAlpha`; stroke files version 5 (lock and cuts) |
| 5 | a clip's `fps`, `timeline.origin`, `export.godot` |
| 6 | a cel's `tween` |
| 7 | a cel's `parts`, a tween's `unpaired` |
| 8 | `effects`; adjustment layers, `depth`, `atmosphere`, `reference` and the `image`, `text`, `rays`, `scatter`, `repeat` and `mirror` nodes came while the format was 8 |
| 9 | `bones` on layers, `pose` on cels, `keys`, weights files |

klad migrates any older bundle on open.
