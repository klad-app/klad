---
name: klad
description: Make and edit 2D art with the klad CLI (`klad`) in a `.draw` bundle that the user can open in the editor at the same time. Use for any drawing, illustration, game asset, icon, background or sprite work in klad, for reading what the user drew or asked for (strokes, selection, pinned notes), and for scripting art generators that emit `klad apply` ops.
---

# klad

A `.draw` bundle is a folder: `doc.json` (layers, cels, vector nodes, palette), `strokes/<layer>/<cel>.strokes` (paint strokes and fills as data), `brushes/<id>.json` (every brush its strokes use) and `.cache/` (renders, history, notes, tiles). The `klad` CLI edits it; The user edits the same bundle in the editor (`klad open`). Both see each other's changes within a second. The user starts and opens drawings from the editor's start window (cmd N, cmd O, recent) and can copy a drawing's path from the document menu at the bottom of the tool strip to hand it to you. To start one yourself: `klad new art.draw --width 1600 --height 1200 --background '#e9e4da' --sketch` (one paint layer, `l1`, and a vector layer named sketch on top, `l3`).

Working on klad itself: the repository's `CLAUDE.md`, `CONTRIBUTING.md` and `docs/DESIGN.md` are the source of truth. Build the CLI with `just build`; the binary is `target/release/klad` on macOS and `target/linux/release/klad` on Linux.

## The loop

1. Look: `klad ls art.draw` (tree, top layer first, ids and bounds) and `klad render art.draw --scale 0.5 --overlay ids,bbox`. Read the PNG it prints.
2. Change: `klad add`, `klad set`, `klad rm`, `klad mv`, `klad stroke`, `klad fill`, `klad transform`, `klad lasso`, `klad path`, `klad palette`, or many at once with `klad apply` (JSON lines, one transaction).
3. Check: add `--render` (preview, longest side 1024) and `--overlay ids,bbox` to the edit, or `klad diff` (before and after, side by side), or `klad render --focus <id>` for a close-up.
4. Undo freely: `klad undo`, `klad redo`, `klad log`. History is shared with the editor.

Keep renders small unless detail matters: `--scale 0.5` or `--focus <id>` costs far fewer tokens than full size.

## What the user wants

- `klad notes art.draw`: notes the user pinned on the canvas, each with a `--region` render command. Do the work, then `klad notes art.draw --done <id>`; the editor shows the pin as done at once and lists it under done in the Notes pane, so mark a note done only when it is.
- `klad selection art.draw`: the layer, shapes, path and strokes the user selected in the editor (strokes as `<cel>:s<id>`), with bounds and a `--focus` command.
- `klad strokes art.draw <layer> --json`: the user's strokes with points and pressure, to trace their shapes (a ridge line and tree positions from a pen sketch, for example).

## Vector nodes

Layers: `paint` (strokes), `vector` (nodes), `group` (children). Add a vector layer, then nodes into it:

```
klad add art.draw layer vector name=sky
klad add art.draw rect --in l1 x=0 y=0 w=2048 h=900 'fill={"linear":{"from":[0,0],"to":[0,900],"stops":[[0,"#20304f"],[1,"#d98b5f"]]}}'
klad set art.draw n3 transform.rotate=-8 opacity=0.8 blur=4
```

- Types: `rect` (x, y, w, h, radius), `ellipse` (cx, cy, rx, ry), `path` (SVG `d`), `group` (children).
- Every node: `fill`, `stroke` (`paint`, `width`, `cap`, `join`, `dash`), `transform` (`x`, `y`, `rotate`, `sx`, `sy`, `pivot`), `opacity`, `blur` (soft edges, glows, haze, shadows), `clip` (SVG path that clips the node and its children), `visible`, `name`.
- Paint: `"#rrggbb"`, `"#rrggbbaa"`, `"none"`, `{"linear":{"from","to","stops"}}`, `{"radial":{"center","radius","stops"}}`.
- As in SVG, a node without `fill` is black; use `fill=none` for outlines only.
- Unknown keys fail with the list of valid ones. Structure (`id`, `type`, `kind`, `children`, `cels`, `nodes`) only changes through add, rm and mv.
- Ids are case sensitive; overlays print them exactly.

## Painting

```
klad stroke art.draw l4 --brush ink --size 14 --color '#1b1b1b' --path 'M100 400 C 300 200 500 600 700 400' --pressure 0.1,1,0.05
klad stroke art.draw l4 --brush ink --size 5 --taper 8,20 --path 'M100 100 L140 220'
```

- `klad brushes art.draw` lists the brushes: the ones the bundle embeds and the library (`ink` inking pen, `pencil`, `marker`, `airbrush`, `soft`, `chalk`, `eraser`, and for painting `flat-bristle`, `round-bristle`, `sponge`, `fog`, `leaves`, `grass`, `splatter`, `wet`, `smudge`, `blender`, `dry-brush`, `two-tone`); the tip column says `+dual`, `+texture`, `+colour`, `+wet`, `+smudge`, `+blend` or `+body`. `--show <id>` prints one as JSON. A bundle keeps every brush it uses, so `ink` in an older bundle can be the older ink; `--brush` takes any id from that list.
- Painting brushes: `wet` picks up the colour under it and mixes it in, `smudge` pulls paint along, `blender` softens what it crosses; all three read only the layer they paint on (paint the colours to mix on one layer). `leaves`, `grass` and `splatter` scatter their picture tips with jittered colour; `two-tone` paints `--second` at a light press and `--color` at a full one. `--second '#e0a040'` (`"second"` in `apply`) is the second colour for brushes with colour dynamics.
- New brushes: write the JSON (copy one from `--show`; keys `tip` (`round`, `chalk` or `{"image":{"image":"name"}}`), `dual`, `grain` (`paper` or `image`, `strength`, `scale`, `contrast`, `pressure`), `color` (`hue`, `saturation`, `value`, `strokeHue`, ..., `pressure`, `along`), `paint` (`wet`, `smudge`, `blend`)) and `klad brushes art.draw --set my-oil brush.json` (`-` reads stdin). Pictures for tips and textures: `klad brushes art.draw --picture bark bark.png` (dark is paint, or the alpha). Ids and pictures are fixed once in the bundle, since strokes keep their brush.
- `--taper in,out` thins the first and last px of the line to a point whatever the pressure; without it the brush's taper applies (`ink` is `10,24`), `--taper 0,0` gives round ends. Short hatching wants about `8,20`, long contours `30,80`.
- `--pressure` spreads values evenly along the stroke; several subpaths in `--path` make several strokes. `--points 'x,y,p x,y,p'` gives explicit samples.
- In `apply`: `{"op":"stroke","layer":"l4","brush":"ink","size":14,"color":"#1b1b1b","path":"M...","pressure":[0.1,1,0.05],"taper":[12,30]}`.
- Agent strokes are marked `agent` in `klad strokes`; the user's are `human`.

## Editing strokes and fills

Every stroke and fill has an address `<cel>:s<id>` (fills share the id counter with strokes). `klad strokes art.draw <layer>` lists them with the cel id on top.

```
klad set art.draw c4:s12 color=#c84a3a size=6 brush=pencil
klad set art.draw c4:s7 color=skin opacity=0.8
klad rm art.draw c4:s12 c4:s13
klad transform art.draw c4:s3 c4:s5 --move 40,-10 --rotate 15 --scale 1.2
klad show art.draw c4:s12
klad render art.draw --focus c4:s12
```

- Strokes take `color`, `size`, `opacity`, `flow`, `brush`, `taper` (`[in,out]` or `null`) and `path`; fills take `color` and `opacity`. The stroke keeps its path, pressure and seed, so a recoloured stroke is the same mark in a new colour.
- Bending a stroke: `klad strokes --json` gives each stroke a fitted `path` (a few points); edit it and `klad reshape art.draw c4:s12 --path 'M...'` (or `set` with `path`). Every sample keeps its pressure, tilt and timing at the same fraction of the length, so the line keeps its weight and taper. The user does the same with A in the editor.
- `transform` moves, rotates (degrees, clockwise) and scales about `--pivot` (default: the middle of the marks' bounds). Stroke sizes and tapers scale with them; fills move by whole pixels and are resampled when turned or scaled. Ids in several cels need a `--pivot`.
- In `apply`: `{"op":"set","id":"c4:s12","color":"#c84a3a"}`, `{"op":"rm","ids":["c4:s12"]}`, `{"op":"transform","ids":["c4:s3","c4:s5"],"x":40,"y":-10,"rotate":15,"sx":1.2,"sy":1.2}`.
- Edits redraw only the tiles the marks touch, from the nearest checkpoint, so they are cheap on large cels.

## Lasso

```
klad lasso art.draw l4 --polygon '600,600 1400,560 1450,1300 640,1380'
klad lasso art.draw l4 --polygon '600,600 1400,560 1450,1300 640,1380' --cut --move 40,-25
klad lasso art.draw l4 --polygon '...' --cut --rotate 12 --scale 1.1
klad lasso art.draw l4 --polygon '...' --flip h
klad lasso art.draw l4 --polygon '...' --cut --rm
```

- Without a move, turn, scale, flip or `--rm` it lists the strokes and fills inside the polygon and those crossing its edge.
- Without `--cut` only whole marks move (strokes whose points are all inside, fills whose pixels are). With `--cut` each mark crossing the edge is split: the outside part stays put, the inside part becomes a new mark that moves with the rest, so nothing outside the lasso changes. The output names the moved marks (`lasso on l4 moved c2:s10 ...`); they stay strokes and can be edited again.
- `klad transform` takes `--flip h` or `--flip v` too. In `transform` and `lasso`, `--scale` scales the marks; their previews use the default size.
- In `apply`: `{"op":"lasso","layer":"l4","polygon":[[600,600],[1400,560],[1450,1300]],"cut":true,"x":40,"y":-25}` (also `rotate`, `sx`, `sy`, `scale`, `pivot`, or `"rm":true`).

## Masks, clipping and lock alpha

```
klad set art.draw l5 clip=true blend=multiply
klad set art.draw l3 lockAlpha=true
klad add art.draw mask --in l5
klad stroke art.draw l9 --brush airbrush --size 180 --path 'M600 600 L 640 700'
klad set art.draw l9 visible=false
klad rm art.draw l9
```

- `clip=true` shows a layer only where the nearest unclipped layer below it (its base) has paint. Clipped layers blend onto the base with their own blend; the group takes the base's opacity and blend. This is how shading stays inside flat colour: flats on one layer, a multiply shade layer and a rim light layer above it, both clipped.
- `klad add art.draw mask --in <layer>` gives a layer a mask and prints its id. The mask is a paint layer of its own (`klad ls` shows it under its layer): strokes and fills on it hide the layer, the eraser brings it back, an empty mask hides nothing. `hide=true` starts it hiding everything. `visible=false` on the mask turns it off; `klad rm` on its id removes it.
- `lockAlpha=true` on a paint layer: new strokes and fills only land where the layer already has paint, which recolours line art (a big soft stroke over lines tints only the lines). It is recorded per stroke, so set it back to false when done, or the user's next pen strokes on that layer only land on existing paint. The eraser does nothing on a lock-alpha layer.
- In `apply`: `{"op":"add","mask":{"name":"light"},"in":"l5"}`, `{"op":"set","id":"l5","clip":true}`.

## Light, effects and adjustments

```
klad effect art.draw l3 glow radius=24 color=#ffd080
klad effect art.draw l3 outline width=4 color=#101418
klad effect art.draw l3              # lists them; --rm 1 removes the second, --clear all
klad add art.draw layer adjust curves 'points=[[0,0],[0.5,0.3],[1,0.85]]' 'blue=[[0,0.12],[1,1]]' name=dusk
klad add art.draw rays --in l7 x=430 y=0 angle=95 spread=40 count=9 length=600 start=120 fill=#fff0c0c0 blur=6
klad set art.draw l5 blend=soft-light
```

- Effects are settings on a layer (or group), applied in order when it composes and always editable: `blur` (radius), `glow` (radius, strength, threshold: 0 glows from every pixel, about 0.7 only from the bright ones, as bloom; color tints it; blend: screen by default, add for hard light, normal lays it like paint), `outline` (width, color, inside=true), `shadow` (x, y, radius, color with alpha), `noise` (amount, size, seed, mono=false for coloured grain), `displace` (amount, size, seed; by=<layer> pushes with that layer's red across and green down, grey stays put; a hidden layer works as the map), `relief` (strength, light=x,y as in `paint --light`, highlight, shade, softness: lights the paint's thickness, so ridges and stroke edges facing the light catch it and the others darken; only strokes from brushes with `body` have thickness). In `apply` or `set`: `effects=[{"glow":{"radius":24}},{"outline":{"width":3}}]`.
- Bloom: put everything that should bloom in a group and give the group a glow with a threshold and blend add. A light layer is a paint layer with `blend=add` and a soft glow (`'effects=[{"glow":{"radius":24,"strength":0.8}}]'`): paint light into it with `soft` or `airbrush`.
- Blend modes: normal, multiply, screen, overlay, add, color-dodge, soft-light, hard-light, lighten, darken, color, luminosity.
- Adjustment layers change what is under them in their group (or, with `clip=true`, only their base): `hsl` (hue in degrees, saturation and lightness -1..1), `levels` (black, white, gamma, low, high), `curves` (points, and red, green, blue applied first; x is the input brightness, y the output), `balance` (shadows, midtones, highlights as [cyan-red, magenta-green, yellow-blue] in -1..1, keepLight), `gradient-map` (stops by brightness), `fill` (paint: a colour or gradient, drawn with the layer's blend). Their opacity, blend and mask apply. To grade a scene, put one curves layer on top; to grade one part, put the part and the adjustment in a group.
- Rays are a vector node: an origin, a direction (`angle` in degrees, 90 is down), `spread`, `count`, `length`, a gap `start` px wide they come through, `falloff`, `noise` and `seed`. The node's `fill` colours them and `blur` softens them; a vector layer of rays on `screen` or `add` reads as light through trees.
- `klad ls` shows `fx: glow outline` on layers with effects and the type of an adjustment layer; `klad show <layer>` lists their settings.

## Depth, pictures, generators and text

```
klad set art.draw g2 depth=2.5                       a plane (a layer at the top of the stack): 0 is near
klad atmosphere art.draw color=#8fa3b8 top=#d3dde7 density=0.55 blur=1.2 desaturate=0.25
klad add art.draw layer image src=photo.jpg reference=true
klad add art.draw repeat --in l4 count=8 'step=[60,0]' 'children=[{"type":"rect","x":0,"y":400,"w":12,"h":80,"fill":"#3a2a1a"}]'
klad add art.draw scatter --in l4 x=0 y=700 w=1600 h=150 count=40 seed=7 'size=[0.6,1.6]' 'turn=[-30,30]' 'children=[{"type":"ellipse","cx":0,"cy":0,"rx":20,"ry":11,"fill":"#3a4450"}]'
klad add art.draw mirror --in l4 x=800 angle=90 'children=[{"type":"path","d":"M560 900 L560 560 L760 410 Z","fill":"#0d1117"}]'
klad font art.draw ~/fonts/Signage.ttf
klad add art.draw text --in l4 'text=THE CROOKED INN' font=Signage.ttf size=46 x=340 y=96 align=center tracking=4 fill=#f3d9a4
```

- Depth: give each top-level group (plane) a `depth`; with an atmosphere every plane fades towards the fog (and the `top` colour higher up), blurs and loses colour, more the deeper it is. Near planes keep depth 0. The user previews the parallax by holding D. `haze` is also an effect of its own (`klad effect art.draw l3 haze amount=0.4 color=#9aa8b8`).
- References: `reference=true` on any layer keeps it visible in the editor but out of `klad render`, `klad hash` and `klad export` (add `--references` to see it). Pictures to trace over go there.
- Pictures: `layer image src=<PNG or JPEG>` copies the file into the bundle (`images/<hash>.png`) and places it on a new vector layer, fitted inside the canvas; `x=`, `y=`, `w=`, `h=` place it. The node is `{"type":"image",...}`, moved and scaled like any shape.
- Generators keep their settings: `repeat` (count, step, turn and pivot for an arc, scale, columns and row for a grid), `scatter` (region x, y, w, h, count, seed, size and turn ranges, `avoid`: SVG paths no copy lands in), `mirror` (x, y, angle of the axis). Children are drawn once per copy; for scatter draw the child around 0,0. `klad add <bundle> <shape> --in <generator id>` adds more children.
- Text: the font must be a file in the bundle (`klad font`); a missing font is refused with its name. `text` takes new lines (`$'line one\nline two'` in bash), `x` is the left, centre or right of each line (`align`), `y` the top of the first line; `tracking` is extra px between letters, `leading` the line height as a multiple of the size; fill and `stroke` colour and outline it.

## Path points

```
klad path art.draw n7
klad path art.draw n7 --move 3 412,230
klad path art.draw n7 --nudge 3,4,5 0,-20
klad path art.draw n7 --handle 3 out 430,210
klad path art.draw n7 --add 418,236
klad path art.draw n7 --rm 4,5 --corner 2 --smooth 6
klad path art.draw n7 --simplify 1.5
```

- Without flags it lists the points (anchors) of a path, rect or ellipse, numbered across subpaths, with handles and corner/smooth/curve. Coordinates are the shape's own (as in its `d`, before its transform). After an edit it lists them again; numbers shift after inserts and removals.
- Any point edit turns a rect or ellipse into a path that looks the same. `--simplify <px>` refits a dense polyline (traced or generated) as a few smooth points within that distance: do it before reshaping something like a ridge line.
- A smooth point keeps its handles in line when one moves (`"free":true` breaks it). Pivots of transformed shapes and their parent groups are pinned first, so editing points never shifts the rest of the shape.
- In `apply`: `{"op":"path","id":"n7","move":3,"to":[412,230]}`, `"move":[3,4],"by":[0,-20]`, `"handle":3,"out":[430,210]`, `"insert":3,"t":0.5`, `"add":[418,236]`, `"rm":[4,5]`, `"corner":2`, `"smooth":6`, `"simplify":1.5`, or no key to only convert.

## Colour and fills

```
klad palette art.draw --add '#f0c8a0' --name skin
klad fill art.draw l2 --at 820,640 --ref lines --gap 4 --color skin
klad fill art.draw l2 --polygon '100,100 400,120 260,380' --color '#20304a'
klad refill art.draw l2
```

- The palette lives in `doc.json`. Any colour argument takes `#rrggbb`, `#rrggbbaa` or a palette name. `klad palette art.draw` lists it; `--add` with an existing `--name` changes that colour; `--rm` takes a name, a colour or an index.
- A flood fill starts at `--at` and spreads over pixels within `--tolerance` (0 to 1, default 0.1) of the seed pixel in the reference: `this` layer (default), `all` (everything visible, background included) or a layer id or name. Put flat colour on a layer under the lines and use `--ref <lines layer>`; a fill on the line layer itself covers the inside edge of the lines.
- `--gap` closes openings in the lines up to that many px wide (inked shapes with small gaps); `--expand` (default 2) tucks the fill under the lines so no paper shows between line and colour.
- Fills are records next to the strokes, with their coverage stored, so they render the same later. When the lines change, `klad refill <bundle> <layer>` recomputes every flood fill of that cel (`--id` for one).
- In `apply`: `{"op":"fill","layer":"l2","at":[820,640],"ref":"lines","gap":4,"color":"skin"}`, `{"op":"palette","add":"#f0c8a0","name":"skin"}`. Strokes and fills in one batch see each other in order, so ink then fill in one `apply`.

## Drawing from shapes

Describe a piece instead of programming it: block the subject in as vector shapes on a hidden sketch layer (rects, ellipses, paths, groups; give them ids so later ops can name them), then let these commands draw on paint layers. Everything they make is ordinary strokes and fills, so `klad set`, `klad transform` and the user's pen edit it afterwards.

```bash
klad add art.draw layer vector name=sketch visible=false
echo '{"op":"add","node":{"id":"body","type":"ellipse","cx":400,"cy":420,"rx":160,"ry":120},"in":"<sketch id>","frame":0}
{"op":"add","node":{"id":"eye","type":"ellipse","cx":450,"cy":380,"rx":18,"ry":18},"in":"<sketch id>","frame":0}' | klad apply art.draw
klad fill art.draw flats --shape body --color skin          # exactly the shape's area, grown 2 px under the ink
klad light art.draw --from body --light -1,-1 --shade shade --rim rim --depth 20   # shade away from the light, a rim towards it
klad ink art.draw lines --from body,eye --weight 7 --light -1,-1                   # tapered strokes, weight from rules
klad hatch art.draw shade --in body --angle 30 --spacing 8 --length 10-24 --color shadow
klad scatter art.draw lines --in ground --mark "M0 0 Q 3 -22 8 -40" --count 20 --turn -25,25
klad paint art.draw paint --in body --color skin --light -1,-1 --weight 12                      # a round form, painted from a light model
klad paint art.draw paint --in pillar --form cylinder --axis 90 --lamp 200,600,150 --color '#c9b8a0'
klad prepare art.draw                                                              # buffer every drawing before the user opens it
```

- Painting a form: after the flats, `klad paint` paints each shape as a form. `--form round` (default) bulges any closed shape like a pillow, its height growing with the distance to the outline; `flat` is a plane facing `--facing x,y,z` (z toward the viewer, y down); `cylinder` is round across `--axis` degrees. The light model is a key light from `--light x,y` at `--elevation` (or `--lamp x,y,height` in canvas px: the shape nearest the lamp gets the full key light, farther ones less by inverse square, so paint a cluster in one op), cool fill from above (`--fill`, `--fill-strength`), warm bounce from below (`--bounce`, `--bounce-strength`) and a darker core just past the terminator (`--core`); `--color` is the local colour in full light, so the lit side comes out at that colour and the shadow side about four value steps under it. The lights multiply the local colour, so a warm colour goes darker and browner in shadow; `--shadow <colour>` (`"shadow"` in `apply`) turns the side out of the key light towards that colour's hue and chroma while keeping the values the model gives it, by `--shadow-mix` (default 0.7) and less in the halftones, with the chroma lowered in the darkest parts. A cool lavender or blue grey keeps shadows from going brown or grey; raise `--fill-strength` (0.3 or so) for lighter shadow sides. Every colour option of `paint` takes palette names. Strokes follow the form (along bands of equal light on round forms, along the axis on cylinders, at `--angle` on planes), take their colour from the model where they lie and stop where the light changes, so none carries the lit colour into the shadow. Thick paint: `oil` and `oil-load` have `body` (a brush key, 0 to 1), so their strokes lay paint height with bristle grooves and a lip at the edge, building up where they cross; `smudge` and `wet` drag it, `blender` flattens it, the eraser takes it. It shows only through a `relief` effect on the layer (`klad effect art.draw <layer> relief strength=3 light=-1,-1`): about 4 for fruit or a figure, 1 to 2 for a table or a wall, whose short decisive strokes otherwise stand out like stickers. Without the effect body changes no pixel. Passes go from twice `--weight` down to half of it (the small ones only where the form turns), then a decisive pass with `--bold` (`oil-load`) in the lights and a few quiet strokes in the shadows (`--decisive 0` skips both); `--vary` lets each stroke stray in value and temperature. Paint on a layer clipped to the flats, or lock the flats layer and paint into it: strokes then run past the outline and short strokes along it cut the edge in; on a plain layer they stay inside it and the flats show at the rim.
- Every stroke the shape ops make is drawn like a hand (`--hand 1`, the default; `0` gives the old even strokes): its own pressure curve (a press-in, a peak, a longer lift-off), tapers that are a share of its length, a width that varies from stroke to stroke, and a slight bow on hatching and flat planes. Brushes for it: `oil` (soft edge, size and opacity from pressure) for building forms, `oil-load` (harder edge, more grain) for strokes that should stay visible.
- `klad light` takes `--lamp x,y,height` too: each shape is shaded away from the lamp instead of one direction for all.
- `klad prepare art.draw [--from 0 --to 23]` replays every painted drawing of those frames into the cache after a big CLI change, so the editor opens and plays it without replaying strokes. The editor still makes its frame pictures itself, quickly, from the cached drawings.

- Ink rules: heavier on the side away from `--light` (the direction the light comes from, -1,-1 is top left), heavier on the silhouette than on inner lines (an outline inside another shape given in the same call), thinner on tight curves, a seeded wobble (`--wobble 0` for none). Outlines split into separate strokes at corners sharper than `--break` (60 degrees); `--gap 3` leaves inkers' gaps where lines meet. Open details (folds, cracks, ribs) go through `--path "M..."`.
- `klad light` writes lasso fills with a cut: on the shade layer the shape minus itself moved towards the light (`--soft 3` overlapping steps soften the inner edge), on the rim layer a band along the lit edge. Clip the shade layer (multiply) and the rim layer to the flats, as in `samples/lantern/`.
- `samples/lantern/generate.py` is the whole recipe for a prop in about 100 lines; `samples/ink-shapes/` shows the ink options side by side.
- Details that make a prop read as made rather than blocked in (an early version of the lantern read "kind of as good, but its details are not too great"): connect what hangs (hook, links, bead, cap: every piece touches the next; a link seen face-on is a closed ring through `--path`, one seen edge-on a small filled bar); give a round form a crisp core shadow with its own `light` pass (`--soft 1`, depth about a third of its width, `shadeOpacity` 1) instead of a soft gradient; build a stone or block from two planes (a top face and a darker side as its own shape) and keep texture to the side plane; plant grass as a few tufts where things meet the ground, leaning away from the object, not a fringe along an edge; ink a shape that enters another as an open path, so its cut edge is not drawn on top.
- The ops are `ink`, `light`, `hatch`, `paint`, `scatter` and `fill` with `shape`; shapes added earlier in the same `apply` can be used by them. `samples/lantern/` builds the inked prop, and with `PAINTED=1` the painted one (`paint` by form in place of `light` and `hatch`).

## Building in 3D and painting stroke by stroke

Anything with straight edges or round rims (boxes, buildings, cylinders, cups, rims of holes) is built in 3D and projected, never typed in as corners: typed corners give vanishing points at different heights and verticals of the same length near and far. `docs/skill/drawkit.py` (Python, no dependencies; `python3 docs/skill/drawkit.py --check` tests it) has a `Camera(eye, focal, centre, horizon)` with `project`, `box` (corners and faces with normals), `visible_faces`, `sphere_points`, `cylinder_points`, `extrusion_points`, `silhouette` (the hull of projected points), `shadow` and `shadow_shape` (points projected along the light onto the ground), and `stroke`, the same hand as the CLI's, for generators that place their own strokes. A level camera with the horizon on the canvas gives two-point perspective with upright verticals; moving the camera's centre on the canvas moves the subject without bending the perspective.

When a piece needs more than `klad paint` gives, paint it stroke by stroke in a generator, the way `samples/solids/generate.py` does:
- One locked paint layer per plane (each visible face of a box, a cylinder's body and its top), filled with its own middle value, then painted into; one base under several planes shows through as streaks of the wrong value.
- Strokes that follow the form, big to small, each coloured from a light model at its own place; cut a stroke where the surface turns away from the viewer and where the light changes, and keep bow off on strokes that follow a value band.
- Then a decisive pass: fewer, larger, visible strokes in the lights, quieter ones in the shadows, loose broad ones on backgrounds.
- Then material detail where the light rakes and at the focus: pits with a dark side toward the light, grain in clusters, a few chips of varied size, crazing in glazes, grime toward the base.
- Cast shadows and contact darkening as soft shapes on a `multiply` vector layer, a crisp copy near the contact and a soft one farther out, so the surface's brushwork shows through.
- Big masses before texture: a large surface (a cave roof, a wall) gets its light as broad strokes first and quiet low-contrast forms after; small strokes laid evenly read as ripples or gravel.
- Where one form grows out of another (a stalactite from rock), paint the parent over the join.
- Zoom in on every object (`klad render --region x,y,w,h --scale 2`) after each pass and fix what you see before showing the user.

## Studying a reference

To recreate a picture (a reference study): `klad add art.draw layer image src=ref.jpg reference=true`, then size it to the canvas with `klad set art.draw <node> x=0 y=0 w=<width> h=<height>` (a picture smaller than the canvas is placed at its own size). Read the reference in quarters with `klad render art.draw --references --region x,y,w,h` (a reference under an opaque background is covered: hide the background or render the reference alone), and take coordinates from those crops. Build in planes from back to front, compare often on one sheet (a bundle holding the reference picture above the render), and fix values before details: the first passes of a cave study were too bright everywhere, not too plain.
- Light layers and mist on `screen` go behind the planes they light from behind; above a plane they turn its forms milky.

## Export

```
klad export art.draw --scale 2
klad export art.draw --layer hero --scale 4 --depth 16 -o hero.png
```

Transparent PNG of the image or one layer or group, next to the bundle unless `-o`. 8-bit exports are dithered so gradients do not band; `--depth 16` keeps 16 bits; `--background` keeps the canvas colour. Vectors stay sharp at any scale, and above full size `klad export` draws the strokes again at that size, so lines stay crisp and textures keep their size (it replays every mark, so it takes a few seconds on a dense painting). Sprite sheets, numbered frames and Godot exports are under Time, Animation exports.

Size the canvas for the largest size the art is ever seen at, not for the size it is used at. Paint layers are pixels at the canvas size: `klad render --scale` above 1, the editor's zoom, a sprite scaled up or a game camera that zooms in all magnify them, and every pixel shows as a block (only `klad export --scale` draws them again). Work out the largest size on screen (the camera's closest zoom times the sprite's scale, times 2 on a high density display), make the canvas about twice that, and show it scaled down (in Godot with mipmaps on). At that size give inked lines 4 to 8 px and noise or grain effects a `size` of 2 or more: lines of 2 to 3 px and 1 px noise read as pixels the moment anything enlarges them. Check at the size the player sees and at the closest zoom, not only at 1:1 on the canvas.

## Scripts

For anything with more than a handful of shapes, write a generator that prints ops as JSON lines and pipe it into `klad apply`. Rebuild from scratch each run (copy the source bundle, apply, render) so the script stays the source of truth. A landscape generator of about 18,000 nodes builds and renders in about 4 s this way.

## Time

`timeline` has `fps` and `frames`; each layer has cels (`frame`, optional `hold`, optional `link` to reuse another cel). A cel that is not a link is a drawing; the frames it shows on are its exposure (from `frame` for `hold` frames, or until the next cel). `--frame n` on `ls`, `render`, `stroke`, `fill`, `lasso` and `add` picks the frame; a stroke or fill paints into the drawing exposed there.

```
klad frames art.draw                          # exposure sheet: c4 = a drawing starts, | still showing, c9->c4 a link, . blank
klad timeline art.draw --fps 12 --frames 24   # length and speed (no flags: show them, the origin, the Godot folder and the clips)
klad cel art.draw new l3 --frame 4            # blank drawing; inside an exposure it takes over the rest of it
klad cel art.draw dup c4 [--frame 6] [--insert]   # copy (strokes keep their ids); default right after c4; --insert moves later cels back
klad cel art.draw link c4 --frame 8 --hold 2  # show c4 again
klad cel art.draw unlink c9                   # the link becomes a drawing of its own
klad frame art.draw insert 4 [--count 2] [--layers l3]   # what showed at 4 shows longer, the rest moves back
klad frame art.draw remove 4 [--count 2] [--layers l3]
klad hash art.draw --all                      # one pixel hash per frame
```

In `apply`: `{"op":"cel","new":"lines","frame":4,"hold":2,"id":"ball4"}` (the `id` lets later ops in the same batch name it), `{"op":"cel","link":"ball0","frame":15}`, `{"op":"frames","insert":4,"count":2}`, `{"op":"timeline","frames":24}`. For an animation script: make every drawing's cel first (`cel new` with an `id`), then its strokes and fills at that frame. `samples/bounce/generate.py` is the pattern: lines, colour filled against the lines, a multiply shadow, a held floor, the rest pose as links.

Strokes copied by `dup` keep their ids, so `c7:s3` in the copy is the same line as `c4:s3`: move it with `klad transform` to make the next pose. Removing a drawing that links show (`rm`, or removing its frames) gives it to the first link instead of breaking them.

### Keys, in-betweens, colour and timing

Animate by keys: draw the key poses with their parts named, check the keys' arcs, let `klad inbetween` make the drawings between them, colour every drawing from the first with `klad fill --like`, check with `klad gaps`, set the timing with `klad retime`, check the arcs again. `samples/creature/` (`generate.py` and `build.sh`, which writes `creature.draw`) is the pattern: an idle and a walk on six layers (shadow, far leg, colour, a clipped multiply shade, a clipped rim light, lines), parts body, sprout, leaf, second leaf, near leg and far leg, `klad arcs --check` before and after in-betweening, the loop closed with `--loop`.

```
klad copy art.draw c1:s3 c1:s4 --frame 6 [--layer shade] [--move 12,-4 --rotate 8 --pivot 512,400] [--cut 'x,y ...']
klad inbetween art.draw --layers lines,colour,shade --from 0 --to 6 [--count 2] [--spacing in-out] [--match near] [--pair s3=s7]
klad inbetween art.draw --layers lines,colour --from 9 --loop      # the loop's last in-betweens, towards its first drawing
klad inbetween art.draw --layers lines --from 0 --to 6 --unpaired pop|fade|draw-on [--at 0.3]
klad inbetween art.draw --redo [--layers lines] [--keep-drawn | --force]
klad part art.draw c4:s14 c4:s15 --name leaf    # or --part leaf on stroke, fill, ink, hatch, scatter
klad arcs art.draw --track 'lines:near leg' --track 'far:far leg' --clip walk --ground 823,811 --travel 7 --check
klad render art.draw --frame 20 --track 'lines:near leg' --region 420,720,200,120 --scale 4   # the paths drawn
klad render art.draw --sheet walk --drawings --overlay parts
klad fill art.draw colour --like colour0 --frames all
klad gaps art.draw colour --sheet all          # closed regions no fill covers, with a render marking them
klad retime art.draw 0-11 --on 2               # or --on 1, --fit 16, --reverse, --pingpong; [--layers l3]
```

- Name parts as you draw keys (`--part`, or `"part"` in ops): in-betweens pair part by part first, whatever the ids, so keys drawn separately, or redrawn, or with the user's strokes in between, still pair. Give each piece that moves on its own its own part (one per leaf, per limb): within a part marks pair in drawing order. Without parts keys pair by mark id, so make every key with the same ops in the same order, or `cel dup` a key and change the copy. `--match near` pairs drawings made separately (and turns strokes drawn backwards); `--pair` forces the odd one. Unpaired marks are left out and listed by default: read that list, or let them `--unpaired pop|fade|draw-on`.
- Check the keys' motion before in-betweening, the way an animator flips them: `klad arcs` with a track per foot (parts), `--ground` per foot (a far foot drawn higher for depth has its own line), and `--travel` for a cycle drawn in place (a planted foot moves back by the travel per frame; on twos after retime, halve it). It reports feet under the ground, planted feet sliding, and two feet swapping sides (legs crossing); `--check` makes the build stop. The creature's first keys slid their feet with the body's tilt: feet go where the ground is, not where the hip is.
- A stroke whose end touches another line in both keys rides on that line (a hand on an arm, a leaf on a stem) and its end stays on it, so closed shapes stay closed. Make joins actually touch in every key (within half the line's width plus 2 px); parts that only float near each other move on their own arcs and drift apart mid-way.
- A loop's last in-betweens: `--from <last key> --loop` in-betweens towards the loop's first drawing (the clip holding the frame, else frame 0) up to the loop's end. Add the clips before in-betweening.
- The editor's in-between panel runs the same `klad inbetween`; its redo takes a choice per in-between: `--redo --only c12,c14,c16 --keep-cels c14 --force-cels c16` (the rest of `--only` only if unchanged). Its History pane shows your steps as "agent" next to the user's, and they can undo back to any of them; a step keeps its author and time through undo and redo, so `klad log` still says whose it was.
- In-betweens show as `c12*` in `klad frames`; `klad show c12` gives `tween` (keys, t). After changing a key, `--redo` remakes them; it refuses in-betweens someone drew on or edited. `--keep-drawn` remakes the agent's marks and keeps the user's on top, carried with the line under them (with his part, the lines of that part); `--force` in-betweens his marks with the rest. Ask him which he wants when he drew corrections; keep-drawn is the safe default. An in-between layer list goes together: list the lines and the layers that fill against them in one call so the fills are recomputed against the in-between lines.
- Flood fills in in-betweens are recomputed from seeds carried by the lines; fills that land on a line or leak are left out and listed (`fills left out: ...`). `klad gaps --sheet all` afterwards should say "nothing left uncovered".
- `--like` finds each fill's region in the other drawings by shape near where it was; it holds while drawings overlap from one to the next (keys of one character). It replaces a fill with the same id, so running it again is safe. Lines that fence a region into pockets (hatching or stitches closer together than the fill's gap) show up in `klad gaps`.
- `klad retime` keeps layers in step: the range is cut where any layer changes, so a held background stretches with the character. `--pingpong` (a b c d c b) and repeated drawings become links. On every layer (no `--layers`) clips and later frames move with it.
- `klad copy` gives the copies new ids and prints the map (`c1:s3 -> c9:s12`); copying from a key into another drawing keeps the copies unpaired with the original ids, so prefer `cel dup` when you want ids kept.
- The in-between and `--like` ops need a batch of their own in `klad apply`.

### Clips

A clip is a named frame range (both ends included) that exports as one animation: `idle`, `run`, `attack`. It loops or plays once, and may have its own fps (otherwise the document's).

```
klad clip art.draw                                          # list: frames, length in s at its fps, loops or once
klad clip art.draw --add run --from 0 --to 11 --loop --fps 24
klad clip art.draw --set run --to 15 --no-loop              # change; --name sprint renames, --fps doc drops its own fps
klad clip art.draw --rm run
klad timeline art.draw --origin 512,700                     # the canvas point a Godot export puts on the node (default the centre)
```

In `apply`: `{"op":"clip","add":"run","from":0,"to":11,"loop":true,"fps":24}`, `{"op":"clip","set":"run","name":"sprint","fps":null}`, `{"op":"clip","rm":"run"}`, `{"op":"timeline","origin":[512,700]}`. Frames inserted or removed on every layer move and stretch clips (an insert inside a clip lengthens it); on listed layers they leave clips alone. `klad frames` marks clips on the right (`[run` first frame, `run]` last) and lists them under the sheet. In the editor clips are bars on the strip above the frame numbers, and Return plays the clip under the playhead at its fps.

### Animation exports

```
klad export art.draw --sheet all [--scale 0.5] [--padding 2] [--max 4096] [--depth 16]
klad export art.draw --sheet run -o out/run.png
klad export art.draw --frames run                           # run-0000.png ... full size, one per frame, in art-run/
klad export art.draw --godot ../game/art/hero               # first time: the folder is kept in the document
klad export art.draw --godot                                # later: the kept folder (the editor's button does the same)
```

- `--sheet all` exports every clip (or the whole timeline as `default` when there are none), `--sheet <clip>` one, `--sheet 0-11` a range. The PNG holds each distinct picture once (holds, links and frames that render the same share one), trimmed to its pixels, in order of first appearance; beyond `--max` it goes to more pages (`art-sheet-2.png`, ...). The JSON is Aseprite's array layout: one entry per exposure with `duration` in ms at the clip's fps (a clip's durations add up to its length), `frame` (rect in the page), `spriteSourceSize` (the trim) and `sourceSize`, plus `frameTags` per clip (`repeat: "1"` when it plays once). Entries on later pages carry `image`.
- `--godot <folder>` writes `art.png` (and pages), `art.tres` (a `SpriteFrames`: an `AtlasTexture` per picture with its trim as margin, one animation per clip with its name, speed, loop and per-frame durations in frames) and `art.tscn` (an `AnimatedSprite2D`, autoplaying the first clip, offset so the origin sits on the node's position). Re-exports keep the uids of the `.tres`, `.tscn` and the imported PNG, so scenes using them keep working. `--store` replaces a kept folder; the kept path (`export.godot`) is relative to the folder holding the bundle.
- Exports print a pixel hash per page and of the JSON on stderr. The bounce (`samples/bounce/build.sh`) exports in about 0.25 s; 40 distinct 1024 frames to Godot in about 1.4 s warm, 2.3 s cold.

See the motion before reading single frames:

```
klad render art.draw --sheet all              # every frame in one grid: frame number, drawing ids, holds
klad render art.draw --sheet 0-11 --drawings  # one cell per drawing: frames that look the same share a cell ("4-5")
klad render art.draw --sheet run --cell 192   # a clip by name; --cell is the longest side of a cell, --columns n
klad render art.draw --frame 5 --overlay onion            # drawings before (red) and after (green) under each layer
klad render art.draw --frame 5 --onion 1,3 --onion-layers lines --onion-by frames --no-tint
klad render art.draw --sheet 0-7 --overlay onion --onion-loop 0-7   # onion in every cell, wrapping inside a loop
```

Cell labels read like `klad frames`: `c5` starts a drawing, `|c5` holds it, `c9>c1` shows c1 again through a link, `.` is blank, and a grey title with `hold` means the frame looks exactly like the one before. Onion skin is under the layer it belongs to, faded with distance (counted in drawings unless `--onion-by frames`); without `--onion-layers` every layer with more than one drawing shows its own. A sheet of 24 frames of a 2048 canvas takes about 0.3 s; onion in a sheet costs more (about 0.8 s for one layer, 2 s for four). Render prints a pixel hash on its second line, handy for comparing with the Mac.

## Traps

- `--sheet` renders whole frames: it refuses `--focus`, `--region` and `--scale` (size the cells with `--cell`). For a close-up of one frame use `--frame n --focus <id>`.

- Strokes and fills only go into paint layers; nodes only into vector layers.
- `klad fill` and `klad stroke` take the target layer by id; `--ref` also takes a name.
- A cel without `hold` lasts until the next cel, so a stroke or node at a later frame lands in that drawing unless the frame starts a cel (`klad cel new` first). A frame past the end of the timeline starts a cel and lengthens the timeline.
- Painting at a frame that shows a link fails from the CLI (paint into the source cel's frame instead); the editor paints into the source.
- `klad ls` on huge scenes lists 24 nodes per container; use `ls --json` or `show <group id>` for the rest.
- The editor rasterises vector layers; very large vector layers take a moment to refresh after an edit.
- `klad path` numbers points across all subpaths of the shape; re-read the list after `--add`, `--insert` or `--rm`.
- A layer created with `lockAlpha=true` takes no paint at all until something without the lock lands on it first; paint the flats, then lock.
- A clipped layer at the bottom of its list has no base and draws unclipped. Hiding a base hides its clipped layers.
- `klad export --layer` on a clipped layer draws it alone but still clipped to its base (without the base's own colour).
- Animation exports draw every visible layer, so a held background (a floor) is in every picture and trimming keeps its width. Hide it, or pass `--layer <character group>`, for a sprite.
- `--sheet` on `render` is a labelled look at the frames; `--sheet` on `export` is the game asset. They take the same specs.
- `klad inbetween --to` is the frame where the second key starts; a frame inside its exposure is refused with the right frame named.
- In-betweens keep the first key's mark ids, so after a loop in-between the ids are the last key's: look marks up by part (`klad strokes` shows each mark's part), not by an id from another key.
- `klad arcs` tracks a part on the first paint layer that has it; name the layer (`far:far leg`) when parts share names across layers (body is on lines, shade and rim).
- In-betweens of keys on a layer with other drawings between them are refused (they would be removed); `--force` replaces them.
- A closed outline drawn as one stroke (a leaf, an eye) is fine to in-between; its own start and end meeting is not a join.
- `ink`, `light`, `hatch` and `scatter` take shape ids, not layer ids; a group means every shape in it. `light`, `hatch`, `scatter` and `fill --shape` need closed outlines (an open path inks, but has no area).
- `klad light` adds fills, it does not replace earlier ones: run it once per set of shapes, or undo before running it again with other settings. So do `klad paint` and `klad hatch`.
- Gradient centres and ends are in the node's own coordinates: canvas px for a node without a transform. A radial centred on `[0,0]` under an ellipse at `cx=930` is transparent where the ellipse is; put the centre at the ellipse's centre, or draw the node around 0,0 and move it with `transform`.
- An edit that finds the document changed under it (the user saved while `klad` was computing, most likely during `inbetween` or `fill --like`) writes nothing, says "the document changed while this edit was being made", and exits with code 75. Run the same command again; it starts from his change.
