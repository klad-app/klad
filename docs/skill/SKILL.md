---
name: klad
license: MIT (LICENSE-MIT in the klad repository)
description: Make and edit 2D art with the klad CLI (`klad`) in a `.klad` bundle that the user can open in the editor at the same time. Use for any drawing, illustration, game asset, icon, background or sprite work in klad, for reading what the user drew or asked for (strokes, selection, pinned notes), and for scripting art generators that emit `klad apply` ops.
---

# klad

A `.klad` bundle is a folder: `doc.json` (layers, cels, vector nodes, palette), `strokes/<layer>/<cel>.strokes` (paint strokes and fills as data), `brushes/<id>.json` (every brush its strokes use) and `.cache/` (renders, history, notes, tiles). The `klad` CLI edits it; the user edits the same bundle in the editor (`klad open`, or the klad app). Both see each other's changes within a second. The user starts and opens drawings from the editor's start window (cmd N, cmd O, recent) and can copy a drawing's path from File (the menu bar) or the document menu at the bottom of the tool strip to hand it to you. To start one yourself: `klad new art.klad --width 1600 --height 1200 --background '#e9e4da' --sketch` (one paint layer, `l1`, and a vector layer named sketch on top, `l3`). If `klad` is not on the path, the user can run "install the klad command" from that menu (or `klad install` from the folder they unpacked); `klad skill` prints these instructions, matching the version installed. When klad itself fails (an error that starts "klad hit a bug"), tell the user and offer `klad report --drawing <bundle>`: it prints a report for an issue and zips the drawing to attach. `klad solo art.klad <layer>` hides every other layer, as an alt click on its eye does in the editor; it changes what the user sees, so use it when asked, and look at one layer alone with `klad export art.klad --layer <layer> -o one.png` instead. `klad samples` lists the sample drawings that come with klad, and `klad samples --copy lantern` copies one into the current folder to read, render or draw on: a quick way to see how a finished piece is layered.

This skill is the tool. The craft of drawing itself (planning a picture, light on form, values, edges, colour, materials, line, a painting method and a checklist to judge the result) is a second skill: `klad skill drawing`, the text of `docs/skill/drawing/SKILL.md`. Read it before you draw or paint anything, and before you critique a picture.

Working on klad itself: the repository's `CLAUDE.md`, `CONTRIBUTING.md` and `docs/DESIGN.md` are the source of truth. Build the CLI with `just compile` (`just build` also counts the version up, for the person making a build); the binary is `target/release/klad` on macOS and `target/linux/release/klad` on Linux.

## The loop

1. Look: `klad ls art.klad` (tree, top layer first, ids and bounds) and `klad render art.klad --scale 0.5 --overlay ids,bbox`. Read the PNG it prints.
2. Change: `klad add`, `klad set`, `klad rm`, `klad mv`, `klad stroke`, `klad fill`, `klad transform`, `klad lasso`, `klad path`, `klad palette`, or many at once with `klad apply` (JSON lines, one transaction).
3. Check: add `--render` (preview, longest side 1024) and `--overlay ids,bbox` to the edit, or `klad diff` (before and after, side by side), or `klad render --focus <id>` for a close-up.
4. Undo freely: `klad undo`, `klad redo`, `klad log`. History is shared with the editor.

Keep renders small unless detail matters: `--scale 0.5` or `--focus <id>` costs far fewer tokens than full size.

## What the user wants

- `klad notes art.klad`: notes the user pinned on the canvas, each with a `--region` render command. Do the work, then `klad notes art.klad --done <id>`; the editor shows the pin as done at once and lists it under done in the Notes pane, so mark a note done only when it is. `--clear-done` removes the done ones, when the user asks for a clean list.
- `klad selection art.klad`: the layer, shapes, path and strokes the user selected in the editor (strokes as `<cel>:s<id>`), with bounds and a `--focus` command.
- `klad strokes art.klad <layer> --json`: the user's strokes with points and pressure, to trace their shapes (a ridge line and tree positions from a pen sketch, for example).

## Vector nodes

Layers: `paint` (strokes), `vector` (nodes), `group` (children). Add a vector layer, then nodes into it:

```
klad add art.klad layer vector name=sky
klad add art.klad rect --in l1 x=0 y=0 w=2048 h=900 'fill={"linear":{"from":[0,0],"to":[0,900],"stops":[[0,"#20304f"],[1,"#d98b5f"]]}}'
klad set art.klad n3 transform.rotate=-8 opacity=0.8 blur=4
```

- Types: `rect` (x, y, w, h, radius), `ellipse` (cx, cy, rx, ry), `path` (SVG `d`), `group` (children).
- Every node: `fill`, `stroke` (`paint`, `width`, `cap`, `join`, `dash`), `transform` (`x`, `y`, `rotate`, `sx`, `sy`, `pivot`), `opacity`, `blur` (soft edges, glows, haze, shadows), `clip` (SVG path that clips the node and its children), `visible`, `name`.
- Paint: `"#rrggbb"`, `"#rrggbbaa"`, `"none"`, `{"linear":{"from","to","stops"}}`, `{"radial":{"center","radius","stops"}}`.
- As in SVG, a node without `fill` is black; use `fill=none` for outlines only.
- Unknown keys fail with the list of valid ones. Structure (`id`, `type`, `kind`, `children`, `cels`, `nodes`) only changes through add, rm and mv.
- Ids are case sensitive; overlays print them exactly.
- Lining shapes up: `klad align art.klad n3 n4 n7 --y middle` puts them on one line by their bounds on the canvas (`--x left|centre|right`, `--y top|middle|bottom`), `--spread x` (or `y`) makes the gaps between them equal with the outer two staying, and `--gap 24` sets the gap instead. `--to canvas` lines up on the canvas (`klad align art.klad title --x centre --to canvas`), `--to n3` on a shape that stays put. One history step. The user does the same with V, shift click and the bar under the shapes.

## Painting

```
klad stroke art.klad l4 --brush ink --size 14 --color '#1b1b1b' --path 'M100 400 C 300 200 500 600 700 400' --pressure 0.1,1,0.05
klad stroke art.klad l4 --brush ink --size 5 --taper 8,20 --path 'M100 100 L140 220'
```

- `klad brushes art.klad` lists the brushes: the ones the bundle embeds and the library (`ink` inking pen, `pencil`, `marker`, `airbrush`, `soft`, `chalk`, `eraser`, and for painting `flat-bristle`, `round-bristle`, `sponge`, `fog`, `leaves`, `grass`, `splatter`, `wet`, `smudge`, `blender`, `dry-brush`, `two-tone`); the tip column says `+dual`, `+texture`, `+colour`, `+wet`, `+smudge`, `+blend`, `+body`, `+feather` or `+fringe`. `--show <id>` prints one as JSON. A bundle keeps every brush it uses, so `ink` in an older bundle can be the older ink; `--brush` takes any id from that list.
- Painting brushes: `wet` picks up the colour under it and mixes it in, `smudge` pulls paint along, `blender` softens what it crosses; all three read only the layer they paint on (paint the colours to mix on one layer). `leaves`, `grass` and `splatter` scatter their picture tips with jittered colour (`leaves` picks one of several leaf shapes per dab: `leaf`, `leaf-oval`, `leaf-lance`, `leaf-twig`; a tip takes more pictures as `{"image":{"image":"leaf","more":["leaf-oval","leaf-lobed"]}}`); `two-tone` paints `--second` at a light press and `--color` at a full one. `--second '#e0a040'` (`"second"` in `apply`) is the second colour for brushes with colour dynamics.
- New brushes: write the JSON (copy one from `--show`; keys `tip` (`round`, `chalk` or `{"image":{"image":"name"}}`), `dual`, `grain` (`paper` or `image`, `strength`, `scale`, `contrast`, `pressure`), `color` (`hue`, `saturation`, `value`, `strokeHue`, ..., `pressure`, `along`), `paint` (`wet`, `smudge`, `blend`), `feather` (0 to 1, a round tip's edge fades like an airbrush; a soft eraser is the eraser with `"feather":0.8`), `fringe` (0 to 1, the edge frays into streaks along the stroke, like a dry brush)) and `klad brushes art.klad --set my-oil brush.json` (`-` reads stdin). Pictures for tips and textures: `klad brushes art.klad --picture bark bark.png` (dark is paint, or the alpha). Ids and pictures are fixed once in the bundle, since strokes keep their brush.
- `--taper in,out` thins the first and last px of the line to a point whatever the pressure; without it the brush's taper applies (`ink` is `10,24`), `--taper 0,0` gives round ends. Short hatching wants about `8,20`, long contours `30,80`.
- `--pressure` spreads values evenly along the stroke; several subpaths in `--path` make several strokes. `--points 'x,y,p x,y,p'` gives explicit samples.
- In `apply`: `{"op":"stroke","layer":"l4","brush":"ink","size":14,"color":"#1b1b1b","path":"M...","pressure":[0.1,1,0.05],"taper":[12,30]}`.
- Agent strokes are marked `agent` in `klad strokes`; the user's are `human`.

## Editing strokes and fills

Every stroke and fill has an address `<cel>:s<id>` (fills share the id counter with strokes). `klad strokes art.klad <layer>` lists them with the cel id on top. In a clone or worktree of a drawing, new ids end in that copy's two-letter tag (`l48qa`, `c12:s349qa`, shown by `klad info`); use them as printed, and the `address` field in JSON. If a drawing refuses to open with "id ... is used twice" (after a git merge), run `klad repair art.klad`: it renames every later one and says what it renamed.

```
klad set art.klad c4:s12 color=#c84a3a size=6 brush=pencil
klad set art.klad c4:s7 color=skin opacity=0.8
klad recolour art.klad c4:s12 c4:s13 c4:s40 --to '#e3962f'   # several at once, each keeps its shade
klad rm art.klad c4:s12 c4:s13
klad transform art.klad c4:s3 c4:s5 --move 40,-10 --rotate 15 --scale 1.2
klad transform art.klad --layer figure --move -120,0           # a whole layer or group: every drawing, its shapes, bones and poses
klad show art.klad c4:s12
klad render art.klad --focus c4:s12
```

- Strokes take `color`, `size`, `opacity`, `flow`, `brush`, `taper` (`[in,out]` or `null`) and `path`; fills take `color` and `opacity`. The stroke keeps its path, pressure and seed, so a recoloured stroke is the same mark in a new colour.
- Bending a stroke: `klad strokes --json` gives each stroke a fitted `path` (a few points); edit it and `klad reshape art.klad c4:s12 --path 'M...'` (or `set` with `path`). Every sample keeps its pressure, tilt and timing at the same fraction of the length, so the line keeps its weight and taper; erasers, smudges and blenders attached to it bend along (one also on other marks splits). The user does the same with A in the editor.
- `transform` moves, rotates (degrees, clockwise) and scales about `--pivot` (default: the middle of the marks' bounds). Stroke sizes and tapers scale with them; fills move by whole pixels and are resampled when turned or scaled. Ids in several cels need a `--pivot`.
- In `apply`: `{"op":"set","id":"c4:s12","color":"#c84a3a"}`, `{"op":"rm","ids":["c4:s12"]}`, `{"op":"transform","ids":["c4:s3","c4:s5"],"x":40,"y":-10,"rotate":15,"sx":1.2,"sy":1.2}`.
- Edits redraw only the tiles the marks touch, from the nearest checkpoint, so they are cheap on large cels.
- Erasers, smudges and blenders attach to the marks they were drawn over (earlier marks in the drawing with paint under them; reaching a smudge's smear takes the smudge's marks). `klad strokes --json` gives a modifier `on` (the addresses it is attached to), a mark `attached` (the modifiers on it), and `hidden: true` for a mark its attached erasers wipe out completely. An attached eraser erases only its marks, so moved with them it leaves what they land on whole.
- Edits follow attachments: `transform` and `lasso` move the modifiers whose marks all move, and split one whose marks move only in part (a copy for the moved marks lands right after it); `rm` removes a modifier left with no marks; `copy` takes every modifier attached to a copied mark. Addressing a modifier itself moves it on its own, and it attaches again to what it lands on. `klad detach art.klad c4:s40` (a modifier, or a mark to free everything attached to it; `{"op":"detach","ids":[...]}`) leaves it where it is, erasing or smudging whatever is under it again.

## Lasso

```
klad lasso art.klad l4 --polygon '600,600 1400,560 1450,1300 640,1380'
klad lasso art.klad l4 --polygon '600,600 1400,560 1450,1300 640,1380' --cut --move 40,-25
klad lasso art.klad l4 --polygon '...' --cut --rotate 12 --scale 1.1
klad lasso art.klad l4 --polygon '...' --flip h
klad lasso art.klad l4 --polygon '...' --cut --rm
```

- Without a move, turn, scale, flip or `--rm` it lists the strokes and fills inside the polygon and those crossing its edge.
- The lasso picks plain marks; erasers, smudges and blenders attached to them follow (see above).
- Without `--cut` only whole marks move (strokes whose points are all inside, fills whose pixels are). With `--cut` each mark crossing the edge is split: the outside part stays put, the inside part becomes a new mark that moves with the rest, so nothing outside the lasso changes. The output names the moved marks (`lasso on l4 moved c2:s10 ...`); they stay strokes and can be edited again.
- `klad transform` takes `--flip h` or `--flip v` too. In `transform` and `lasso`, `--scale` scales the marks; their previews use the default size.
- In `apply`: `{"op":"lasso","layer":"l4","polygon":[[600,600],[1400,560],[1450,1300]],"cut":true,"x":40,"y":-25}` (also `rotate`, `sx`, `sy`, `scale`, `pivot`, or `"rm":true`).

## Pixel art

```
klad new sprite.klad --pixel 32x32                      a pixel drawing: one pixel layer, l1
klad add art.klad layer pixel name=sprite               a pixel layer in any drawing
klad palette sprite.klad --add '#22223b' --name ink     name the colours first
klad pixel sprite.klad l1 --grid - <<'GRID'             a sprite as text: a legend, a blank line, the rows
i = ink
b = body

..iiii..
.ibbbbi.
.ibbbbi.
..iiii..
GRID
klad pixels sprite.klad l1                              read the layer back as text, with numbered rows and columns
klad render sprite.klad --overlay grid                  enlarged, with numbered pixel lines
klad pixel sprite.klad l1 --color ink --cells '3-12,2 3,3-8'      single pixels, runs and blocks
klad pixel sprite.klad l1 --color ink --path 'M3.5 20.5 C 10 4 22 4 28.5 20.5'   a curve, one pixel thin
klad pixel sprite.klad l1 --color ink --ellipse 4,4,16,12            shapes: --line, --rect, --ellipse (--filled)
klad pixel sprite.klad l1 --color body --fill 12,10                  flood fill from a pixel
klad pixel sprite.klad l1 --erase --cells '7,5 8,5'
klad export sprite.klad --scale 8 -o sprite@8x.png      square pixels, to keep
```

- A pixel layer is a grid with no soft edges, and one pixel is one canvas pixel: a 32x32 sprite is a 32x32 drawing. Size the drawing in pixels of the art, not of the screen; `klad ls` shows the layer as `pixel`.
- Draw a sprite as a grid of characters: `klad pixel <bundle> <layer> --grid <file>` (`-` reads stdin). Legend lines come first (one character, `=`, then a palette name, a colour, `erase` or `empty`), then a blank line, then one row of characters per row of pixels. `.` is empty (it leaves what is there) unless the legend says otherwise; a character the legend does not name is an error that says where. `--at x,y` puts the grid's top left pixel elsewhere, so a part (an eye, a hand) can be a small grid of its own. Each character becomes one mark bound to its colour, so a whole sprite is a handful of marks you can recolour or move.
- Read before you change: `klad pixels <bundle> <layer>` prints what the layer shows as the same kind of grid, with a legend (each colour's character, name, hex and id; a colour keeps its character as long as the palette's order stays) and the pixel numbers along the top and the left. `--plain` prints only the legend and rows, in the form `--grid` reads, so a sprite can be read, edited as text and written to an empty layer or frame; `--json` gives `rows` and `legend`. Reading the text is exact; the picture is for judging how it looks.
- For a few pixels use `--cells`: `x,y` for one pixel, `x0-x1,y` for a run along a row, `x,y0-y1` for a run down a column and `x0-x1,y0-y1` for a block, separated by spaces. Pixels off the canvas are dropped.
- `--path` draws SVG path data as a line of pixels, for outlines and curves that would be tedious as cells. A pixel is the square from `x,y` to `x+1,y+1`, so aim at pixel middles (`3.5`, not `3`). Straight segments keep the corners they are given; curves come out one pixel thin with no doubled corners. `--size 2` (1 to 64) makes the pencil a square of that many pixels, for `--cells` and `--path` alike. In `apply`: `"path":"M3.5 3.5 L9.5 3.5"` in place of `cells`, and `"size":2`.
- Shapes, each one mark: `--line x0,y0,x1,y1` is a straight line between two pixels; `--rect x,y,w,h` and `--ellipse x,y,w,h` are the outline of a rectangle, or of the ellipse that fits that box of pixels (`x,y` its top left pixel, `w,h` its size in pixels), and `--filled` takes all of it; `--size` thickens a line or outline. An ellipse outline plus a fill is the quick way to a round body.
- `--fill x,y` is a flood fill: every pixel joined to that one that has exactly its colour, or is empty like it, with no tolerance. It spreads to the four neighbours; `--diagonal` also crosses corners (a one-pixel diagonal line then no longer stops it). It reads this layer; `--ref all` reads every visible pixel layer and `--ref <layer>` another one, so flat colour can go on a layer under the outlines: `klad pixel sprite.klad colour --color body --fill 12,10 --ref lines`. Check the outline is closed first (`klad pixels`): through a gap the fill takes the whole background.
- Shading with ramps: `--shade 1` (or `-1`) draws no colour of its own; every pixel it covers takes the next lighter (or darker) colour of its own ramp, so one call shades a form made of several materials: `klad pixel sprite.klad l1 --shade -1 --rect 4,12,16,4 --filled` darkens the lower part of whatever is there, and pixels in no ramp or at its end stay. It works with any source (cells, a path, a shape, a fill). So: block the form in each ramp's middle colour, then shade down on the side away from the light and up where the light hits.
- `--dither 50` draws only that share of the pixels, in an even pattern fixed to the canvas (50 a checkerboard, 25 one pixel in four, 75 three in four), and `--second <colour>` gives the others a second colour: a band of dither between two ramp steps softens the step. Use it sparingly and only at sizes where the pattern reads as a tone.
- `--outline` draws a one-pixel outline around what the layer shows, in `--color`, as one mark: every empty pixel that touches a drawn one (`--diagonal` also takes the corners, for a heavier line; `--inside` recolours the drawing's own edge pixels instead; `--ref <layer>` outlines another layer's pixels, to keep the outline on a layer of its own).
- Symmetry: `--mirror-x 16` also draws the mirror image across the upright line at x 16 (a whole number is the line between two pixels, so 16 mirrors a 32 px sprite about its middle; `15.5` runs through column 15, for sprites with a middle column); `--mirror-y` mirrors across a level line, and both together give four copies. It works with every source, so draw half a face or half a tree and get the whole. In `apply`: `"mirrorX":16`.
- Tiles: on a canvas that wraps (`klad new tile.klad --pixel 16 --wrap`, or `klad canvas tile.klad --wrap on`), pixels drawn past an edge come back in on the other side, and fills and outlines go round the edges, so a shape drawn over a corner shows in all four. `klad render tile.klad --repeat 3` shows the tile three times across and down: look there for seams, lines that do not meet and a pattern that reads as a grid. Moving marks with `klad transform` or `klad lasso` does not wrap.
- `--color` is a palette name, a palette index or `#rrggbb`. Every pixel mark is bound to a palette entry: a colour the palette lacks is added to it, so name the colours first (`klad palette sprite.klad --add '#f0c8a0' --name skin`) and draw with the names. `klad palette` lists the entries with their ids.
- The palette is the drawing's colours, and changing one changes the picture: `klad palette sprite.klad --recolour skin --to '#e8b796'` (a name, a colour, an index or an id) recolours every pixel mark drawn with it, on every frame. So fix a colour in the palette, not mark by mark. `klad palette sprite.klad --load pico-8` adds a palette that comes with klad (`klad palettes` lists them: pico-8, resurrect-64, four-greens, 1-bit) or a file (`.hex`, one colour per line, or a GIMP `.gpl`); colours the palette holds are not added twice. A ramp is a row of colours for one material, dark to light: `klad palette sprite.klad --ramp wood --base '#8b5a2b' --steps 5` makes `wood-1` (dark, cooler) to `wood-5` (light, warmer) around the base, and `--ramp skin --of skin-dark,skin,skin-light` groups colours it has. Shade a form with the steps of its ramp, not with new colours.
- Editing an area: `klad lasso sprite.klad l1 --box x,y,w,h --cut --move 2,0` takes the pixels in that box (its top left pixel and size; `--polygon` for another outline) and moves them by whole pixels; `--flip h`, `--rotate 90`, `--scale 2` and `--rm` work the same way. With `--cut` the box takes pixels: marks it crosses are split between whole pixels and the inside halves become marks of their own. Without it only marks that lie whole inside are taken. Quarter turns, flips and whole-number scales keep every pixel; other angles and scales stay hard but look jagged.
- Editing by colour: `klad strokes sprite.klad l1 --entry skin` lists only the marks of one palette colour (with `--json`, their addresses), to move, remove or recolour them together. `klad recolour sprite.klad c2:s4 c2:s9 --to shadow` gives pixel marks exactly that palette colour (no shades are kept, unlike on paint layers); `--step 1` or `--step -1` moves each mark along its colour's ramp instead; `klad recolour sprite.klad --from skin --to skin-dark` replaces a colour, rebinding every mark drawn in it (`--layer`, `--frame` to limit it). To change what a colour looks like everywhere, change the swatch (`klad palette --recolour`); to move marks to another colour, replace.
- Animation is frame by frame: a pixel layer holds a drawing per frame like a paint layer (`klad add sprite.klad cel --in l1 frame=2`, then `klad pixel ... --frame 2`; `klad duplicate`, links, holds, clips and `--overlay onion` all work). To make the next frame, read the last one (`klad pixels sprite.klad l1 --frame 0 --plain`), change the text and write it to the new frame. Keying a pixel layer (`klad key sprite.klad l1 --frame 4 x=6`) moves it by whole pixels: positions in between are rounded, so a slow move steps a pixel at a time. `klad inbetween`, `klad pose`, `klad weights` and `klad attach` refuse pixel layers and say why: they move points and bend strokes, which would take pixels off the grid.
- Palette variants are other colours for the same sprite (teams, seasons, a hurt flash, day and night): `klad palette sprite.klad --new-variant blue` makes one and shows it, and every colour you change while it is shown (`--recolour team --to '#2a6fdb'`, `--add` with an existing name) is blue's own; the base palette keeps its colours, and a variant follows the base for every colour it has not changed. `--variant base` (or `--variant blue`) shows another one: the pixels stay the same marks, only their colours change, as one undo step. `klad palette sprite.klad` lists what is shown and each variant's own colours; `--rename-variant blue --to navy` and `--rm-variant navy` do what they say. Draw in the base palette and bind the parts that will change to their own palette colours (a `team` colour and its shades, not the same red that is also used for the mouth), or a variant changes more than you meant. Export without switching: `klad export sprite.klad --variant blue` renders that variant whatever is shown (file names get `-blue` after the drawing's name), and `--variants` exports the base and every variant in one go; both work with `--sheet`, `--frames`, `--gif` and `--godot`. Variants recolour pixel marks only: strokes on paint layers and vector shapes store their own colours.
- Exports stay sharp: a pixel drawing exports at whole-number scales only (`--scale 1` to 64, at most 16384 px a side), enlarged pixel for pixel, never smoothed; `--scale 2.5` is refused with the reason. That holds for `klad export` of a frame, `--sheet`, `--frames`, `--gif` and `--godot` (sheet cells and the Godot offset are scaled with it; the Godot scenes set `texture_filter` to nearest so the sprite stays sharp in the game). To show a sprite to someone, `klad export sprite.klad --gif --scale 8`; in a game, export at `--scale 1` and let the engine enlarge it.
- Pictures to pixels. `klad pixelize sprite.klad --image ref.png --to l1` puts a PNG or JPEG on a pixel layer: every pixel takes the nearest colour of the drawing's palette (so it lands in the colours the sprite has; `--colors N` makes N colours from the picture and adds them instead), `--size 48` shrinks it to 48 px on its longest side first (each pixel the average of the area it covers; a picture enlarged by a whole number comes back exact; pixels less than half solid stay empty), `--at x,y` places its top left, `--frame` picks the drawing. `klad import ref.png [--size N] [--colors N]` makes a new pixel drawing from a picture, with the picture's own colours as its palette (reduced to 32 when it has more than 256). `klad pixelize hero.klad --size 64 [--layer body] [--colors 12] [-o out.klad]` makes a new pixel drawing (`hero-pixel.klad`) of a painted or vector layer or of the whole picture, every frame with one set of colours, frames that come out the same held, clips kept; the drawing itself is not changed. All three give marks bound to palette colours, one per colour, so the result is edited like anything else here. Treat it as a start, not a finish: shrinking blurs edges into in-between colours and breaks one-pixel outlines, so look at it at real size, then redraw the outline, and cut stray colours with `klad recolour --from a --to b`.
- Aseprite files: `klad import hero.aseprite` (or `.ase`; `-o` names the drawing) makes `hero.klad` next to the file: image layers as pixel layers with their name, visibility, opacity and blend mode, groups as groups, every cel as a drawing on its frame (linked cels stay links), the palette with its names, tags as clips. Frame times become whole frames at the slowest speed that fits (100 ms frames are 10 fps; a 300 ms frame holds for three). Indexed, grey and RGBA files all open, every pixel bound to its palette colour. It says what it leaves out: tilemap layers, tilesets, slices, tag directions other than forwards, blend modes klad lacks. `klad export hero.klad --aseprite` writes the pixel layers and groups back (RGBA; holds and links as linked cels, clips as tags, a pixel layer's keyed moves and opacity baked into its cels; `--variant` and `--variants` work); paint, vector and other layers are left out and named. Use it to hand work to someone who uses Aseprite, or to take over a sprite made there.
- Each mark, `<cel>:s<id>`, stays editable: `klad set sprite.klad c2:s1 color=shadow` recolours it (and binds it to that entry), `klad transform ... --move 2,0` moves it by whole pixels, `--flip h` and `--rotate 90` keep it hard, `klad rm` removes it. `klad strokes sprite.klad l1` lists the marks with their entry and cell count; `--json` gives each one's `cells`.
- Later marks cover earlier ones; `--erase` (or `erase` in a grid's legend) makes a mark that clears what was drawn before it at those pixels (remove that mark and the pixels come back).
- Looking: `klad render` and the `--render` preview of an edit enlarge a pixel drawing by a whole number to about 512 px, so every pixel is a square you can see (`--scale 1` gives the real size, which is how the player sees it: check there too). `--overlay grid` draws a line between pixels and numbers them along the top and the left, so a pixel's `x,y` can be read off the picture: the number is on the line where that column or row starts. `klad export --scale 8` (whole numbers) writes the enlarged PNG to keep.
- In `apply`: `{"op":"pixel","layer":"l1","color":"skin","cells":[[4,5],[5,5]]}`, with `"erase":true`, `"line":[x0,y0,x1,y1]`, `"rect":[x,y,w,h]` or `"ellipse":[x,y,w,h]` (and `"filled":true`), `"fill":[x,y]` (and `"diagonal"`, `"ref"`), `"outline":true` (and `"inside"`), `"shade":-1` in place of a colour, `"dither":50` with `"second"`, or a grid: `{"op":"pixel","layer":"l1","rows":["..ii..",".ibbi."],"legend":{"i":"ink","b":"body"},"at":[0,0]}`. Brush strokes (`klad stroke`, `ink`, `paint`, ...) and `klad fill` are refused on a pixel layer: fill it with `klad pixel --fill`.

## Masks, clipping and lock alpha

```
klad set art.klad l5 clip=true blend=multiply
klad set art.klad l3 lockAlpha=true
klad add art.klad mask --in l5
klad stroke art.klad l9 --brush airbrush --size 180 --path 'M600 600 L 640 700'
klad set art.klad l9 visible=false
klad rm art.klad l9
```

- `clip=true` shows a layer only where the nearest unclipped layer below it (its base) has paint. Clipped layers blend onto the base with their own blend; the group takes the base's opacity and blend. This is how shading stays inside flat colour: flats on one layer, a multiply shade layer and a rim light layer above it, both clipped.
- `klad add art.klad mask --in <layer>` gives a layer a mask and prints its id. The mask is a paint layer of its own (`klad ls` shows it under its layer): strokes and fills on it hide the layer, the eraser brings it back, an empty mask hides nothing. `hide=true` starts it hiding everything. `visible=false` on the mask turns it off; `klad rm` on its id removes it.
- `lockAlpha=true` on a paint layer: new strokes and fills only land where the layer already has paint, which recolours line art (a big soft stroke over lines tints only the lines). It is recorded per stroke, so set it back to false when done, or the user's next pen strokes on that layer only land on existing paint. The eraser does nothing on a lock-alpha layer.
- In `apply`: `{"op":"add","mask":{"name":"light"},"in":"l5"}`, `{"op":"set","id":"l5","clip":true}`.

## Light, effects and adjustments

```
klad effect art.klad l3 glow radius=24 color=#ffd080
klad effect art.klad l3 outline width=4 color=#101418
klad effect art.klad l3              # lists them; --rm 1 removes the second, --clear all
klad add art.klad layer adjust curves 'points=[[0,0],[0.5,0.3],[1,0.85]]' 'blue=[[0,0.12],[1,1]]' name=dusk
klad add art.klad rays --in l7 x=430 y=0 angle=95 spread=40 count=9 length=600 start=120 fill=#fff0c0c0 blur=6
klad set art.klad l5 blend=soft-light
```

- Effects are settings on a layer (or group), applied in order when it composes and always editable: `blur` (radius), `glow` (radius, strength, threshold: 0 glows from every pixel, about 0.7 only from the bright ones, as bloom; color tints it; blend: screen by default, add for hard light, normal lays it like paint), `outline` (width, color, inside=true), `shadow` (x, y, radius, color with alpha), `noise` (amount, size, seed, mono=false for coloured grain), `displace` (amount, size, seed; by=<layer> pushes with that layer's red across and green down, grey stays put; a hidden layer works as the map), `relief` (strength, light=x,y as in `paint --light`, highlight, shade, softness: lights the paint's thickness, so ridges and stroke edges facing the light catch it and the others darken; only strokes from brushes with `body` have thickness). In `apply` or `set`: `effects=[{"glow":{"radius":24}},{"outline":{"width":3}}]`.
- Bloom: put everything that should bloom in a group and give the group a glow with a threshold and blend add. A light layer is a paint layer with `blend=add` and a soft glow (`'effects=[{"glow":{"radius":24,"strength":0.8}}]'`): paint light into it with `soft` or `airbrush`.
- Blend modes: normal, multiply, screen, overlay, add, color-dodge, soft-light, hard-light, lighten, darken, color, luminosity.
- Adjustment layers change what is under them in their group (or, with `clip=true`, only their base): `hsl` (hue in degrees, saturation and lightness -1..1), `levels` (black, white, gamma, low, high), `curves` (points, and red, green, blue applied first; x is the input brightness, y the output), `balance` (shadows, midtones, highlights as [cyan-red, magenta-green, yellow-blue] in -1..1, keepLight), `gradient-map` (stops by brightness), `fill` (paint: a colour or gradient, drawn with the layer's blend). Their opacity, blend and mask apply. To grade a scene, put one curves layer on top; to grade one part, put the part and the adjustment in a group.
- Rays are a vector node: an origin, a direction (`angle` in degrees, 90 is down), `spread`, `count`, `length`, a gap `start` px wide they come through, `falloff`, `noise` and `seed`. The node's `fill` colours them and `blur` softens them; a vector layer of rays on `screen` or `add` reads as light through trees.
- `klad ls` shows `fx: glow outline` on layers with effects and the type of an adjustment layer; `klad show <layer>` lists their settings.

## Depth, pictures, generators and text

```
klad set art.klad g2 depth=2.5                       a plane (a layer at the top of the stack): 0 is near
klad atmosphere art.klad color=#8fa3b8 top=#d3dde7 density=0.55 blur=1.2 desaturate=0.25
klad add art.klad layer image src=photo.jpg reference=true
klad add art.klad repeat --in l4 count=8 'step=[60,0]' 'children=[{"type":"rect","x":0,"y":400,"w":12,"h":80,"fill":"#3a2a1a"}]'
klad add art.klad scatter --in l4 x=0 y=700 w=1600 h=150 count=40 seed=7 'size=[0.6,1.6]' 'turn=[-30,30]' 'children=[{"type":"ellipse","cx":0,"cy":0,"rx":20,"ry":11,"fill":"#3a4450"}]'
klad add art.klad mirror --in l4 x=800 angle=90 'children=[{"type":"path","d":"M560 900 L560 560 L760 410 Z","fill":"#0d1117"}]'
klad fonts                                           the fonts that come with klad, by group, with their styles
klad fonts --search slab                             every open font on Google Fonts that matches; --get <family> downloads one
klad add art.klad text --in l4 'text=THE CROOKED INN' 'font=Cinzel Bold' size=46 x=340 y=96 align=center tracking=4 fill=#f3d9a4
klad set art.klad t3 'font=Pirata One'
klad font art.klad ~/fonts/Signage.ttf               a font file of your own
```

- Depth: give each top-level group (plane) a `depth`; with an atmosphere every plane fades towards the fog (and the `top` colour higher up), blurs and loses colour, more the deeper it is. Near planes keep depth 0. The user previews the parallax by holding D. `haze` is also an effect of its own (`klad effect art.klad l3 haze amount=0.4 color=#9aa8b8`).
- References: `reference=true` on any layer keeps it visible in the editor but out of `klad render`, `klad hash` and `klad export` (add `--references` to see it). Pictures to trace over go there.
- Pictures: `layer image src=<PNG or JPEG>` copies the file into the bundle (`images/<hash>.png`) and places it on a new vector layer, fitted inside the canvas; `x=`, `y=`, `w=`, `h=` place it. The node is `{"type":"image",...}`, moved and scaled like any shape.
- Generators keep their settings: `repeat` (count, step, turn and pivot for an arc, scale, columns and row for a grid), `scatter` (region x, y, w, h, count, seed, size and turn ranges, `avoid`: SVG paths no copy lands in), `mirror` (x, y, angle of the axis). Children are drawn once per copy; for scatter draw the child around 0,0. `klad add <bundle> <shape> --in <generator id>` adds more children.
- Text: `font=` takes a family that comes with klad (`klad fonts`: sans, serif, mono, display, handwriting and pixel families such as Inter, Cinzel, Bangers, Caveat, Press Start 2P), a family and style (`"Barlow Bold"`, `"Inter Black"`), or a font file already in the bundle. The file is copied into the bundle's `fonts/` folder with its licence, so the drawing renders the same on every machine; the node then names the file (`Cinzel.ttf`). A family klad does not ship but Google Fonts has is downloaded first (`klad fonts --search`, `--get`). Families that come in every weight take `weight=` (100 to 900, 400 regular, 700 bold); `"Inter Bold"` sets it. Text is shaped like a browser does it (kerning, ligatures, accents). `text` takes new lines (`$'line one\nline two'` in bash), `x` is the left, centre or right of each line (`align`), `y` the top of the first line; `tracking` is extra px between letters, `leading` the line height as a multiple of the size; fill and `stroke` colour and outline it, and a gradient fill gilds it.
- Lettering on a prop (a sign, a label): put the text on a vector layer clipped to the board's paint layer so it never leaves the board, give that layer a small `shadow` effect for depth, and lay a few faint grain strokes over it on a multiply layer clipped to the same base so the letters sit in the surface (`samples/sign`). The user types text in the editor with T and picks fonts from a list drawn in each font.

## Path points

```
klad path art.klad n7
klad path art.klad n7 --move 3 412,230
klad path art.klad n7 --nudge 3,4,5 0,-20
klad path art.klad n7 --handle 3 out 430,210
klad path art.klad n7 --add 418,236
klad path art.klad n7 --rm 4,5 --corner 2 --smooth 6
klad path art.klad n7 --simplify 1.5
```

- Without flags it lists the points (anchors) of a path, rect or ellipse, numbered across subpaths, with handles and corner/smooth/curve. Coordinates are the shape's own (as in its `d`, before its transform). After an edit it lists them again; numbers shift after inserts and removals.
- Any point edit turns a rect or ellipse into a path that looks the same. `--simplify <px>` refits a dense polyline (traced or generated) as a few smooth points within that distance: do it before reshaping something like a ridge line.
- A smooth point keeps its handles in line when one moves (`"free":true` breaks it). Pivots of transformed shapes and their parent groups are pinned first, so editing points never shifts the rest of the shape.
- In `apply`: `{"op":"path","id":"n7","move":3,"to":[412,230]}`, `"move":[3,4],"by":[0,-20]`, `"handle":3,"out":[430,210]`, `"insert":3,"t":0.5`, `"add":[418,236]`, `"rm":[4,5]`, `"corner":2`, `"smooth":6`, `"simplify":1.5`, or no key to only convert.

## Colour and fills

```
klad palette art.klad --add '#f0c8a0' --name skin
klad fill art.klad l2 --at 820,640 --ref lines --gap 4 --color skin
klad fill art.klad l2 --polygon '100,100 400,120 260,380' --color '#20304a'
klad refill art.klad l2
```

- The palette lives in `doc.json`. Any colour argument takes `#rrggbb`, `#rrggbbaa` or a palette name. `klad palette art.klad` lists it; `--add` with an existing `--name` changes that colour; `--rm` takes a name, a colour or an index.
- A flood fill starts at `--at` and spreads over pixels within `--tolerance` (0 to 1, default 0.1) of the seed pixel in the reference: `this` layer (default), `all` (everything visible, background included) or a layer id or name. Put flat colour on a layer under the lines and use `--ref <lines layer>`; a fill on the line layer itself covers the inside edge of the lines.
- `--gap` closes openings in the lines up to that many px wide (inked shapes with small gaps); `--expand` (default 2) tucks the fill under the lines so no paper shows between line and colour.
- Fills are records next to the strokes, with their coverage stored, so they render the same later. When the lines change, `klad refill <bundle> <layer>` recomputes every flood fill of that cel (`--id` for one).
- In `apply`: `{"op":"fill","layer":"l2","at":[820,640],"ref":"lines","gap":4,"color":"skin"}`, `{"op":"palette","add":"#f0c8a0","name":"skin"}`. Strokes and fills in one batch see each other in order, so ink then fill in one `apply`.

## Drawing from shapes

Describe a piece instead of programming it: block the subject in as vector shapes on a hidden sketch layer (rects, ellipses, paths, groups; give them ids so later ops can name them), then let these commands draw on paint layers. Everything they make is ordinary strokes and fills, so `klad set`, `klad transform` and the user's pen edit it afterwards.

```bash
klad add art.klad layer vector name=sketch visible=false
echo '{"op":"add","node":{"id":"body","type":"ellipse","cx":400,"cy":420,"rx":160,"ry":120},"in":"<sketch id>","frame":0}
{"op":"add","node":{"id":"eye","type":"ellipse","cx":450,"cy":380,"rx":18,"ry":18},"in":"<sketch id>","frame":0}' | klad apply art.klad
klad fill art.klad flats --shape body --color skin          # exactly the shape's area, grown 2 px under the ink
klad light art.klad --from body --light -1,-1 --shade shade --rim rim --depth 20   # shade away from the light, a rim towards it
klad ink art.klad lines --from body,eye --weight 7 --light -1,-1                   # tapered strokes, weight from rules
klad hatch art.klad shade --in body --angle 30 --spacing 8 --length 10-24 --color shadow
klad scatter art.klad lines --in ground --mark "M0 0 Q 3 -22 8 -40" --count 20 --turn -25,25
klad paint art.klad paint --in body --color skin --light -1,-1 --weight 12                      # a round form, painted from a light model
klad paint art.klad paint --in pillar --form cylinder --axis 90 --lamp 200,600,150 --color '#c9b8a0'
klad foliage art.klad canopy --in clump1 --color '#4d7a3b' --light -0.6,-0.8 --leaf 16 --mass crown   # a clump built from lit leaves
klad grass art.klad field --in field --color '#6f8a3c' --light -0.9,-0.45 --blade 30 --far 0.22 --wind 12  # blades lit by rolling ground
klad prepare art.klad                                                              # buffer every drawing before the user opens it
```

- Painting a form: after the flats, `klad paint` paints each shape as a form. `--form round` (default) bulges any closed shape like a pillow, its height growing with the distance to the outline; `flat` is a plane facing `--facing x,y,z` (z toward the viewer, y down); `cylinder` is round across `--axis` degrees. The light model is a key light from `--light x,y` at `--elevation` (or `--lamp x,y,height` in canvas px: the shape nearest the lamp gets the full key light, farther ones less by inverse square, so paint a cluster in one op), cool fill from above (`--fill`, `--fill-strength`), warm bounce from below (`--bounce`, `--bounce-strength`) and a darker core just past the terminator (`--core`); `--color` is the local colour in full light, so the lit side comes out at that colour and the shadow side about four value steps under it. The lights multiply the local colour, so a warm colour goes darker and browner in shadow; `--shadow <colour>` (`"shadow"` in `apply`) turns the side out of the key light towards that colour's hue and chroma while keeping the values the model gives it, by `--shadow-mix` (default 0.7) and less in the halftones, with the chroma lowered in the darkest parts. A cool lavender or blue grey keeps shadows from going brown or grey; raise `--fill-strength` (0.3 or so) for lighter shadow sides. Every colour option of `paint` takes palette names. Strokes follow the form (along bands of equal light on round forms, along the axis on cylinders, at `--angle` on planes), take their colour from the model where they lie and stop where the light changes, so none carries the lit colour into the shadow. Thick paint: `oil` and `oil-load` have `body` (a brush key, 0 to 1), so their strokes lay paint height: flat in the middle with bristle grooves, lips at the sides, thinner at light pressure, taller for bigger brushes; new paint pushes most of the wet paint under it aside, so it builds up only a little where strokes cross. `smudge` and `wet` drag it, `blender` flattens it, the eraser takes it. It shows only through a `relief` effect on the layer (`klad effect art.klad <layer> relief strength=4 softness=3.5 light=-1,-1`): strength about 4 to 5 for fruit or a figure, 1 to 2 for a table or a wall; softness about a fifth of the stroke size (3 to 4.5 for strokes of 15 to 20 px). Below 1, a surface built from many small strokes shows every stroke's ridges and reads as wrinkled leather, not paint. Without the effect body changes no pixel. Passes go from twice `--weight` down to half of it (the small ones only where the form turns), then a decisive pass with `--bold` (`oil-load`) in the lights and a few quiet strokes in the shadows (`--decisive 0` skips both); `--vary` lets each stroke stray in value and temperature. Paint on a layer clipped to the flats, or lock the flats layer and paint into it: strokes then run past the outline and short strokes along it cut the edge in; on a plain layer they stay inside it and the flats show at the rim.
- Every stroke the shape ops make is drawn like a hand (`--hand 1`, the default; `0` gives the old even strokes): its own pressure curve (a press-in, a peak, a longer lift-off), tapers that are a share of its length, a width that varies from stroke to stroke, and a slight bow on hatching and flat planes. Brushes for it: `oil` (soft edge, size and opacity from pressure) for building forms, `oil-load` (harder edge, more grain) for strokes that should stay visible.
- `klad light` takes `--lamp x,y,height` too: each shape is shaded away from the lamp instead of one direction for all.
- `klad prepare art.klad [--from 0 --to 23]` replays every painted drawing of those frames into the cache after a big CLI change, so the editor opens and plays it without replaying strokes. The editor still makes its frame pictures itself, quickly, from the cached drawings.

- Foliage: never one leaf picture scattered evenly, and never a smooth blob with leaves sprinkled on its edge; both look fake. Block the crown in as 6 to 12 clump shapes (irregular blobs with lobes) plus one `crown` shape around them all, then for each clump from back to front: fill a darker, slightly smaller core (about 0.82 of the clump, so the sky shows between the edge leaves) and run `foliage` on it with `mass: "crown"`. Every leaf takes its colour from the clump lit as a round form and turned with the whole crown, in three value families from a cool shade through `--color` to a warm `--lit`, with the most variety in the light; leaves along the outline break the silhouette, most where light and sky reach, a few glowing at the shadow edge. Back clumps a darker colour, front ones lighter; leaf size about a fiftieth of the crown's width (`samples/tree`). Far canopies stay soft vector masses.
- Grass: `grass` grows blades from the far (top) edge of a shape to the near one, smaller and denser far off (`--far`), leaning with `--wind`, each from a dark root to a lit tip over ground that rolls in soft light patches, with taller blades breaking the top edge and thin glints on lit blades. One kind of blade everywhere looks plastic: grass is a mix of species, each growing in its own patches. Without `kinds` the op grows a meadow mix (fine blades, broad flopping ones, dry straw, stalks with seed heads, clover low between them); give your own with `kinds` (JSON: `name`, `share`, `brush`, `length` and `width` against the blade, `curl`, `spread`, `color` or `hue`, `head` a picture brush stamped at the tip with `head_size`, `low` for rosettes and clover, `patchy` 0 to 1). Seed heads and low plants thin out with distance. Fill the shape a dark green first so gaps read as depth. A field with short blades and a near bank with long ones make a meadow; rare accents (your own tufts or flowers) are a species with a small share. Distant grass is a soft mass, not blades.
- The shape ops decide where marks go and how light colours them; the brush decides what a mark looks like, and it should be yours when the subject needs it. `foliage`, `grass`, `scatter` and `paint` all take `--brush`: a stroke brush (round or chalk tip) draws each leaf or blade as a stroke, a picture brush stamps one picture per mark, turned to it. To make one, draw the shape in a small drawing of its own (black on transparent, pointing right for brushes whose angle follows the stroke), `klad render` it to a PNG, add it with `klad brushes art.klad --picture tuft tuft.png`, and define the brush with `klad brushes art.klad --set tuft tuft.json` (`{"tip":{"image":{"image":"tuft","more":["tuft-b"]}},"angle":"follow","spacing":1}`; several pictures give variety). `samples/meadow/build.sh` does exactly this. Make a new leaf or tuft for each species and style instead of reusing the library's for everything.
- Ink rules: heavier on the side away from `--light` (the direction the light comes from, -1,-1 is top left), heavier on the silhouette than on inner lines (an outline inside another shape given in the same call), thinner on tight curves, a seeded wobble (`--wobble 0` for none). Outlines split into separate strokes at corners sharper than `--break` (60 degrees); `--gap 3` leaves inkers' gaps where lines meet. Open details (folds, cracks, ribs) go through `--path "M..."`.
- `klad light` writes lasso fills with a cut: on the shade layer the shape minus itself moved towards the light (`--soft 3` overlapping steps soften the inner edge), on the rim layer a band along the lit edge. Clip the shade layer (multiply) and the rim layer to the flats, as in `samples/lantern/`.
- `samples/lantern/generate.py` is the whole recipe for a prop in about 100 lines; `samples/ink-shapes/` shows the ink options side by side.
- Details that make a prop read as made rather than blocked in (an early version of the lantern read "kind of as good, but its details are not too great"): connect what hangs (hook, links, bead, cap: every piece touches the next; a link seen face-on is a closed ring through `--path`, one seen edge-on a small filled bar); give a round form a crisp core shadow with its own `light` pass (`--soft 1`, depth about a third of its width, `shadeOpacity` 1) instead of a soft gradient; build a stone or block from two planes (a top face and a darker side as its own shape) and keep texture to the side plane; plant grass as a few tufts where things meet the ground, leaning away from the object, not a fringe along an edge; ink a shape that enters another as an open path, so its cut edge is not drawn on top.
- Overlapping parts (a character's limbs, gear and clothing; the pieces of a prop) each get their own flats, shade, rim and lines layers, stacked back to front, so the parts in front cover the outlines behind them; one lines layer over all the flats draws every outline through everything in front of it. Build each limb as one shape (not a thigh and a shin that overlap), let boots, hands and straps cover what they hold, and cut an opening (a hood around a face) as one shape instead of stacking a band over it. A dark backing that shows through gaps (mortar between stones) stays inside the shapes it backs.
- Group pieces into parts by overlap, not by material or by object: a part is a set of pieces none of which overlaps another. Two pieces that overlap go in different parts, the front one above (a bottle's neck over its body, a key's bow over the lobes behind it, a sign's face over its back edge, a mushroom's cap over its stem). Before drawing, list the pieces and pair up every overlap. Shading bands (the dark side of a cylinder, a terminator band) are tones inside a piece, never pieces of their own: fill them, never ink them, or the ink draws a seam down the form. For a stack of many small pieces (coins, pebbles, scales) fill and ink each piece in turn on one layer, far to near, instead of a layer set per piece.
- The ops are `ink`, `light`, `hatch`, `paint`, `scatter` and `fill` with `shape`; shapes added earlier in the same `apply` can be used by them. `samples/lantern/` builds the inked prop, and with `PAINTED=1` the painted one (`paint` by form in place of `light` and `hatch`).

## Bones, poses and keys

Draw a character once and pose it, instead of redrawing every frame. Its skeleton goes on a bones layer of its own (it draws nothing), and its paint layers join it with `klad attach`: each follows one bone rigidly, or bends with the skeleton by the weights. Name parts as you draw (`"part"` on strokes and fills): they are what weights and the rig export use.

```bash
klad add art.klad layer bones name=figure                    # the bones layer; the commands below name it
klad bone art.klad figure --add body --head 400,590 --tail 400,360
klad bone art.klad figure --add upper --parent body --head 452,392 --tail 560,360    # a child starts at its parent's end, or anywhere
klad bone art.klad figure --chain thigh,shin,foot --through '390,590 395,700 385,810 425,815' --parent body   # each the child of the one before
klad ik art.klad figure --add shin                           # a limb: shin and thigh reach for a target, bending as drawn
klad attach art.klad forearm --bone fore                     # a layer drawn as its own piece moves with one bone, no weights
klad attach art.klad lines --bend --skeleton figure          # a layer drawn in one piece bends by the weights
klad weights art.klad figure --auto                          # blend from the bones, for layers that bend
klad weights art.klad figure --bone head --part head         # a part held rigidly by one bone
klad weights art.klad figure --bone upper --set '470,380,20,1 500,370,16,0.8'    # soft dabs: x,y,radius,weight
klad pose art.klad figure --from 0 --to 4 --bone upper=-35 --bone fore=-55 --hold 4
klad pose art.klad figure --from 0 --to 8 --target shin=405,800 --root 30,-6        # the foot stays on 405,800 while the body moves
klad inbetween art.klad --layers colour,lines --from 0 --to 4      # poses of one rest drawing in-between on the bones
klad render art.klad --frame 4 --overlay bones                      # or --overlay weights:upper
klad key art.klad lantern --frame 0 y=0 --ease in-out; klad key art.klad lantern --frame 12 y=-18
klad export art.klad --godot-rig game/art --godot-scene game/art
```

- Angles are degrees from the rest pose, positive turning clockwise on screen; a child follows its parent. Pose from the rest drawing (`--from` its frame); posing a frame again re-poses it.
- Fix a skeleton in place: `--move upper --tail 570,350` moves an end (children that start at that tail come along), `--move upper --by 10,0` moves a bone with all its children, `--reparent hand --parent fore` changes a parent. The user does the same in the bone tool: the skeleton list picks a bone, its ends and middle drag, a drag on empty canvas draws a chain (each click adds the next bone; Esc ends it), alt drag from an end starts a chain from there.
- IK poses a limb by where its end goes: `klad ik --add shin` makes the shin and its parent a limb (`--chain 1` an aim: the bone alone points at the target), `--bend clockwise|counterclockwise|flip` picks the side the joint bends to (by default as drawn), `--softness 8` keeps a knee from snapping straight. Each pose then sets targets (`--target shin=x,y`, canvas px): the limb turns so the shin's tail lands there, and the target ignores `--root`, so a planted foot stays planted while the body bobs; `--mix shin=0.5` blends between the bones' own turns and the target. In-betweens move targets in straight lines, so plant feet with the same target in both keys and swing free limbs with turns instead. `klad bones` lists the constraints, each pose's targets and its solved turns.
- A paint layer moves with the bones only once it is rigged: attached (`klad attach <layer> --bone <bone>`) it moves rigidly with that bone, as a cut-out piece; set to bend (`--bend --skeleton <bones layer>`) each point moves by the weights; not rigged, posing leaves it alone. Draw limbs on their own layers and attach them when they should stay stiff; let a layer drawn in one piece across a joint (a sleeve, a tail) bend and paint its weights. Rigged layers can sit anywhere in the layer tree. `klad bones` says which layers follow which bone.
- Automatic weights bend joints over a short stretch; hold rigid things (head, hands, props) with `--part`, or a stiff limb with dabs at 1. Look at `--overlay weights:<bone>` before posing.
- Keys move whole layers, groups and shapes (`x`, `y`, `rotate`, `sx`, `sy`, `scale`, `opacity`) between frames with an ease (`linear`, `in`, `out`, `in-out`, `step`, or a curve `x1,y1,x2,y2`); a segment follows its first key's ease. A lantern that floats, a glow that pulses, a far plane that drifts: no drawing per frame.
- Godot: `--godot` exports frames (always works), `--godot-rig` a Skeleton2D with Polygon2D parts and an AnimationPlayer per clip (only frames that are poses of the rest drawing; the others are listed), `--godot-scene` one Parallax2D per top-level layer scrolling by its depth. `samples/rig` builds all of it.

## Painting over, and measuring against a reference

`paintover` turns a flat or smooth picture into a painted one: it renders what is below a paint layer (or `--from` layers) and lays brush strokes on that layer, big to small. Each stroke takes the source's colour where it starts and runs along the forms (across the colour gradient) until the colour under it changes; smaller brushes only go where the painting still differs from the source by more than `--threshold`, so detail lands where the source has it. Flat areas get broad, mostly level strokes. The strokes are ordinary strokes in the part `paint over`, so `body` and `relief` light them and the user can edit them.

```bash
klad add art.klad layer paint name=brushwork            # on top of the drawing
klad paintover art.klad brushwork --sizes 32,16,8 --threshold 6 [--in rock] [--focus face] [--brush oil-load] [--curve 0.5] [--length 1,8]
klad paintover walk.klad brushwork --frame 0 --sizes 14,7
klad paintover walk.klad brushwork --like c37 --frames 0-27    # carry it drawing to drawing through an animation
```

- Block in and light the picture first (flats, `paint`, shadows), then paint over it: the paint-over keeps your values and edges and adds the hand. Keep the source layers under the brushwork; hide them or not.
- `--focus <shape>` keeps the smallest brush to the focal area, so detail falls off away from it. Bigger `--threshold` means fewer, bigger strokes.
- Animation: never paint over each drawing on its own (the strokes jump every frame). Paint over the first drawing, then `--like <its cel> --frames a-b`: each later drawing gets the previous one's strokes moved with the source marks under them, recoloured from the new drawing, and new strokes only where the source changed; the background stays still. The carried strokes keep their order and seeds, so in-betweens of the brushwork layer pair them part by part; the strokes added where a drawing needed them (part `paint over added at <frame>`) belong to that drawing only, so in-between with `--unpaired fade` to fade them in and out.
- `paintover` needs a batch of its own in `apply` (it reads the drawing as it is).

`compare` measures your drawing against a reference picture, so a critique is numbers as well as a look: the value range (L at the 5th, 50th and 95th percentile), saturation, stroke texture, edge sharpness and mean colour difference, with what differs in words, plus a row per `--region` and a heat map of where they differ most.

```bash
klad compare art.klad study.png --region shade=40,300,200,120 --region cup=610,180,90,120 --heat diff.png
```

Read the words ("lighter by 4 L", "bluer", "33% busier", "softer edges") and fix the biggest first. In the editor, "compare with the reference" (View) does the same against the drawing's reference picture layer.

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

To recreate a picture (a reference study): `klad add art.klad layer image src=ref.jpg reference=true`, then size it to the canvas with `klad set art.klad <node> x=0 y=0 w=<width> h=<height>` (a picture smaller than the canvas is placed at its own size). Read the reference in quarters with `klad render art.klad --references --region x,y,w,h` (a reference under an opaque background is covered: hide the background or render the reference alone), and take coordinates from those crops. Build in planes from back to front, compare often on one sheet (a bundle holding the reference picture above the render), and fix values before details: the first passes of a cave study were too bright everywhere, not too plain.
- Light layers and mist on `screen` go behind the planes they light from behind; above a plane they turn its forms milky.

## Export

```
klad export art.klad --scale 2
klad export art.klad --layer hero --scale 4 --depth 16 -o hero.png
```

Transparent PNG of the image or one layer or group, next to the bundle unless `-o`. 8-bit exports are dithered so gradients do not band; `--depth 16` keeps 16 bits; `--background` keeps the canvas colour. Vectors stay sharp at any scale, and above full size `klad export` draws the strokes again at that size, so lines stay crisp and textures keep their size (it replays every mark, so it takes a few seconds on a dense painting). Sprite sheets, numbered frames and Godot exports are under Time, Animation exports.

Size the canvas for the largest size the art is ever seen at, not for the size it is used at. Paint layers are pixels at the canvas size: `klad render --scale` above 1, the editor's zoom, a sprite scaled up or a game camera that zooms in all magnify them, and every pixel shows as a block (only `klad export --scale` draws them again). Work out the largest size on screen (the camera's closest zoom times the sprite's scale, times 2 on a high density display), make the canvas about twice that, and show it scaled down (in Godot with mipmaps on). At that size give inked lines 4 to 8 px and noise or grain effects a `size` of 2 or more: lines of 2 to 3 px and 1 px noise read as pixels the moment anything enlarges them. Check at the size the player sees and at the closest zoom, not only at 1:1 on the canvas.

## Scripts

For anything with more than a handful of shapes, write a generator that prints ops as JSON lines and pipe it into `klad apply`. Rebuild from scratch each run (copy the source bundle, apply, render) so the script stays the source of truth. A landscape generator of about 18,000 nodes builds and renders in about 4 s this way.

## Time

`timeline` has `fps` and `frames`; each layer has cels (`frame`, optional `hold`, optional `link` to reuse another cel). A cel that is not a link is a drawing; the frames it shows on are its exposure (from `frame` for `hold` frames, or until the next cel). `--frame n` on `ls`, `render`, `stroke`, `fill`, `lasso` and `add` picks the frame; a stroke or fill paints into the drawing exposed there.

```
klad frames art.klad                          # exposure sheet: c4 = a drawing starts, | still showing, c9->c4 a link, . blank
klad timeline art.klad --fps 12 --frames 24   # length and speed (no flags: show them, the origin, the Godot folder and the clips)
klad cel art.klad new l3 --frame 4            # blank drawing; inside an exposure it takes over the rest of it
klad cel art.klad dup c4 [--frame 6] [--insert]   # copy (strokes keep their ids); default right after c4; --insert moves later cels back
klad cel art.klad link c4 --frame 8 --hold 2  # show c4 again
klad cel art.klad unlink c9                   # the link becomes a drawing of its own
klad frame art.klad insert 4 [--count 2] [--layers l3]   # what showed at 4 shows longer, the rest moves back
klad frame art.klad remove 4 [--count 2] [--layers l3]
klad hash art.klad --all                      # one pixel hash per frame
```

In `apply`: `{"op":"cel","new":"lines","frame":4,"hold":2,"id":"ball4"}` (the `id` lets later ops in the same batch name it), `{"op":"cel","link":"ball0","frame":15}`, `{"op":"frames","insert":4,"count":2}`, `{"op":"timeline","frames":24}`. For an animation script: make every drawing's cel first (`cel new` with an `id`), then its strokes and fills at that frame. `samples/bounce/generate.py` is the pattern: lines, colour filled against the lines, a multiply shadow, a held floor, the rest pose as links.

Strokes copied by `dup` keep their ids, so `c7:s3` in the copy is the same line as `c4:s3`: move it with `klad transform` to make the next pose. Removing a drawing that links show (`rm`, or removing its frames) gives it to the first link instead of breaking them.

### Keys, in-betweens, colour and timing

Animate by keys: draw the key poses with their parts named, check the keys' arcs, let `klad inbetween` make the drawings between them, colour every drawing from the first with `klad fill --like`, check with `klad gaps`, set the timing with `klad retime`, check the arcs again. `samples/creature/` (`generate.py` and `build.sh`, which writes `creature.klad`) is the pattern: an idle and a walk on six layers (shadow, far leg, colour, a clipped multiply shade, a clipped rim light, lines), parts body, sprout, leaf, second leaf, near leg and far leg, `klad arcs --check` before and after in-betweening, the loop closed with `--loop`.

```
klad copy art.klad c1:s3 c1:s4 --frame 6 [--layer shade] [--move 12,-4 --rotate 8 --pivot 512,400] [--cut 'x,y ...']
klad inbetween art.klad --layers lines,colour,shade --from 0 --to 6 [--count 2] [--spacing in-out] [--match near] [--pair s3=s7]
klad inbetween art.klad --layers lines,colour --from 9 --loop      # the loop's last in-betweens, towards its first drawing
klad inbetween art.klad --layers lines --from 0 --to 6 --unpaired pop|fade|draw-on [--at 0.3]
klad inbetween art.klad --redo [--layers lines] [--keep-drawn | --force]
klad part art.klad c4:s14 c4:s15 --name leaf    # or --part leaf on stroke, fill, ink, hatch, scatter
klad arcs art.klad --track 'lines:near leg' --track 'far:far leg' --clip walk --ground 823,811 --travel 7 --check
klad render art.klad --frame 20 --track 'lines:near leg' --region 420,720,200,120 --scale 4   # the paths drawn
klad render art.klad --sheet walk --drawings --overlay parts
klad fill art.klad colour --like colour0 --frames all
klad gaps art.klad colour --sheet all          # closed regions no fill covers, with a render marking them; naming the lines layer checks the layer that fills from it
klad retime art.klad 0-11 --on 2               # or --on 1, --fit 16, --reverse, --pingpong; [--layers l3]
```

- Name parts as you draw keys (`--part`, or `"part"` in ops): in-betweens pair part by part first, whatever the ids, so keys drawn separately, or redrawn, or with the user's strokes in between, still pair. Give each piece that moves on its own its own part (one per leaf, per limb): within a part marks pair in drawing order. Without parts keys pair by mark id, so make every key with the same ops in the same order, or `cel dup` a key and change the copy. `--match near` pairs drawings made separately (and turns strokes drawn backwards); `--pair` forces the odd one. Unpaired marks are left out and listed by default: read that list, or let them `--unpaired pop|fade|draw-on`.
- Check the keys' motion before in-betweening, the way an animator flips them: `klad arcs` with a track per foot (parts), `--ground` per foot (a far foot drawn higher for depth has its own line), and `--travel` for a cycle drawn in place (a planted foot moves back by the travel per frame; on twos after retime, halve it). It reports feet under the ground, planted feet sliding, and two feet swapping sides (legs crossing); `--check` makes the build stop. It follows the part of the sole on the ground, so a foot rolling from heel to toe is not a slide; for a foot with no flat sole, track a point on it instead (`--track 412,820`, carried by the stroke nearest to it). The creature's first keys slid their feet with the body's tilt: feet go where the ground is, not where the hip is.
- Light that changes as a limb turns: colour the second key's strokes and fills for their new light (`paint` on that key, or `set <cel>:s<id> color=...`); in-betweens blend each pair's colour evenly (in OKLab) along with its points, size, opacity and taper.
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
klad clip art.klad                                          # list: frames, length in s at its fps, loops or once
klad clip art.klad --add run --from 0 --to 11 --loop --fps 24
klad clip art.klad --set run --to 15 --no-loop              # change; --name sprint renames, --fps doc drops its own fps
klad clip art.klad --rm run
klad timeline art.klad --origin 512,700                     # the canvas point a Godot export puts on the node (default the centre)
```

In `apply`: `{"op":"clip","add":"run","from":0,"to":11,"loop":true,"fps":24}`, `{"op":"clip","set":"run","name":"sprint","fps":null}`, `{"op":"clip","rm":"run"}`, `{"op":"timeline","origin":[512,700]}`. Frames inserted or removed on every layer move and stretch clips (an insert inside a clip lengthens it); on listed layers they leave clips alone. `klad frames` marks clips on the right (`[run` first frame, `run]` last) and lists them under the sheet. In the editor clips are bars on the strip above the frame numbers, and Return plays the clip under the playhead at its fps.

### Animation exports

```
klad export art.klad --sheet all [--scale 0.5] [--padding 2] [--max 4096] [--depth 16]
klad export art.klad --sheet run -o out/run.png
klad export art.klad --frames run                           # run-0000.png ... full size, one per frame, in art-run/
klad export art.klad --gif [run] [--scale 2] [--background] [--dither]   # art-run.gif per clip, or art.gif of the whole timeline
klad export art.klad --godot ../game/art/hero               # first time: the folder is kept in the document
klad export art.klad --godot                                # later: the kept folder (the editor's button does the same)
```

- `--sheet all` exports every clip (or the whole timeline as `default` when there are none), `--sheet <clip>` one, `--sheet 0-11` a range. The PNG holds each distinct picture once (holds, links and frames that render the same share one), trimmed to its pixels, in order of first appearance; beyond `--max` it goes to more pages (`art-sheet-2.png`, ...). The JSON is Aseprite's array layout: one entry per exposure with `duration` in ms at the clip's fps (a clip's durations add up to its length), `frame` (rect in the page), `spriteSourceSize` (the trim) and `sourceSize`, plus `frameTags` per clip (`repeat: "1"` when it plays once). Entries on later pages carry `image`.
- `--gif` writes an animated GIF for each clip (`art-<clip>.gif`; a clip that loops makes a GIF that loops), or one of the whole timeline when there are no clips; `--gif <clip>` or `--gif 0-11` writes one, and `-o` names the file (or the folder, for several). Each picture is shown for as long as its frames last, in hundredths of a second with the rounding carried over, so the GIF runs as long as the clip. A GIF holds 255 colours and one clear one: pixel art and flat colour come out exact, a painting with more colours is reduced to 255: by default each pixel takes the nearest one, which keeps flat areas clean but bands soft gradients (a glow turns into rings); `--dither` mixes the nearest colours in a fine even pattern instead, which keeps gradients smooth at the cost of a visible grain and a file about a third larger. Use `--dither` for painted, lit or hazy pieces, leave it off for flat colour and line art; it changes nothing when the frames hold 255 colours or fewer. Half-clear pixels become clear or solid. Transparent unless `--background`.
- `--godot <folder>` writes `art.png` (and pages), `art.tres` (a `SpriteFrames`: an `AtlasTexture` per picture with its trim as margin, one animation per clip with its name, speed, loop and per-frame durations in frames) and `art.tscn` (an `AnimatedSprite2D`, autoplaying the first clip, offset so the origin sits on the node's position). Re-exports keep the uids of the `.tres`, `.tscn` and the imported PNG, so scenes using them keep working. `--store` replaces a kept folder; the kept path (`export.godot`) is relative to the folder holding the bundle.
- Exports print a pixel hash per page and of the JSON on stderr. The bounce (`samples/bounce/build.sh`) exports in about 0.25 s; 40 distinct 1024 frames to Godot in about 1.4 s warm, 2.3 s cold.

See the motion before reading single frames:

```
klad render art.klad --sheet all              # every frame in one grid: frame number, drawing ids, holds
klad render art.klad --sheet 0-11 --drawings  # one cell per drawing: frames that look the same share a cell ("4-5")
klad render art.klad --sheet run --cell 192   # a clip by name; --cell is the longest side of a cell, --columns n
klad render art.klad --frame 5 --overlay onion            # drawings before (red) and after (green) under each layer
klad render art.klad --frame 5 --onion 1,3 --onion-layers lines --onion-by frames --no-tint
klad render art.klad --sheet 0-7 --overlay onion --onion-loop 0-7   # onion in every cell, wrapping inside a loop
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
