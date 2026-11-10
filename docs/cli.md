_Last updated: with the commands it lists. `just reference` writes it from `klad reference`; `cargo test -p klad-cli` fails when it is out of date._

# klad command reference

Every command of the `klad` command line with its options, as `klad <command> --help` prints them. A drawing is a `.klad` folder (`docs/format.md`); `klad apply` takes the same edits as JSON lines, one transaction per call. `docs/skill/SKILL.md` explains how an agent uses them, and the `klad-client` crate (`crates/client`) runs them from Rust.

This reference is licensed under the MIT License (`LICENSE-MIT`), unlike klad itself.

## Commands

- [`klad new`](#klad-new): Create an empty bundle with paint layers
- [`klad frames`](#klad-frames): Exposure sheet: a row per frame, a column per layer (top first)
- [`klad frame`](#klad-frame): Insert or remove frames in every layer or the listed ones
- [`klad arcs`](#klad-arcs): Track marks, parts or points across frames: positions, spacing, ground contacts, feet crossing
- [`klad part`](#klad-part): Name a group of marks in a drawing (a part), so in-betweens pair them whatever their ids
- [`klad duplicate`](#klad-duplicate): Duplicate a layer with its drawings, links and mask, above it
- [`klad cel`](#klad-cel): Make drawings on the timeline: new, dup, link, unlink, move
- [`klad copy`](#klad-copy): Copy strokes and fills into another drawing, with new ids, moving them on the way
- [`klad retime`](#klad-retime): Retime a range of frames: on ones/twos/threes, fit into n frames, reverse, ping-pong
- [`klad inbetween`](#klad-inbetween): Make the drawings between two keys: a new cel on each frame between them
- [`klad font`](#klad-font): Copy a font into the bundle for text nodes, by name or from a file, or list the fonts it has
- [`klad fonts`](#klad-fonts): List the fonts that come with klad, search the open fonts it can download, or download one
- [`klad atmosphere`](#klad-atmosphere): Fog and haze by depth: show, set or turn off the document's atmosphere
- [`klad timeline`](#klad-timeline): Show the timeline (fps, length, origin, clips) or set fps, length and origin
- [`klad clip`](#klad-clip): List, add, change, rename and remove clips (named frame ranges)
- [`klad ls`](#klad-ls): Layer tree with ids, kinds, contents and bounds at a frame, top layer first
- [`klad show`](#klad-show): One layer, cel, node, stroke or fill (<cel>:s<id>) in full (JSON)
- [`klad add`](#klad-add): Add a layer, cel or shape
- [`klad set`](#klad-set): Change properties: klad set art.klad n4 fill=#ff0000 transform.rotate=15 (null removes)
- [`klad recolour`](#klad-recolour): Recolour strokes and fills together: the group moves to the colour and each keeps its own lighter or darker shade
- [`klad detach`](#klad-detach): Detach eraser, smudge and blender strokes from the marks they are attached to
- [`klad rm`](#klad-rm): Remove layers, cels or nodes
- [`klad transform`](#klad-transform): Move, rotate or scale strokes and fills (<cel>:s<id>), or a whole layer or group (--layer)
- [`klad lasso`](#klad-lasso): Lasso strokes and fills on a paint layer; --cut also takes the parts crossing the edge
- [`klad path`](#klad-path): List the points of a path, rect or ellipse, or edit them
- [`klad mv`](#klad-mv): Move a layer or node: --above, --below, --in, --index
- [`klad apply`](#klad-apply): Apply many edits from JSON lines (stdin or --file) in one write
- [`klad render`](#klad-render): Render a frame to PNG and print the path
- [`klad stroke`](#klad-stroke): Paint a stroke with a brush along an SVG path or explicit points
- [`klad fill`](#klad-fill): Fill a region of a paint layer, inside the lines (flood) or inside a polygon (lasso)
- [`klad ink`](#klad-ink): Ink the outlines of vector shapes as tapered strokes, the way an inker would
- [`klad light`](#klad-light): Shade and rim light vector shapes from one light direction
- [`klad hatch`](#klad-hatch): Fill a shape's area with short tapered strokes: hatching, bark, fur, stone grain
- [`klad paint`](#klad-paint): Paint a shape as a form, with strokes that follow it and colours from a light model
- [`klad prepare`](#klad-prepare): Buffer the drawings of a span of frames, as the editor's prepare does
- [`klad scatter`](#klad-scatter): Scatter copies of a small mark over a shape: grass, cracks, dots
- [`klad foliage`](#klad-foliage): Paint closed shapes as clumps of leaves, lit as forms
- [`klad bone`](#klad-bone): Add, remove, rename, move or reparent a bone of a layer's or group's skeleton
- [`klad ik`](#klad-ik): Add, change or remove an IK on a bone: a limb (the bone and its parent) or an aim reaches for a target set per pose
- [`klad attach`](#klad-attach): Rig a layer: attach it to a bone, let it bend with a skeleton, or unrig it
- [`klad solo`](#klad-solo): Show only one layer: hide every other layer (its groups stay shown, its own layers as they are)
- [`klad bones`](#klad-bones): List the skeletons, their bones and weights, and the posed drawings
- [`klad weights`](#klad-weights): Set how much each bone moves each part of the drawing: automatic, painted dabs, a whole part, smoothed
- [`klad pose`](#klad-pose): Pose a drawing with its bones: a new drawing at --to, bent from the rest drawing at --from
- [`klad key`](#klad-key): Key a layer, group or shape: its position, turn, scale or opacity at a frame, eased to the next key
- [`klad keys`](#klad-keys): List the keys of every keyed layer, group and shape (or one)
- [`klad compare`](#klad-compare): Measure a drawing against a reference picture: values, saturation, texture, edges, colour per region
- [`klad paintover`](#klad-paintover): Paint over a picture with brushwork: strokes that follow its forms and stop at its colour edges, big to small
- [`klad grass`](#klad-grass): Grow grass in closed shapes: blades lit by the ground, smaller with distance, breaking the top edge
- [`klad gaps`](#klad-gaps): List closed regions of the lines that no fill on the layer covers
- [`klad refill`](#klad-refill): Recompute the flood fills of a paint cel after the lines they were made from changed
- [`klad palette`](#klad-palette): The document palette: list, --add a colour (with --name), --rm a name, colour or index
- [`klad export`](#klad-export): Export PNGs, sprite sheets, numbered frames or a Godot SpriteFrames and scene
- [`klad brushes`](#klad-brushes): List brushes: the library, and with a bundle the ones it embeds
- [`klad effect`](#klad-effect): List, add or remove a layer's effects: klad effect art.klad l3 glow radius=24 color=#ffd080
- [`klad reshape`](#klad-reshape): Bend a brush stroke along a new path; it keeps its pressure, tilt and timing (klad strokes --json gives each stroke's fitted path)
- [`klad undo`](#klad-undo): Undo the last change (from the CLI or the editor)
- [`klad redo`](#klad-redo): Redo the last undone change
- [`klad log`](#klad-log): Recent changes, newest first
- [`klad selection`](#klad-selection): What the human has selected in the editor (layer and shapes, with bounds)
- [`klad notes`](#klad-notes): Notes the human pinned on the canvas; --done <id> marks one as handled, --clear-done removes the handled ones
- [`klad diff`](#klad-diff): Before and after renders of a change, side by side (default: the last one)
- [`klad info`](#klad-info): Paint cels with stroke counts and file sizes
- [`klad repair`](#klad-repair): Give every layer, drawing or shape whose id is used twice (after a merge) a new id, moving its files along
- [`klad strokes`](#klad-strokes): List the strokes and fills of a paint layer's cel at a frame
- [`klad hash`](#klad-hash): Print pixel hashes, for comparing macOS and Linux renders
- [`klad demo`](#klad-demo): Write a sample bundle (vector sky and hills, painted glow, clouds, hatching, grass)
- [`klad stress`](#klad-stress): Create a large bundle of synthetic paint layers for editor performance tests
- [`klad scribble`](#klad-scribble): Append synthetic ink strokes to a paint layer (sync test)
- [`klad watch`](#klad-watch): Print editor saves as they arrive, with how long they took to show up here
- [`klad licences`](#klad-licences): Print the licence and the third-party notices (every open source part klad is built on)
- [`klad install`](#klad-install): Put klad on the terminal's path, for you and for agents
- [`klad skill`](#klad-skill): Print an agent skill: how an agent draws with klad, or the craft of drawing itself
- [`klad samples`](#klad-samples): List the sample drawings that come with klad, or copy one to draw on
- [`klad report`](#klad-report): Print a problem report to paste into an issue: version, system and the last lines of klad's log
- [`klad updates`](#klad-updates): Check now whether a newer klad is out (the editor also checks once a day by itself)
- [`klad bench`](#klad-bench): Measure stroke storage, brush speed, replay, cache and compositing
- [`klad open`](#klad-open): Open the editor, on a bundle or on the start window (new, open, recent)

## klad new

```text
Create an empty bundle with paint layers

Usage: klad new [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --width <WIDTH>
          [default: 2048]

      --height <HEIGHT>
          [default: 2048]

      --layers <LAYERS>
          [default: 1]

      --background <BACKGROUND>
          Background colour, for example #ffffff

      --fps <FPS>
          [default: 12]

      --frames <FRAMES>
          [default: 1]

      --sketch
          Add a vector layer named sketch on top, for blocking in shapes

  -h, --help
          Print help
```

## klad frames

```text
Exposure sheet: a row per frame, a column per layer (top first).

A cel id marks the frame a drawing starts, | that it is still showing, c5->c1 a link (c5 shows c1's drawing), . a blank frame. Clips are listed on the right ([run on their first frame, run] on their last).

Usage: klad frames [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --from <FROM>


      --to <TO>


      --json


  -h, --help
          Print help (see a summary with '-h')
```

## klad frame

```text
Insert or remove frames in every layer or the listed ones.

  klad frame art.klad insert 4 --count 2
  klad frame art.klad remove 4 --layers l3,l5

Insert: what showed at the frame now shows there and for the inserted frames too (that exposure lengthens), everything after moves later. Remove: exposures over the removed frames get shorter, drawings left with no frames go (links to them take them over), everything after moves earlier. On every layer, clips and the timeline length follow; on listed layers an insert lengthens the timeline only as far as their drawings need (a last drawing without a hold keeps its length).

Usage: klad frame [OPTIONS] <BUNDLE> <ACTION> <AT>

Arguments:
  <BUNDLE>


  <ACTION>
          [possible values: insert, remove]

  <AT>


Options:
      --count <COUNT>
          [default: 1]

      --layers <LAYERS>
          Layer ids or names (default: every layer)

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad arcs

```text
Track things across frames, the way an animator checks arcs and spacing: a mark (c4:s12, through the drawings that keep its id, as in-betweens do), a part (leaf, or lines:leaf: the centre of the part's marks in each drawing) or a point (x,y: carried by the stroke nearest to it). Prints each frame's position, the distance moved since the frame before, and the lowest point. --ground y (one for all tracks, or one per track) adds the frames where a track goes under the ground line or slides along it while touching it (for a cycle drawn in place, --travel gives the px per frame the planted foot should move back by); with two or more tracks, the frames where two of them swap sides (feet crossing) are listed. --check makes any of these an error, for scripts. klad render --overlay arcs --track ... draws the same paths.

  klad arcs walk.klad --track 'near foot' --track 'far foot' --clip walk --ground 610,598 --travel 7 --check
  klad arcs walk.klad --track c12:s4 --from 12 --to 27 --json

Usage: klad arcs [OPTIONS] --track <TRACK> <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --track <TRACK>


      --clip <CLIP>
          The frames of this clip

      --from <FROM>


      --to <TO>


      --ground <GROUND>
          The ground line's y, one for every track or one per track: report feet under it or sliding on it

      --travel <TRAVEL>
          For a cycle drawn in place: how far the character travels per frame (px, + to the right); a planted foot should move back by as much

          [default: 0]

      --slack <SLACK>
          How many px off still counts as on the ground and not sliding

          [default: 2]

      --json


      --check
          Exit with an error when a track goes under the ground, slides, or two tracks swap sides

  -h, --help
          Print help (see a summary with '-h')
```

## klad part

```text
Name a group of marks in a drawing: a part such as leaf, near leg or body. In-betweens pair marks part by part first (in order within a part, nearest shape when the counts differ), whatever their ids; arcs track a part's centre. A mark is in one part at most; naming it again moves it. dup, copy and in-betweens keep parts. --name '' takes the marks out of their part.

  klad part art.klad c4:s14 c4:s15 --name leaf
  klad part art.klad c9:s17 c9:s18 --name leaf
  klad stroke art.klad lines --path "M..." --part leaf

Also an op: {"op":"part","ids":["c4:s14","c4:s15"],"name":"leaf"}. klad strokes lists each mark's part.

Usage: klad part [OPTIONS] --name <NAME> <BUNDLE> [IDS]...

Arguments:
  <BUNDLE>


  [IDS]...


Options:
      --name <NAME>


      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad duplicate

```text
Duplicate a layer: its drawings, links, in-between marks and mask are copied with new ids, and the copy, named "<name> copy", goes right above it.

  klad duplicate art.klad l3

Also an op in klad apply: {"op":"duplicate","layer":"l3"}.

Usage: klad duplicate [OPTIONS] <BUNDLE> <LAYER>

Arguments:
  <BUNDLE>


  <LAYER>


Options:
      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad cel

```text
Make drawings on the timeline.

  klad cel art.klad new l3 --frame 4            a blank drawing on layer l3 at frame 4
  klad cel art.klad dup c4 [--frame 6]           a copy of c4's drawing (default: right after c4's exposure)
  klad cel art.klad link c4 --frame 8            show c4's drawing again at frame 8
  klad cel art.klad unlink c9                    turn the link c9 into a drawing of its own
  klad cel art.klad move c5 --frame 12           start c5 at frame 12, keeping its hold; drawings in the way are pushed along

Inside an exposure a new cel takes over the rest of it; elsewhere it lasts until the next cel, or --hold frames (dup and link default to their source's hold). --insert moves later cels of that layer back to make room (only at the start or end of an exposure). Strokes keep their ids in a dup.

Usage: klad cel [OPTIONS] <BUNDLE> <ACTION> <TARGET>

Arguments:
  <BUNDLE>


  <ACTION>
          [possible values: new, dup, link, unlink, move]

  <TARGET>
          new: the layer; dup, link, unlink, move: the cel

Options:
      --frame <FRAME>


      --hold <HOLD>


      --insert


      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad copy

```text
Copy strokes and fills (<cel>:s<id>) into another drawing. The copies get new ids, printed as a map (c1:s3 -> c9:s12), and land on top of that drawing in the order they had. --to names the drawing; --frame takes the drawing the layer shows at that frame (a new drawing on an empty frame or past the end; a frame that shows a link names its source instead). The marks' own layer unless --layer. The move, turn, scale and flip options are klad transform's, applied to the copies together about the middle of their bounds or --pivot. --cut keeps only the parts inside a polygon.

  klad copy art.klad c1:s3 c1:s4 --frame 4
  klad copy art.klad c1:s3 --to c9 --move 12,-4 --rotate 8
  klad copy art.klad c1:s3 --frame 6 --layer shade --flip h --pivot 512,400

Also a copy op in klad apply: {"op":"copy","ids":["c1:s3"],"frame":4,"x":12,"y":-4} (or "to":"c9").

Usage: klad copy [OPTIONS] <BUNDLE> [IDS]...

Arguments:
  <BUNDLE>


  [IDS]...


Options:
      --to <TO>
          The drawing (cel id) the copies go into

      --frame <FRAME>
          The drawing shown at this frame (a new drawing on an empty frame)

      --layer <LAYER>
          With --frame: the layer (default: the marks' own)

      --move <OFFSET>
          dx,dy in px

      --rotate <ROTATE>
          Degrees, clockwise

      --scale <SCALE>
          Uniform scale

      --sx <SX>


      --sy <SY>


      --pivot <PIVOT>
          x,y to turn, scale and flip about

      --flip <FLIP>
          Mirror: h (left-right) or v (up-down)

          [possible values: h, v]

      --cut <CUT>
          Keep only the parts inside this polygon: x,y pairs separated by spaces

      --render
          Render a preview after the edit and print its path (longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad retime

```text
Retime frames FROM-TO (both included) on every layer, or on --layers (with their masks and children). The range is cut where any of those layers changes what it shows; each piece is one picture.

  klad retime art.klad 0-11 --on 2        every picture shows for 2 frames (ones to twos)
  klad retime art.klad 0-23 --on 1        and back to ones
  klad retime art.klad 0-11 --fit 16      the range becomes 16 frames; order and relative timing kept, at least one frame each
  klad retime art.klad 0-7 --reverse
  klad retime art.klad 0-7 --pingpong     the pictures again backwards as links, the ends not repeated: a b c d c b

Later cels move by the change in length; on every layer clips move and stretch with it and the timeline grows or shrinks; on listed layers clips stay and the timeline only grows. A drawing that ends up in two places is shown again by a link. One history step. Also a retime op: {"op":"retime","from":0,"to":11,"on":2,"layers":["l3"]} (or "fit":16, "reverse":true, "pingpong":true).

Usage: klad retime [OPTIONS] <BUNDLE> <RANGE>

Arguments:
  <BUNDLE>


  <RANGE>
          FROM-TO, both included

Options:
      --on <ON>
          Frames per picture: 1, 2, 3 ...

      --fit <FIT>
          The range becomes this many frames

      --reverse


      --pingpong


      --layers <LAYERS>
          Layer ids or names (default: every layer)

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad inbetween

```text
Make in-betweens of two keys on paint layers: the drawings shown at --from and --to (the second key is the frame where a drawing starts; a link there names its source, so a loop's last in-betweens can lead back to its first drawing). By default a new cel on every frame between them; --count n spreads n of them, each holding until the next.

  klad inbetween art.klad --layers lines,colour --from 0 --to 6
  klad inbetween art.klad --layers lines --from 0 --to 8 --count 3 --spacing in-out
  klad inbetween art.klad --layers arm --from 4 --to 10 --match near --pair s3=s7
  klad inbetween art.klad --redo                     after a key changed

Strokes pair by id (a key duplicated from the other keeps its ids), by --match order or by nearest shape; --pair forces pairs (first key id = second key id). Each pair is resampled along its length; points, pressure, tilt, size, opacity, flow, taper and colour move at the spacing (colour evenly in OKLab, so colour the second key for its light and the in-betweens carry the change), and the pair turns and scales along an arc about the point its best-fit rotation turns around, so a swinging limb keeps its length. A line end that touches another line in both keys touches it in every in-between. Lasso fills are interpolated; flood fills are recomputed in each in-between from a seed carried with the lines next to it (the layers they fill against go first). Unpaired marks and fills that fail (a seed on a line, a leak) are left out and listed.

Spacing: even, in (eases in: the in-betweens bunch near the first key), out (they bunch near the second key), in-out, or the positions themselves, 0.2,0.5,0.9. Every in-between remembers its keys (tween in klad show), so --redo remakes them after a key changed; it refuses cels changed since they were made unless --force. One history step.

Usage: klad inbetween [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --layers <LAYERS>
          Paint layers (ids or names; a group means its paint layers)

      --from <FROM>
          The frame of the first key

      --to <TO>
          The frame where the second key starts

      --count <COUNT>
          How many in-betweens (default: one per frame between the keys)

      --spacing <SPACING>
          even, in, out, in-out, or positions like 0.25,0.5,0.9

          [default: even]

      --match <MATCHING>
          Pair strokes by id, order or near (nearest shape)

          [default: id]

      --pair <PAIR>
          Force pairs: s3=s7 (first key's s3 with the second key's s7)

      --redo
          Remake the in-betweens (of --layers, between --from and --to when given)

      --force
          Replace drawings between the keys, or redo in-betweens changed since they were made

      --loop
          The second key is the loop's first drawing (the clip holding --from, else frame 0); --to defaults to the loop's end

      --keep-drawn
          With --redo: remake the agent's marks and keep the marks drawn by hand on top, carried with the lines under them

      --unpaired <UNPAIRED>
          Marks in one key only: leave them out (listed), pop at --at, fade, or draw on along their length

          [default: leave]
          [possible values: leave, pop, fade, draw-on]

      --at <AT>
          With --unpaired pop: when it pops, 0 to 1 (default 0.5)

      --only <ONLY>
          With --redo: only these in-betweens (cel ids)

      --keep-cels <KEEP_CELS>
          With --redo: these in-betweens keep their hand-drawn marks on top

      --force-cels <FORCE_CELS>
          With --redo: these in-betweens are remade with their hand-drawn marks

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad font

```text
Copy one of klad's fonts (by family, or family and style) or a font file (TTF or OTF) into the bundle's fonts folder, so text nodes render the same on every machine; a family that klad does not have yet but Google Fonts does is downloaded first. With no font, list the bundle's fonts. Text nodes also take a family name directly (font=Inter, font="Inter Bold") and copy it in themselves.

  klad font art.klad Cinzel
  klad font art.klad "Barlow Bold"
  klad font art.klad ~/fonts/Signage.ttf
  klad add art.klad text --in l4 text=INN font=Cinzel size=48 x=120 y=80 fill=#2b1d12

Usage: klad font <BUNDLE> [FONT]

Arguments:
  <BUNDLE>


  [FONT]


Options:
  -h, --help
          Print help (see a summary with '-h')
```

## klad fonts

```text
List the font families that come with klad and the ones downloaded before, by group (sans, serif, mono, display, handwriting, pixel), with their licence and styles; with a bundle, also the fonts it holds. --search looks through every open family on Google Fonts (about 1,900) and --get downloads one into the per-user fonts folder, after which it works like the ones klad ships.

  klad fonts
  klad fonts --group pixel
  klad fonts --search "slab"
  klad fonts --get Lobster

Usage: klad fonts [OPTIONS] [BUNDLE]

Arguments:
  [BUNDLE]


Options:
      --group <GROUP>
          Only this group: sans, serif, mono, display, handwriting or pixel

      --search <SEARCH>
          Search the open fonts that can be downloaded, by words of the family name or the group

      --get <GET>
          Download this family from Google Fonts into the per-user fonts folder

      --json
          Print JSON

  -h, --help
          Print help (see a summary with '-h')
```

## klad atmosphere

```text
The atmosphere fades layers at the top of the stack that have a depth (klad set art.klad g2 depth=2) towards the fog colour, blurs them and takes colour out, more the deeper they are.

  klad atmosphere art.klad
  klad atmosphere art.klad color=#9aa8b8 top=#d8e0e8 density=0.5 blur=1 desaturate=0.15
  klad atmosphere art.klad --off

Usage: klad atmosphere [OPTIONS] <BUNDLE> [PROPS]...

Arguments:
  <BUNDLE>


  [PROPS]...


Options:
      --off
          Remove the atmosphere

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad timeline

```text
Show the timeline (fps, length, origin, clips) or set fps, length and origin.

  klad timeline art.klad
  klad timeline art.klad --fps 12 --frames 24
  klad timeline art.klad --origin 512,700      the canvas point a Godot export puts on the node's position
  klad timeline art.klad --origin centre       back to the middle of the canvas (the default)

Usage: klad timeline [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --fps <FPS>


      --frames <FRAMES>


      --origin <ORIGIN>
          x,y on the canvas, or centre

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad clip

```text
List, add, change, rename and remove clips: named frame ranges such as idle, run or attack. Exports turn each clip into an animation; a clip may have its own fps.

  klad clip art.klad                                        list them
  klad clip art.klad --add run --from 0 --to 11 --loop --fps 12
  klad clip art.klad --set run --to 15 --no-loop
  klad clip art.klad --set run --name sprint               rename
  klad clip art.klad --set run --fps doc                   use the document's fps again
  klad clip art.klad --rm run

Frames inserted or removed on every layer move and stretch clips; a clip ends where the timeline ends.

Usage: klad clip [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --add <ADD>
          Add a clip with this name

      --set <SET>
          Change the clip with this name

      --rm <RM>
          Remove the clip with this name

      --name <NAME>
          With --set: the clip's new name

      --from <FROM>
          First frame

      --to <TO>
          Last frame (included)

      --loop
          The clip loops

      --no-loop
          The clip plays once

      --fps <FPS>
          The clip's own fps, or doc for the document's

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad ls

```text
Layer tree with ids, kinds, contents and bounds at a frame, top layer first

Usage: klad ls [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --frame <FRAME>
          [default: 0]

      --json


  -h, --help
          Print help
```

## klad show

```text
One layer, cel, node, stroke or fill (<cel>:s<id>) in full (JSON)

Usage: klad show <BUNDLE> <ID>

Arguments:
  <BUNDLE>


  <ID>


Options:
  -h, --help
          Print help
```

## klad add

```text
Add a layer, cel or shape.

  klad add art.klad layer vector name=sky
  klad add art.klad rect --in l2 x=0 y=0 w=2048 h=900 fill=#1b2a41
  klad add art.klad ellipse --in l2 cx=500 cy=300 rx=80 ry=80 'fill={"radial":{"center":[500,300],"radius":80,"stops":[[0,"#fff6d5"],[1,"#f3c76a"]]}}'
  klad add art.klad cel --in l3 frame=4 hold=2

Values are JSON when they parse as JSON, otherwise strings. Dotted keys nest: transform.rotate=15, stroke.width=3.

Usage: klad add [OPTIONS] <BUNDLE> <WHAT> [REST]...

Arguments:
  <BUNDLE>


  <WHAT>
          layer, cel, mask, rect, ellipse, path, group, rays, image, scatter, repeat, mirror or text

  [REST]...
          For layers the kind first (paint, vector, group, bones, image with src=photo.jpg, or adjust and its type: hsl, levels, curves, balance, gradient-map, fill), then key=value pairs

Options:
      --in <INTO>
          Container: group layer, vector layer, cel or group node

      --above <ABOVE>
          Place just above this sibling

      --below <BELOW>
          Place just below this sibling

      --index <INDEX>
          Position in the container, 0 is the bottom

      --frame <FRAME>
          Frame whose cel receives new nodes (default 0)

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad set

```text
Change properties of a layer, cel, shape, stroke or fill. Values are JSON when they parse as JSON, otherwise strings; null removes a key. Dotted keys nest.

  klad set art.klad n4 fill=#ff0000 transform.rotate=15
  klad set art.klad l2 opacity=0.5 blend=multiply
  klad set art.klad c4:s12 color=#c84a3a size=6 brush=pencil
  klad set art.klad c4:s7 color=skin opacity=0.8

Strokes take color, size, opacity, flow, brush and taper ([in,out] or null); fills take color and opacity. Colours may be palette names.

Usage: klad set [OPTIONS] <BUNDLE> <ID> [PROPS]...

Arguments:
  <BUNDLE>


  <ID>


  [PROPS]...


Options:
      --frame <FRAME>
          Frame to render with --render

          [default: 0]

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad recolour

```text
Recolour strokes and fills together. The group's average colour moves to --to and every mark keeps its own difference from that average (in OKLab: lighter, darker, warmer, cooler), so painted strokes stay painted. One mark takes the colour as it is. One history step.

  klad recolour art.klad c9:s12 c9:s13 c9:s40 --to '#e3962f'
  klad recolour art.klad c9:s12 c9:s13 --to skin

Usage: klad recolour [OPTIONS] --to <TO> <BUNDLE> [IDS]...

Arguments:
  <BUNDLE>


  [IDS]...


Options:
      --to <TO>
          The colour: #rrggbb or a palette name

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad detach

```text
Detach eraser, smudge and blender strokes from the marks they are attached to. Give the modifier strokes, or marks to detach every modifier attached to them. A detached eraser erases whatever is under it again, and none of them move with other marks any more. One history step.

  klad detach art.klad c9:s40
  klad detach art.klad c9:s12 c9:s13

Usage: klad detach [OPTIONS] <BUNDLE> [IDS]...

Arguments:
  <BUNDLE>


  [IDS]...


Options:
      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad rm

```text
Remove layers, cels or nodes

Usage: klad rm [OPTIONS] <BUNDLE> [IDS]...

Arguments:
  <BUNDLE>


  [IDS]...


Options:
      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help
```

## klad transform

```text
Move, rotate or scale strokes and fills, addressed as <cel>:s<id> (klad strokes lists them). Rotation and scale turn about --pivot, by default the middle of the marks' bounds. Stroke sizes and tapers scale with them; fills move by whole pixels and are resampled when turned or scaled.

--layer moves a whole layer or group instead: the marks of every drawing on it and on the layers in it (--only <frame>: just the drawings shown at that frame), its shapes, and with every drawing also its bones, its weights and its poses' roots and targets, so a rigged character moves as one. Shapes scale evenly only, and a layer with keyed shapes can only be moved. The pivot defaults to the middle of everything moved.

  klad transform art.klad c4:s12 c4:s13 --move 40,-10
  klad transform art.klad c4:s12 --rotate 15 --scale 1.2
  klad transform art.klad c4:s3 c4:s5 --sx -1 --pivot 512,400
  klad transform art.klad --layer figure --move -120,0
  klad transform art.klad --layer sky --scale 1.1 --only 12

Also a transform op in klad apply: {"op":"transform","ids":["c4:s12"],"x":40,"y":-10,"rotate":15,"sx":1.2,"sy":1.2}, or for a layer {"op":"transform","layer":"figure","x":-120,"y":0,"only":12}.

Usage: klad transform [OPTIONS] <BUNDLE> [IDS]...

Arguments:
  <BUNDLE>


  [IDS]...


Options:
      --layer <LAYER>
          a whole layer or group, every drawing on it (with its bones and poses), instead of ids

      --only <ONLY>
          with --layer: only the drawings shown at this frame

      --move <OFFSET>
          dx,dy in px

      --rotate <ROTATE>
          Degrees, clockwise

      --scale <SCALE>
          Uniform scale

      --sx <SX>


      --sy <SY>


      --pivot <PIVOT>
          x,y to turn and scale about

      --flip <FLIP>
          Mirror: h (left-right) or v (up-down)

          [possible values: h, v]

      --frame <FRAME>
          Frame to render with --render

          [default: 0]

      --render
          Render a preview after the edit and print its path (longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad lasso

```text
Select the strokes and fills of a paint layer inside a polygon and move, turn, scale, flip or delete them in one step. Without --cut only whole marks move: strokes whose points are all inside and fills whose pixels are. With --cut every stroke or fill crossing the edge is split at the edge: the part outside stays where it is and the part inside moves with the rest, so nothing outside the lasso changes. Without a move, turn, scale, flip or --rm it only lists what is inside and what crosses.

  klad lasso art.klad l3 --polygon '100,100 400,120 380,400 90,380'
  klad lasso art.klad l3 --polygon '100,100 400,120 380,400 90,380' --cut --move 60,-20
  klad lasso art.klad l3 --polygon '...' --cut --rotate 15 --scale 1.2
  klad lasso art.klad l3 --polygon '...' --flip h
  klad lasso art.klad l3 --polygon '...' --cut --rm

Also a lasso op in klad apply: {"op":"lasso","layer":"l3","polygon":[[100,100],[400,120],[380,400]],"cut":true,"x":60,"y":-20} (or "rm":true).

Usage: klad lasso [OPTIONS] --polygon <POLYGON> <BUNDLE> <LAYER>

Arguments:
  <BUNDLE>


  <LAYER>


Options:
      --polygon <POLYGON>
          x,y pairs separated by spaces

      --cut
          Split strokes and fills crossing the edge and take the inside parts too

      --move <OFFSET>
          dx,dy in px

      --rotate <ROTATE>
          Degrees, clockwise

      --scale <SCALE>
          Uniform scale

      --sx <SX>


      --sy <SY>


      --pivot <PIVOT>
          x,y to turn, scale and flip about

      --flip <FLIP>
          Mirror: h (left-right) or v (up-down)

          [possible values: h, v]

      --rm
          Delete what the lasso holds

      --frame <FRAME>
          [default: 0]

      --render
          Render a preview after the edit and print its path (longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad path

```text
Without edit flags: list the points (anchors) of a path, rect or ellipse with their handles, numbered across subpaths. Coordinates are the shape's own, as in its d, before its transform. Any edit turns a rect or ellipse into a path that looks the same.

  klad path art.klad n7
  klad path art.klad n7 --move 3 412,230
  klad path art.klad n7 --nudge 3,4,5 0,-20
  klad path art.klad n7 --handle 3 out 430,210
  klad path art.klad n7 --insert 3,0.5      (split the segment after point 3)
  klad path art.klad n7 --add 418,236       (a point on the nearest segment)
  klad path art.klad n7 --rm 4,5 --corner 2 --smooth 6
  klad path art.klad n7 --simplify 1.5      (fewer, smooth points within 1.5 px)

As ops: {"op":"path","id":"n7","move":3,"to":[412,230]}, "move":[3,4],"by":[0,-20]; "handle":3,"out":[430,210]; "insert":3,"t":0.5; "add":[418,236]; "rm":[4,5]; "corner":2; "smooth":6; "simplify":1.5. A smooth point keeps its handles in line unless "free":true.

Usage: klad path [OPTIONS] <BUNDLE> <NODE>

Arguments:
  <BUNDLE>


  <NODE>


Options:
      --move <POINT> <X,Y>


      --nudge <POINTS> <DX,DY>


      --handle <POINT> <in|out> <X,Y>


      --insert <POINT[,T]>


      --add <X,Y>


      --rm <POINTS>


      --corner <POINTS>


      --smooth <POINTS>


      --simplify <PX>


      --convert
          Turn a rect or ellipse into a path without other changes

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad mv

```text
Move a layer or node: --above, --below, --in, --index

Usage: klad mv [OPTIONS] <BUNDLE> <ID>

Arguments:
  <BUNDLE>


  <ID>


Options:
      --in <INTO>
          Container: group layer, vector layer, cel or group node

      --above <ABOVE>
          Place just above this sibling

      --below <BELOW>
          Place just below this sibling

      --index <INDEX>
          Position in the container, 0 is the bottom

      --frame <FRAME>
          Frame whose cel receives new nodes (default 0)

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help
```

## klad apply

```text
Apply many edits from JSON lines in one transaction: all apply or none.

  {"op":"add","layer":{"kind":"vector","name":"sky"}}
  {"op":"add","node":{"type":"rect","x":0,"y":0,"w":100,"h":50,"fill":"#336699"},"in":"l1"}
  {"op":"set","id":"n2","fill":"#ff0000","transform.rotate":15}
  {"op":"rm","id":"n2"}
  {"op":"mv","id":"l1","below":"l3"}
  {"op":"add","cel":{"frame":4,"hold":2},"in":"l3"}

New ids are printed in order.

Usage: klad apply [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --file <FILE>


      --frame <FRAME>
          Frame to render with --render

          [default: 0]

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad render

```text
Render a frame to PNG and print the path

Usage: klad render [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
  -o, --out <OUT>


      --frame <FRAME>
          [default: 0]

      --scale <SCALE>
          Scale (default 1, or longest side 1024 px with --focus)

      --region <REGION>
          Only this canvas region: x,y,w,h

      --focus <FOCUS>
          Zoom to a layer, cel or node, with a margin

      --overlay <OVERLAY>
          Draw on top: ids, bbox, grid, onion, parts, arcs or all (comma separated); onion shows the drawings before (red) and after (green) under each animated layer, parts boxes each named part, arcs the paths of --track

      --track <TRACK>
          With --overlay arcs (implied): a mark (c4:s12), a part (leaf, or lines:leaf) or a point (x,y) whose path across the clip or sheet is drawn; repeat for more

      --depth <DEPTH>
          How deep into groups the overlay labels go

          [default: 1]

      --no-cache
          Replay every stroke instead of using cached tiles

      --references
          Show reference layers (photos to trace over), which renders leave out

      --onion <BEFORE,AFTER>
          Onion skin: how many drawings (or frames) before and after (default 2,2)

      --onion-by <drawings|frames>
          Count the onion skin in drawings (default) or in frames

      --onion-layers <LAYERS>
          Layers that show onion skin (ids or names; default: every layer with more than one drawing)

      --no-tint
          Onion skin without the red and green tint

      --onion-loop <FROM-TO>
          Treat these frames as a loop: the onion skin wraps around inside them

      --sheet <all|FROM-TO|CLIP>
          Render many frames in one grid, labelled with frame numbers, drawing ids and holds

      --drawings
          With --sheet: one cell per drawing (frames that look the same share a cell)

      --cell <CELL>
          With --sheet: the longest side of each cell in px

          [default: 256]

      --columns <COLUMNS>
          With --sheet: cells per row (default: about square)

  -h, --help
          Print help
```

## klad stroke

```text
Paint with a brush along an SVG path (M, L, H, V, C, S, Q, T, Z; several subpaths make several strokes) or explicit points.

  klad stroke art.klad l3 --brush ink --size 14 --color #1b1b1b --path 'M100 400 C 300 200 500 600 700 400' --pressure 0.1,1,0.05
  klad stroke art.klad l3 --brush ink --size 6 --taper 12,30 --path 'M100 100 Q 200 40 300 120'
  klad stroke art.klad l3 --points '10,10,0.5 40,30,1 80,35,0.2'

Pressure is spread evenly along the stroke. --taper thins the first and last px of the line whatever the pressure. Brushes are a bundle brush id or a library brush (klad brushes). Strokes are marked as agent strokes.

Usage: klad stroke [OPTIONS] <BUNDLE> <LAYER>

Arguments:
  <BUNDLE>


  <LAYER>


Options:
      --path <PATH>


      --points <POINTS>
          x,y,pressure triples separated by spaces

      --brush <BRUSH>
          [default: ink]

      --color <COLOR>
          [default: #000000]

      --size <SIZE>
          [default: 8]

      --opacity <OPACITY>
          [default: 1]

      --flow <FLOW>
          [default: 1]

      --pressure <PRESSURE>
          Pressure along the stroke, for example 0.2,1,0.1

      --speed <SPEED>
          Pen speed in px/s, sets sample spacing and timing

          [default: 700]

      --seed <SEED>


      --taper <TAPER>
          Taper in and out by length in px, for example 12,30 (default: the brush's)

      --frame <FRAME>
          [default: 0]

      --part <PART>
          Put the new marks in this part of the drawing (see klad part)

      --second <SECOND>
          The second colour, for brushes that mix towards it with pressure or along the stroke

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad fill

```text
Fill a region of a paint layer. A flood fill starts at --at and spreads over pixels within --tolerance of that pixel in the reference: this layer (default), all (everything visible) or a layer id or name. --gap closes openings in the lines up to that many px wide; --expand grows the fill under the lines by that many px. --polygon fills a lasso polygon instead. The fill is stored with its coverage, so it renders the same later; klad refill recomputes it after the lines change.

  klad fill art.klad l2 --at 820,640 --ref lines --gap 4 --color '#c84a3a'
  klad fill art.klad l2 --at 820,640 --ref all --color skin
  klad fill art.klad l2 --polygon '100,100 400,120 260,380' --color '#20304a'

Colours are #rrggbb, #rrggbbaa or a palette name. Also a fill op in klad apply: {"op":"fill","layer":"l2","at":[820,640],"ref":"lines","gap":4,"color":"#c84a3a"}

Usage: klad fill [OPTIONS] <BUNDLE> <LAYER>

Arguments:
  <BUNDLE>


  <LAYER>


Options:
      --at <AT>
          Seed point x,y for a flood fill

      --polygon <POLYGON>
          Lasso polygon: x,y pairs separated by spaces

      --ref <REFERENCE>
          this (default), all, or a layer id or name

      --tolerance <TOLERANCE>
          0..1, how different a pixel may be from the seed

          [default: 0.1]

      --gap <GAP>
          Close openings in the lines up to this many px

          [default: 0]

      --expand <EXPAND>
          Grow the fill under the lines by this many px

          [default: 2]

      --color <COLOR>
          [default: #000000]

      --opacity <OPACITY>
          [default: 1]

      --frame <FRAME>
          [default: 0]

      --like <LIKE>
          Carry the flood fills of this drawing (a cel of the layer) to the layer's other drawings

      --frames <FRAMES>
          With --like: the frames whose drawings get them: all, FROM-TO or a clip

      --shape <SHAPE>
          Fill exactly the area of a vector shape (a group: each closed shape in it), grown by --expand

      --part <PART>
          Put the new marks in this part of the drawing (see klad part)

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad ink

```text
Ink the outlines of vector shapes (rects, ellipses, paths, groups of them; often on a hidden sketch layer) as strokes on a paint layer. An outline is split into separate strokes at corners sharper than --break degrees, each stroke tapers in and out, and its weight follows rules instead of pressure written by hand: heavier on the side facing away from the light, heavier on the silhouette (the outer edge of the shapes inked together) than on inner lines, thinner along tight curves, with a small seeded wobble.

  klad ink art.klad lines --from body,eye,leg --weight 6 --light -1,-1
  klad ink art.klad lines --from rock --weight 9 --break 45 --gap 3 --wobble 1
  klad ink art.klad lines --path "M10 10 C 80 0 120 60 200 40" --weight 4

The strokes are ordinary agent strokes: klad set, klad transform and the editor edit them. Also an ink op: {"op":"ink","layer":"l3","from":["n5"],"weight":6,"light":[-1,-1],"break":60,"gap":0,"wobble":0.6}.

Usage: klad ink [OPTIONS] <BUNDLE> <LAYER>

Arguments:
  <BUNDLE>


  <LAYER>


Options:
      --from <FROM>
          Vector shape ids; a group inks every shape in it

      --path <PATH>
          SVG path data to ink instead of shapes

      --weight <WEIGHT>
          The full line weight in px

          [default: 6]

      --light <LIGHT>
          Where the light comes from, x,y (-1,-1: top left)

      --break <BREAK_DEG>
          Split strokes at corners sharper than this many degrees

          [default: 60]

      --gap <GAP>
          Leave this many px open at the ends of each stroke

          [default: 0]

      --wobble <WOBBLE>
          Seeded wobble in px (0: none)

          [default: 0.6]

      --taper <TAPER>
          Taper in,out in px (default: the brush's)

      --brush <BRUSH>
          [default: ink]

      --color <COLOR>
          [default: #000000]

      --seed <SEED>
          [default: 1]

      --frame <FRAME>
          [default: 0]

      --part <PART>
          Put the new marks in this part of the drawing (see klad part)

      --hand <HAND>
          How much each stroke is drawn like a hand: its own pressure curve, tapers and width (0 gives even strokes)

          [default: 1]

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad light

```text
Make the two lighting layers of the flat-colour recipe from the shapes: on --shade, the side of each shape facing away from the light (the shape minus itself moved towards the light by --depth, in --soft overlapping steps so the inner edge is soft); on --rim, a thin band along the edge facing the light. Both are lasso fills with a cut, so with the layers clipped to the flats (multiply for shade, screen or add for rim) the light follows the shapes.

  klad light art.klad --from body,head --light -1,-1 --shade shade --rim rim
  klad light art.klad --from rock --shade shade --depth 24 --soft 4 --shade-color '#3a2f50'

Also a light op: {"op":"light","from":["n5"],"light":[-1,-1],"shade":"l5","rim":"l6","depth":14,"soft":3,"rimWidth":4}.

Usage: klad light [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --from <FROM>
          Vector shape ids; a group lights every closed shape in it

      --light <LIGHT>
          Where the light comes from, x,y (-1,-1: top left)

      --shade <SHADE>
          The paint layer for the shade

      --rim <RIM>
          The paint layer for the rim light

      --depth <DEPTH>
          How far the shade reaches into a shape, px

          [default: 14]

      --soft <SOFT>
          Overlapping shade steps (1: a hard edge)

          [default: 3]

      --rim-width <RIM_WIDTH>
          [default: 4]

      --shade-color <SHADE_COLOR>
          [default: #5a4a6a]

      --rim-color <RIM_COLOR>
          [default: #fff1cc]

      --shade-opacity <SHADE_OPACITY>
          [default: 0.6]

      --rim-opacity <RIM_OPACITY>
          [default: 0.85]

      --frame <FRAME>
          [default: 0]

      --lamp <LAMP>
          A lamp at x,y,height in canvas px: each shape is shaded away from it

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad hatch

```text
Fill the area of closed vector shapes (or a polygon) with short tapered strokes, parallel at --angle or following the outline (--along), clipped to the area, with seeded variation.

  klad hatch art.klad shade --in body --angle 35 --spacing 7 --length 10-24
  klad hatch art.klad lines --in trunk --along --spacing 9 --weight 1.5 --color '#3b2a1e'

Also a hatch op: {"op":"hatch","layer":"l5","in":["n5"],"angle":30,"spacing":6,"length":[12,30],"jitter":0.3,"weight":2}.

Usage: klad hatch [OPTIONS] <BUNDLE> <LAYER>

Arguments:
  <BUNDLE>


  <LAYER>


Options:
      --in <WITHIN>
          Closed vector shapes to hatch inside

      --polygon <POLYGON>
          Or a polygon: x,y pairs separated by spaces

      --angle <ANGLE>
          [default: 30]

      --along
          Follow the outline instead of parallel lines

      --spacing <SPACING>
          [default: 6]

      --length <LENGTH>
          Stroke lengths MIN-MAX px

          [default: 12-30]

      --jitter <JITTER>
          [default: 0.3]

      --weight <WEIGHT>
          Stroke size in px

          [default: 2]

      --brush <BRUSH>
          [default: ink]

      --color <COLOR>
          [default: #000000]

      --opacity <OPACITY>
          [default: 1]

      --seed <SEED>
          [default: 1]

      --frame <FRAME>
          [default: 0]

      --part <PART>
          Put the new marks in this part of the drawing (see klad part)

      --hand <HAND>
          How much each stroke is drawn like a hand: its own pressure curve, tapers and width (0 gives even strokes)

          [default: 1]

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad paint

```text
Paint closed vector shapes (or a polygon) as forms. --form round bulges any closed shape like a pillow (its height grows with the distance to its outline), flat is a plane facing --facing x,y,z, cylinder is round across --axis degrees. The light model: a key light from --light x,y at --elevation (or a --lamp x,y,height in canvas px; the shape nearest the lamp gets the full key light and farther ones less, by inverse square), cool fill from above (--fill, --fill-strength), warm bounce from below (--bounce, --bounce-strength) and a darker core just past the terminator (--core). --color is the local colour in full light.

Strokes follow the form: along bands of equal light on round forms, along the axis on cylinders, at --angle on planes. Each takes its colour from the model where it lies and stops where the light changes, so no stroke carries the lit colour into the shadow. Passes go from 2x --weight down to half of it (the small ones only where the form turns), then a decisive pass with --bold in the lights and a few quiet strokes in the shadows (--decisive 0 skips both). Every stroke is drawn like a hand (--hand): its own pressure curve, tapers and width.

Paint on a layer clipped to the flats, or lock the flats layer and paint into it: then strokes run past the outline and a pass of short strokes along it cuts the edge in, so no flat colour shows at the rim. On a plain layer strokes stay inside the outline.

  klad paint art.klad paint --in rock --color '#8a8f96' --light -1,-1
  klad paint art.klad paint --in pillar --form cylinder --axis 90 --color '#c9b8a0' --lamp 200,600,150
  klad paint art.klad paint --in wall --form flat --facing 0,0,1 --angle 0 --weight 10

Also a paint op: {"op":"paint","layer":"l5","in":["n5"],"color":"#5d6a78","light":[-1,-1],"weight":14,"form":"round"}.

Usage: klad paint [OPTIONS] <BUNDLE> <LAYER>

Arguments:
  <BUNDLE>


  <LAYER>


Options:
      --in <WITHIN>
          Closed vector shapes to paint inside

      --polygon <POLYGON>
          Or a polygon: x,y pairs separated by spaces

      --brush <BRUSH>
          [default: oil]

      --bold <BOLD>
          The brush for the decisive strokes in the lights

          [default: oil-load]

      --color <COLOR>
          The local colour in full light

          [default: #808080]

      --form <FORM>
          round, flat or cylinder

          [default: round]

      --axis <AXIS>
          A cylinder's axis in degrees (90 is vertical)

          [default: 90]

      --facing <FACING>
          A flat form's facing x,y,z (z toward the viewer, y down)

          [default: 0,0,1]

      --light <LIGHT>
          Where the light comes from, as a direction x,y

          [default: -1,-1]

      --elevation <ELEVATION>
          How far the light is toward the viewer (0 grazes the picture)

          [default: 0.6]

      --lamp <LAMP>
          A lamp at x,y,height in canvas px instead of a direction

      --key <KEY>
          The key light's colour

          [default: #fff3e2]

      --fill <FILL>
          The fill light's colour, from above

          [default: #8a9ab8]

      --fill-strength <FILL_STRENGTH>
          [default: 0.16]

      --bounce <BOUNCE>
          The bounce light's colour, from below

          [default: #b08a66]

      --bounce-strength <BOUNCE_STRENGTH>
          [default: 0.1]

      --core <CORE>
          How much darker the core just past the terminator is

          [default: 0.35]

      --shadow <SHADOW>
          The shadow side takes this colour's hue and chroma and keeps its own values (a colour or a palette name)

      --shadow-mix <SHADOW_MIX>
          How far the shadow side goes towards --shadow, 0 to 1

          [default: 0.7]

      --angle <ANGLE>
          Stroke direction on flat forms, degrees

          [default: 30]

      --weight <WEIGHT>
          Stroke size in px of the middle pass

          [default: 14]

      --decisive <DECISIVE>
          How many decisive and quiet strokes, 0 for none

          [default: 1]

      --vary <VARY>
          How much each stroke's value strays

          [default: 0.03]

      --hand <HAND>
          How much each stroke is drawn like a hand (0 gives even strokes)

          [default: 1]

      --opacity <OPACITY>
          [default: 1]

      --seed <SEED>
          [default: 1]

      --frame <FRAME>
          [default: 0]

      --part <PART>
          Put the new marks in this part of the drawing (see klad part)

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad prepare

```text
Replay every painted drawing shown in frames --from to --to (all frames by default) into the bundle's cache, so the editor and later renders read them instead of replaying their strokes. Run it after a big change made by the CLI, before the editor opens or plays the file.

  klad prepare art.klad
  klad prepare art.klad --from 0 --to 23

Usage: klad prepare [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --from <FROM>


      --to <TO>


  -h, --help
          Print help (see a summary with '-h')
```

## klad scatter

```text
Scatter copies of a small mark (SVG path data around 0,0) inside closed vector shapes (or a polygon), avoiding other shapes, each copy scaled and turned a little (seeded).

  klad scatter art.klad lines --in ground --mark "M0 0 L0 -18" --count 40 --size 0.7-1.2 --turn -15,15
  klad scatter art.klad lines --in rock --mark "M0 0 L6 3 L10 -2" --spacing 22 --avoid moss

Also a scatter op: {"op":"scatter","layer":"l3","in":["n5"],"mark":"M0 0 L0 -18","count":40,"size":[0.7,1.2],"turn":[-15,15],"avoid":["n9"]}.

Usage: klad scatter [OPTIONS] <BUNDLE> <LAYER>

Arguments:
  <BUNDLE>


  <LAYER>


Options:
      --in <WITHIN>
          Closed vector shapes to scatter inside

      --polygon <POLYGON>
          Or a polygon: x,y pairs separated by spaces

      --avoid <AVOID>
          Shapes to keep clear of

      --mark <MARK>
          [default: "M0 0 L0 -18"]

      --count <COUNT>
          How many marks (default: one every --spacing px)

      --spacing <SPACING>
          [default: 14]

      --size <SIZE>
          Scale of each copy MIN-MAX

          [default: 0.7-1.2]

      --turn <TURN>
          Turn of each copy MIN,MAX degrees

          [default: -15,15]

      --weight <WEIGHT>
          Stroke size in px

          [default: 2]

      --brush <BRUSH>
          [default: ink]

      --color <COLOR>
          [default: #000000]

      --opacity <OPACITY>
          [default: 1]

      --seed <SEED>
          [default: 1]

      --frame <FRAME>
          [default: 0]

      --part <PART>
          Put the new marks in this part of the drawing (see klad part)

      --hand <HAND>
          How much each stroke is drawn like a hand: its own pressure curve, tapers and width (0 gives even strokes)

          [default: 1]

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad foliage

```text
Build each closed shape out of leaves: leaf marks across the clump take their colour from a round form lit by --light (or a --lamp x,y,height), in a few value steps from --shade through --color to --lit (by default darker and cooler, lighter and warmer than --color), with the most variety in the light; then leaves along the outline, most on the side facing the light and the sky, a few glowing (--glow) at the shadow side's edge, so the silhouette breaks up and the sky shows between them. --mass names a shape around the whole crown, so every clump also turns with the big form. --leaf is the leaf size in px, --density how close the leaves sit. Fill a darker, slightly smaller core under each clump first and paint clumps back to front on one layer. The brush is leaves (several leaf shapes, one per dab).

  klad foliage tree.klad canopy --in clump1 --color '#4d7a3b' --light -0.6,-0.8 --leaf 16 --mass crown

Also a foliage op: {"op":"foliage","layer":"canopy","in":["clump1"],"color":"#4d7a3b","light":[-0.6,-0.8],"leaf":16,"mass":"crown"}.

Usage: klad foliage [OPTIONS] <BUNDLE> <LAYER>

Arguments:
  <BUNDLE>


  <LAYER>


Options:
      --in <WITHIN>
          Closed vector shapes, each a clump of leaves

      --color <COLOR>
          The leaves' middle colour

          [default: #4f7a3c]

      --lit <LIT>
          The lit leaves' colour (default: lighter and warmer)

      --shade <SHADE>
          The shadowed leaves' colour (default: darker and cooler)

      --glow <GLOW>
          Light through thin leaves at the shadow edge (default: a warm yellow-green)

      --light <LIGHT>
          [default: -1,-1]

      --lamp <LAMP>
          A lamp at x,y,height in canvas px instead of a direction

      --leaf <LEAF>
          Leaf size in px

          [default: 14]

      --density <DENSITY>
          How close the leaves sit (2 is twice as many)

          [default: 1]

      --mass <MASS>
          A shape around the whole crown, so each clump also turns with the big form

      --brush <BRUSH>
          [default: leaves]

      --edges-only
          Only the leaves along the outline, not across the clump

      --seed <SEED>
          [default: 1]

      --frame <FRAME>
          [default: 0]

      --part <PART>
          Put the new marks in this part of the drawing (see klad part)

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad bone

```text
Bones are a preview: these commands may still change in later versions, and drawings keep their rigs when they do.

Bones go on a bones layer (klad add <bundle> layer bones, or Layer > New bones layer), which draws nothing; klad attach rigs paint layers to them. A bone has a name, an optional parent, a head and a tail in canvas px; a child usually starts where its parent ends. --chain adds several bones at once, each the child of the one before and starting where it ends, through one more point than bones (the editor's bone tool draws chains the same way). Removing a bone removes its children (and the IK on them, its poses keeping their look). Moving a tail takes the heads of the children that start there along; --by moves a bone with all its children.

  klad bone art.klad body --add hip --head 400,500 --tail 400,420
  klad bone art.klad body --add thigh --parent hip --head 400,500 --tail 410,620
  klad bone art.klad body --chain thigh,shin,foot --through '400,500 410,620 405,740 440,745' --parent hip
  klad bone art.klad body --rename thigh --as femur
  klad bone art.klad body --move femur --tail 420,630
  klad bone art.klad body --move femur --by 12,-4
  klad bone art.klad body --reparent femur --parent pelvis
  klad bone art.klad body --rm femur

Also a bone op: {"op":"bone","layer":"body","add":"thigh","parent":"hip","head":[400,500],"tail":[410,620]}, or {"op":"bone","layer":"body","chain":["thigh","shin"],"through":[[400,500],[410,620],[405,740]],"parent":"hip"}.

Usage: klad bone [OPTIONS] <BUNDLE> <LAYER>

Arguments:
  <BUNDLE>


  <LAYER>


Options:
      --add <ADD>


      --rm <RM>


      --rename <RENAME>


      --as <TO_NAME>


      --move <MOVED>


      --reparent <REPARENT>


      --parent <PARENT>


      --head <HEAD>


      --tail <TAIL>


      --by <BY>
          dx,dy in px: with --move, the bone and its children

      --chain <CHAIN>
          names of a chain of new bones, comma separated: each the child of the one before

      --through <THROUGH>
          the chain's points, 'x,y x,y ...': one more than its bones

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad ik

```text
Bones are a preview: these commands may still change in later versions, and drawings keep their rigs when they do.

An IK turns bones so the end of a limb reaches a target, instead of turning each bone by hand. A limb (--chain 2, when the bone has a parent) turns the bone and its parent so the bone's tail lands on the target, the joint between them bending --bend clockwise or counterclockwise on screen (by default the way it bends in the rest drawing; flip turns it over); out of reach it points straight at the target. An aim (--chain 1) turns the bone to point at the target. --softness (px) eases the limb into full stretch so a knee does not snap straight: it starts slowing that far before full reach and is straight that far past it. Targets are set per pose (klad pose --target), in canvas px, and stay put when the root moves, so a foot stays planted while the body moves; a pose without a target for an IK leaves that limb to its turns. IKs are solved parent first: a foot's aim after its leg's limb.

  klad ik art.klad body --add shin
  klad ik art.klad body --add shin --chain 2 --bend counterclockwise --softness 8
  klad ik art.klad body --add foot --chain 1
  klad ik art.klad body --set shin --bend flip
  klad ik art.klad body --rm shin

Also an ik op: {"op":"ik","layer":"body","add":"shin","chain":2,"bend":"clockwise","softness":8}. Removing an IK (or its bone) keeps every pose as it looks, as turns.

Usage: klad ik [OPTIONS] <BUNDLE> <LAYER>

Arguments:
  <BUNDLE>


  <LAYER>


Options:
      --add <ADD>


      --set <SET>


      --rm <RM>


      --chain <CHAIN>
          2: a limb, the bone and its parent; 1: an aim, the bone alone

      --bend <BEND>
          which way the limb's joint bends on screen

          [possible values: clockwise, counterclockwise, flip]

      --softness <SOFTNESS>
          px: how far before full reach the limb starts to slow, so it does not snap straight

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad attach

```text
Bones are a preview: these commands may still change in later versions, and drawings keep their rigs when they do.

Bones live on bones layers (klad add <bundle> layer bones, or Layer > New bones layer); a bones layer draws nothing. Any paint layer in the drawing can be rigged to one: attached to a bone (--bone), it moves rigidly with that bone when posed, the way a cut-out arm follows its upper arm, so a character drawn in pieces (upper arm, forearm, hand on their own layers) needs no weights; set to bend (--bend), each of its points moves by the skeleton's weights, for a layer drawn in one piece across a joint. --off unrigs it: posing leaves it as it is. --skeleton names the bones layer when the drawing has more than one (with --bone, the one that has that bone is found).

  klad attach art.klad forearm --bone fore
  klad attach art.klad sleeve --bend --skeleton bones
  klad attach art.klad forearm --off

Also an attach op: {"op":"attach","layer":"forearm","bone":"fore"}; {"op":"attach","layer":"sleeve","skeleton":"bones"} bends; without either it unrigs. Renaming a bone keeps its layers on it; removing it unrigs them.

Usage: klad attach [OPTIONS] <BUNDLE> <LAYER>

Arguments:
  <BUNDLE>


  <LAYER>


Options:
      --bone <BONE>


      --bend
          bend with the skeleton by its weights, instead of following one bone

      --skeleton <SKELETON>
          the bones layer, when there is more than one

      --off
          unrig: posing leaves the layer as it is

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad solo

```text
Show only one layer: every other layer is hidden, the groups it is in stay shown and the layers inside it keep their own visibility, in one step that klad undo takes back. The editor's alt click on a layer's eye does the same.

  klad solo art.klad lines

Usage: klad solo [OPTIONS] <BUNDLE> <LAYER>

Arguments:
  <BUNDLE>


  <LAYER>


Options:
      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad bones

```text
List the skeletons, their bones and weights, and the posed drawings

Usage: klad bones [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --json


  -h, --help
          Print help
```

## klad weights

```text
Every bone has a weight map over the rest drawing (a grid at a quarter of the canvas resolution); where weights blend, lines bend smoothly; a part held by one bone at weight 1 moves rigidly. --auto starts from the bones (by distance, blending across the joints). --set paints soft round dabs of a weight onto --bone: 'x,y,radius,weight ...'. --part makes a named part of the drawing (at --frame) fully that bone's (and no other's). --smooth blurs the maps a little. render --overlay weights:<bone> shows a bone's map.

  klad weights art.klad body --auto
  klad weights art.klad body --bone shin --set '400,600,30,1 410,640,20,0.8'
  klad weights art.klad body --bone head --part head

Also a weights op: {"op":"weights","layer":"body","bone":"shin","set":[[400,600,30,1]]}.

Usage: klad weights [OPTIONS] <BUNDLE> <LAYER>

Arguments:
  <BUNDLE>


  <LAYER>


Options:
      --auto


      --bone <BONE>


      --set <SET>


      --part <PART>


      --value <VALUE>
          [default: 1]

      --smooth


      --frame <FRAME>
          [default: 0]

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad pose

```text
Bones are a preview: these commands may still change in later versions, and drawings keep their rigs when they do.

A pose is a turn per bone (degrees from its rest angle, children follow their parent), a move of the whole (--root), and a target per IK (--target, canvas px; see klad ik): the IK's limb turns so its end reaches the target, and the target stays put when the root moves. --mix blends an IK's limb from its turns (0) to reaching the target (1, the default). Every paint layer rigged to the skeleton (klad attach) gets a new drawing at --to: a layer attached to a bone moves with that bone alone; in a layer set to bend each stroke point and fill moves by its bones' weighted blend, the same way Godot's Skeleton2D bends a Polygon2D. The pose is kept on the drawing; posing that frame again re-poses it from the same rest drawing, and posing from a posed drawing starts from its rest drawing. klad inbetween between two poses of one rest drawing turns the bones instead of matching strokes, and moves the targets in straight lines, so a foot planted in both keys stays planted.

  klad pose art.klad body --from 0 --to 4 --bone thigh=25 --bone shin=-40 --root 0,6
  klad pose art.klad body --from 0 --to 8 --target shin=410,740 --root 20,6
  klad pose art.klad body --from 0 --to 12 --target shin=430,700 --mix shin=0.5 --bone shin=-20

Also a pose op: {"op":"pose","layer":"body","from":0,"to":4,"bones":{"thigh":25,"shin":-40},"root":[0,6],"ik":{"shin":{"target":[410,740],"mix":1}}}.

Usage: klad pose [OPTIONS] --to <TO> <BUNDLE> <LAYER>

Arguments:
  <BUNDLE>


  <LAYER>


Options:
      --from <FROM>
          [default: 0]

      --to <TO>


      --bone <BONES>
          name=degrees; repeat for more

      --root <ROOT>
          [default: 0,0]

      --target <TARGETS>
          bone=x,y: where the end of that bone's IK reaches, in canvas px; repeat for more

      --mix <MIXES>
          bone=0..1: how far that IK pulls its limb from the turns to the target (default 1)

      --hold <HOLD>


      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad key

```text
Keys animate a layer, a group or a vector shape without a drawing per frame: x and y (px), rotate (degrees), sx, sy or scale, and opacity, each with its own keys. Between two keys the value follows the first key's ease: linear, in, out, in-out, step, or a curve x1,y1,x2,y2 (as in CSS). Before the first key and after the last the value holds. A layer or group turns and scales about the canvas centre; a shape about its own pivot. --rm removes the keys at that frame (only the named properties, or all).

  klad key art.klad lantern --frame 0 y=0
  klad key art.klad lantern --frame 12 y=-14 --ease in-out
  klad key art.klad glow --frame 6 opacity=0.4
  klad key art.klad lantern --frame 12 --rm

Also a key op: {"op":"key","id":"lantern","frame":12,"y":-14,"ease":"in-out"}.

Usage: klad key [OPTIONS] --frame <FRAME> <BUNDLE> <ID> [VALUES]...

Arguments:
  <BUNDLE>


  <ID>


  [VALUES]...
          prop=value: x, y, rotate, sx, sy, scale, opacity

Options:
      --frame <FRAME>


      --ease <EASE>
          [default: linear]

      --rm


      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad keys

```text
List the keys of every keyed layer, group and shape (or one)

Usage: klad keys [OPTIONS] <BUNDLE> [ID]

Arguments:
  <BUNDLE>


  [ID]


Options:
      --json


  -h, --help
          Print help
```

## klad compare

```text
Render the drawing (or read a PNG or JPEG) at the reference's size and compare the two in OKLab: the value range (L at the 5th, 50th and 95th percentile), mean chroma, stroke texture (mean absolute Laplacian of L), edge sharpness (99th percentile of the gradient) and the mean colour difference, with what differs in words (lighter, bluer, busier, softer edges). --region adds a row per named rectangle; --heat writes a picture of where they differ most (black, red, yellow, white as the difference grows to 20).

  klad compare cafe.klad reference.png --region shade=40,300,200,120 --region cup=610,180,90,120 --heat diff.png

Usage: klad compare [OPTIONS] <DRAWING> <REFERENCE>

Arguments:
  <DRAWING>


  <REFERENCE>


Options:
      --frame <FRAME>
          [default: 0]

      --region <REGIONS>
          name=x,y,w,h in the reference's pixels; repeat for more

      --heat <HEAT>


      --json


  -h, --help
          Print help (see a summary with '-h')
```

## klad paintover

```text
Repaint what is below a paint layer (or the layers in --from) with strokes on that layer: big brushes first, then smaller ones only where the painting still differs from the source by more than --threshold (a colour difference in OKLab, x100), so detail lands where the source has it. Each stroke takes the source's colour where it starts, broken a little warm and cool (--vary), and runs along the forms (across the colour gradient) until the colour under it changes, it reaches --length (in brush widths, min,max) or it would bend more than --curve allows (0 straight, 1 follows every turn). --in limits it to closed shapes; --focus lets the smallest brush work only in and near one shape. The strokes are ordinary strokes in the part "paint over" (--part), one history step, so they take body and relief and can be edited.

For animation, paint over one key, then carry it to the next with --like <cel>: every stroke moves with the marks under it, takes its colour from the new drawing, and strokes are only added (part "paint over added at <frame>") where the new drawing needs them. The carried strokes keep their order, so klad inbetween pairs them part by part and nothing boils; the added strokes belong to one drawing each, so in-betweens leave them unpaired: use --unpaired fade to fade them in and out.

  klad paintover still.klad brushwork --sizes 32,16,8 --threshold 6
  klad paintover walk.klad brushwork --frame 4 --like c12

Also a paintover op, alone in its batch: {"op":"paintover","layer":"brushwork","sizes":[32,16,8],"threshold":6}.

Usage: klad paintover [OPTIONS] <BUNDLE> <LAYER>

Arguments:
  <BUNDLE>


  <LAYER>


Options:
      --from <FROM>
          Layers to paint from (default: everything below the layer, with the background)

      --in <WITHIN>
          Closed vector shapes to paint inside

      --sizes <SIZES>
          Brush sizes in px, biggest first

          [default: 32 16 8]

      --threshold <THRESHOLD>
          How far the painting may differ from the source (OKLab difference x100)

          [default: 6]

      --length <LENGTH>
          Stroke length in brush widths: min,max

          [default: 1,8]

      --curve <CURVE>
          How closely strokes follow the forms' turns (0 to 1)

          [default: 0.5]

      --brush <BRUSH>
          [default: oil]

      --focus <FOCUS>
          A closed shape: the smallest brush only paints in and near it

      --vary <VARY>
          Warm and cool, light and dark break per stroke (OKLab)

          [default: 0.025]

      --like <LIKE>
          Carry the paint-over of this drawing (a cel id on the same layer) to this frame

      --frames <FRAMES>
          With --like: carry it from drawing to drawing over these frames (a-b), making a drawing on the layer wherever the source starts one

      --part <PART>
          [default: "paint over"]

      --seed <SEED>
          [default: 1]

      --frame <FRAME>
          [default: 0]

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad grass

```text
Fill closed shapes with grass blades from the far (top) edge to the near (bottom) one: each blade rises from its root, leans with --wind (degrees) and bends, and is smaller and denser towards the top (--far is the far size against the near one). The ground under them rolls in soft patches about --patch px wide, lit by --light (or a --lamp x,y,height): each blade runs from a dark root to a lit tip in a few value families from --shade through --color to --lit. Along the top edge taller blades (--edge) break the outline against what is behind. Near blades are drawn last.

With a stroke brush (blade, the default, or any round or chalk tip) each blade is a tapered stroke whose colour runs from --color at the root to the lit colour at the tip when the brush blends to a second colour by pressure. With a picture brush (a tip made of pictures, like your own tufts) each blade is one stamp of the picture, turned to the blade. Build your own brush for your grass (klad brushes --picture, --set) and pass it with --brush: the op decides where the marks go and how they are lit, the brush decides what they look like.

  klad grass meadow.klad field --in field-shape --color '#6f8a3c' --light -0.9,-0.45 --blade 30 --far 0.22
  klad grass meadow.klad bank --in bank --brush my-tuft --blade 160 --density 0.1

Also a grass op: {"op":"grass","layer":"field","in":["field-shape"],"color":"#6f8a3c","blade":30,"far":0.22,"wind":12}.

Usage: klad grass [OPTIONS] <BUNDLE> <LAYER>

Arguments:
  <BUNDLE>


  <LAYER>


Options:
      --in <WITHIN>
          Closed vector shapes to grow grass in

      --color <COLOR>
          The grass's middle colour

          [default: #5f8a3e]

      --lit <LIT>
          The lit tips' colour (default: lighter and warmer)

      --shade <SHADE>
          The shadowed roots' colour (default: darker and cooler)

      --light <LIGHT>
          [default: -1,-1]

      --lamp <LAMP>
          A lamp at x,y,height in canvas px instead of a direction

      --blade <BLADE>
          Blade length in px at the near edge

          [default: 36]

      --width <WIDTH>
          Blade width in px at the near edge (default: an eighth of the length)

      --far <FAR>
          Size at the far edge against the near one

          [default: 0.45]

      --wind <WIND>
          How far the blades lean, degrees (negative leans left)

          [default: 8]

      --density <DENSITY>
          How close the blades stand (2 is twice as many)

          [default: 1]

      --edge <EDGE>
          How much taller the blades along the top edge are

          [default: 1.35]

      --patch <PATCH>
          Width of the ground's light patches in px (default: four blade lengths)

      --brush <BRUSH>
          [default: blade]

      --kinds <KINDS>
          The species, as JSON: [{"name":"fine","share":0.5,"width":0.7},{"brush":"my-clover","low":true,"share":0.2}] (default: a meadow mix of fine, broad, dry, seed heads and clover)

      --seed <SEED>
          [default: 1]

      --frame <FRAME>
          [default: 0]

      --part <PART>
          Put the new marks in this part of the drawing (see klad part)

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad gaps

```text
List the closed regions of a fill layer's reference (the lines its flood fills fill against) that no fill on the layer covers: pockets a fill missed, or drawings not coloured yet. Regions open to the canvas edge are the outside and do not count. Each pocket prints its bounds and size, and a render of the frame marks them in magenta with a box.

  klad gaps art.klad colour --frame 4
  klad gaps art.klad colour --sheet all
  klad gaps art.klad colour --sheet walk --ref lines --gap 4

The reference, gap and tolerance default to those of the layer's flood fills on those frames.

Usage: klad gaps [OPTIONS] <BUNDLE> <LAYER>

Arguments:
  <BUNDLE>


  <LAYER>


Options:
      --frame <FRAME>


      --sheet <all|FROM-TO|CLIP>
          Check many frames

      --ref <REFERENCE>
          this, all or a layer (default: what the layer's fills use)

      --gap <GAP>
          Close openings up to this many px (default: the fills' gap)

      --tolerance <TOLERANCE>


      --min <MIN>
          Ignore pockets smaller than this many px

          [default: 12]

      --render
          Render the frames even when nothing is found

      --json


  -h, --help
          Print help (see a summary with '-h')
```

## klad refill

```text
Recompute the flood fills of a paint cel after the lines they were made from changed

Usage: klad refill [OPTIONS] <BUNDLE> <LAYER>

Arguments:
  <BUNDLE>


  <LAYER>


Options:
      --frame <FRAME>
          [default: 0]

      --id <ID>
          Only this fill (klad strokes lists fill ids)

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help
```

## klad palette

```text
The document palette, kept in doc.json. Named colours can be used wherever a colour goes (--color skin).

  klad palette art.klad
  klad palette art.klad --add '#f0c8a0' --name skin
  klad palette art.klad --add '#d0a080' --name skin    (changes skin)
  klad palette art.klad --rm skin

As ops: {"op":"palette","add":"#f0c8a0","name":"skin"}, {"op":"palette","rm":"skin"}, {"op":"palette","set":[{"color":"#1b1b1b","name":"ink"}]}

Usage: klad palette [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --add <ADD>


      --name <NAME>


      --rm <RM>


      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad export

```text
Export a transparent PNG of the whole image or one layer or group, at a frame, at any scale from 1/16 to 8 (vectors stay sharp; paint layers are resampled). 8-bit PNGs are dithered from the 16-bit float pixels so gradients do not band; --depth 16 writes 16-bit PNGs. --background keeps the canvas background colour.

  klad export art.klad
  klad export art.klad --scale 2
  klad export art.klad --layer lines --scale 4 --depth 16 -o lines.png

Animations (each clip, or the whole timeline when there are no clips):

  klad export art.klad --sheet all [--scale 0.5] [--padding 2] [--max 4096]
      a PNG holding each distinct picture once, trimmed to its pixels (more pages, art-sheet-2.png ..., when it does not fit --max), plus art-sheet.json in Aseprite's array layout: one entry per exposure with its duration in ms, frameTags for clips
  klad export art.klad --sheet run            one clip; --sheet 0-11 a range
  klad export art.klad --frames run           numbered full-size PNGs, one per frame (run-0000.png ...) in art-run/
  klad export art.klad --godot ../game/art    the sheet, art.tres (SpriteFrames: one animation per clip with its fps, loop and per-frame durations) and art.tscn (an AnimatedSprite2D placed so the timeline's origin sits on the node's position). The first folder given is kept in the document (export.godot); later exports and the editor's button write there, and Godot uids are kept so scenes using them keep working. --store replaces a kept folder.

Usage: klad export [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --png
          PNG output (the default for a single image)

      --layer <LAYER>
          A layer or group id or name instead of the whole image

      --frame <FRAME>
          [default: 0]

      --scale <SCALE>
          [default: 1]

      --depth <DEPTH>
          8 (dithered) or 16 bits per channel

          [default: 8]

      --background
          Keep the canvas background instead of transparency

      --references
          Keep reference layers (photos to trace over), which exports leave out

      --sheet <SHEET>
          Sprite sheet and Aseprite JSON: all, a clip or a range like 0-11

      --frames <FRAMES>
          Numbered PNGs, one per frame: all, a clip or a range

      --godot [<GODOT>]
          Godot SpriteFrames and AnimatedSprite2D scene into this folder (default: the document's export.godot)

      --store
          Keep the --godot folder as the document's, replacing the one it has

      --godot-scene [<GODOT_SCENE>]
          A Godot parallax scene: each top-level layer a texture (with its fog) on a Parallax2D that scrolls by 1/(1+depth), into this folder (default: the document's export.godot)

      --godot-rig [<GODOT_RIG>]
          A Godot rig: the rest drawing as Polygon2Ds weighted to a Skeleton2D, and an AnimationPlayer with the poses of every clip, into this folder

      --padding <PADDING>
          Sheets: transparent pixels around each picture

          [default: 2]

      --max <MAX>
          Sheets: largest page side in px

          [default: 4096]

  -o, --out <OUT>


  -h, --help
          Print help (see a summary with '-h')
```

## klad brushes

```text
List brushes: the built-in library, and with a bundle the brushes it embeds (a document keeps every brush it uses in brushes/<id>.json, so it renders the same when the library changes).

  klad brushes
  klad brushes art.klad
  klad brushes art.klad --show ink

Usage: klad brushes [OPTIONS] [BUNDLE]

Arguments:
  [BUNDLE]


Options:
      --show <SHOW>
          Print one brush in full (JSON)

      --json


      --set <ID> <FILE>
          Add a brush to the bundle from a JSON file (- for stdin)

      --picture <NAME> <PNG>
          Add a picture (tip or texture) to the bundle's brushes/images

  -h, --help
          Print help (see a summary with '-h')
```

## klad effect

```text
Effects apply in order when the layer composes: blur (radius), glow (radius, strength, threshold, color, blend), outline (width, color, inside), shadow (x, y, radius, color), noise (amount, size, seed, mono), displace (amount, size, seed, by: a layer id whose red and green push the pixels). With no effect name it lists them.

Usage: klad effect [OPTIONS] <BUNDLE> <LAYER> [REST]...

Arguments:
  <BUNDLE>


  <LAYER>


  [REST]...
          blur, glow, outline, shadow, noise or displace, then key=value pairs

Options:
      --rm <RM>
          Remove the effect at this position (0 is the first)

      --clear
          Remove every effect

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help (see a summary with '-h')
```

## klad reshape

```text
Bend a brush stroke along a new path; it keeps its pressure, tilt and timing (klad strokes --json gives each stroke's fitted path)

Usage: klad reshape [OPTIONS] --path <PATH> <BUNDLE> <STROKE>

Arguments:
  <BUNDLE>


  <STROKE>
          The stroke, <cel>:s<id>

Options:
      --path <PATH>
          SVG path data the stroke follows from start to end

      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help
```

## klad undo

```text
Undo the last change (from the CLI or the editor)

Usage: klad undo [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help
```

## klad redo

```text
Redo the last undone change

Usage: klad redo [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --render
          Render a preview after the edit and print its path

      --scale <SCALE>
          Preview scale (default: longest side 1024 px)

      --json
          Print the result as JSON

      --overlay <OVERLAY>
          Draw on the preview: ids, bbox, grid or all (comma separated)

  -h, --help
          Print help
```

## klad log

```text
Recent changes, newest first

Usage: klad log [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --limit <LIMIT>
          [default: 20]

      --json


  -h, --help
          Print help
```

## klad selection

```text
What the human has selected in the editor (layer and shapes, with bounds)

Usage: klad selection [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --json


  -h, --help
          Print help
```

## klad notes

```text
Notes the human pinned on the canvas; --done <id> marks one as handled, --clear-done removes the handled ones

Usage: klad notes [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --done <DONE>


      --all
          Include notes already marked done

      --clear-done
          Remove every note marked done

      --json


  -h, --help
          Print help
```

## klad diff

```text
Before and after renders of a change, side by side (default: the last one)

Usage: klad diff [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --step <STEP>
          History step number from klad log

      --frame <FRAME>
          [default: 0]

      --scale <SCALE>


  -h, --help
          Print help
```

## klad info

```text
Paint cels with stroke counts and file sizes

Usage: klad info <BUNDLE>

Arguments:
  <BUNDLE>


Options:
  -h, --help
          Print help
```

## klad repair

```text
Give every layer, drawing or shape whose id is used twice (after a merge) a new id, moving its files along

Usage: klad repair [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --json


  -h, --help
          Print help
```

## klad strokes

```text
List the strokes and fills of a paint layer's cel at a frame

Usage: klad strokes [OPTIONS] <BUNDLE> <LAYER>

Arguments:
  <BUNDLE>


  <LAYER>


Options:
      --frame <FRAME>
          [default: 0]

      --after <AFTER>
          Only strokes with an id above this

          [default: 0]

      --json
          Print strokes as JSON with their points (x, y, pressure)

  -h, --help
          Print help
```

## klad hash

```text
Print pixel hashes, for comparing macOS and Linux renders

Usage: klad hash [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --frame <FRAME>
          [default: 0]

      --all
          One line per frame of the timeline

      --no-cache


  -h, --help
          Print help
```

## klad demo

```text
Write a sample bundle (vector sky and hills, painted glow, clouds, hatching, grass)

Usage: klad demo [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --size <SIZE>
          [default: 2048]

      --seed <SEED>
          [default: 1]

      --force
          Replace an existing bundle

  -h, --help
          Print help
```

## klad stress

```text
Create a large bundle of synthetic paint layers for editor performance tests

Usage: klad stress [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --size <SIZE>
          [default: 4096]

      --layers <LAYERS>
          [default: 30]

      --strokes <STROKES>
          [default: 300]

      --seed <SEED>
          [default: 1]

      --frames <FRAMES>
          Timeline length; a drawing per layer every --every frames

          [default: 1]

      --every <EVERY>
          [default: 2]

  -h, --help
          Print help
```

## klad scribble

```text
Append synthetic ink strokes to a paint layer (sync test)

Usage: klad scribble [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --layer <LAYER>


      --frame <FRAME>
          [default: 0]

      --count <COUNT>
          [default: 1]

      --seed <SEED>


      --size <SIZE>
          Brush size for every stroke (default: varied)

      --color <COLOR>
          [default: #bf2620]

  -h, --help
          Print help
```

## klad watch

```text
Print editor saves as they arrive, with how long they took to show up here

Usage: klad watch [OPTIONS] <BUNDLE>

Arguments:
  <BUNDLE>


Options:
      --poll
          Poll instead of native file events

      --seconds <SECONDS>
          [default: 120]

  -h, --help
          Print help
```

## klad licences

```text
Print the licence and the third-party notices (every open source part klad is built on)

Usage: klad licences

Options:
  -h, --help
          Print help
```

## klad install

```text
Put klad on the terminal's path, for you and for agents. macOS: a link in /usr/local/bin (asks for your password when that folder is not yours). Linux: a link in ~/.local/bin. Windows: this folder is added to your PATH. --dir puts the link in another folder.

  klad install
  klad install --dir ~/bin

Usage: klad install [OPTIONS]

Options:
      --dir <DIR>


  -h, --help
          Print help (see a summary with '-h')
```

## klad skill

```text
Print an agent skill, to hand to an agent as its instructions.

  klad skill            how an agent draws with klad: the commands, layers, brushes and traps (docs/skill/SKILL.md)
  klad skill drawing    the craft: planning a picture, light on form, values, edges, colour, materials, line, a painting method and a critique checklist (docs/skill/drawing/SKILL.md)

Both are under the MIT licence.

Usage: klad skill [WHICH]

Arguments:
  [WHICH]
          Which skill: klad (the default) or drawing

Options:
  -h, --help
          Print help (see a summary with '-h')
```

## klad samples

```text
List the sample drawings that come with klad (the editor's start window shows the same ones), or copy one into a folder to open, render and draw on; the copy is yours, the samples stay as they are. Without --into it goes in the current folder, as <name>.klad (or <name> 2.klad when that is taken). KLAD_SAMPLES points at another samples folder (one holding shipped.json).

  klad samples
  klad samples --copy lantern
  klad samples --copy rig --into ~/Drawings

Usage: klad samples [OPTIONS]

Options:
      --copy <COPY>
          The sample to copy, by name

      --into <INTO>
          The folder the copy goes in (default: the current folder)

      --json


  -h, --help
          Print help (see a summary with '-h')
```

## klad report

```text
Print a problem report to paste into a new issue on klad's GitHub page: klad's version, your system and the last lines of klad's log (errors the editor showed, and crashes), with your home folder written as ~. Nothing is sent. --drawing names the drawing's size and zips it next to itself, without its cache, to attach to the issue; the editor's Help > Report a problem does the same, with its last messages. KLAD_LOG points at another log file.

  klad report
  klad report --drawing art.klad

Usage: klad report [OPTIONS]

Options:
      --drawing <DRAWING>
          A drawing to zip next to itself and attach

  -h, --help
          Print help (see a summary with '-h')
```

## klad updates

```text
Check now whether a newer klad is out, on klad's releases page (the editor also checks once a day by itself, and its About window has check now). KLAD_UPDATE_FEED points it at another feed, a URL or a JSON file shaped like GitHub's latest release.

  klad updates

Usage: klad updates

Options:
  -h, --help
          Print help (see a summary with '-h')
```

## klad bench

```text
Measure stroke storage, brush speed, replay, cache and compositing

Usage: klad bench [OPTIONS]

Options:
      --strokes <STROKES>
          [default: 10000]

      --size <SIZE>
          [default: 4096]

      --seed <SEED>
          [default: 1]

  -h, --help
          Print help
```

## klad open

```text
Open the editor, on a bundle or on the start window (new, open, recent)

Usage: klad open [BUNDLE]

Arguments:
  [BUNDLE]


Options:
  -h, --help
          Print help
```
