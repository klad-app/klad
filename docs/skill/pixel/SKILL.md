---
name: klad-pixel
license: MIT (LICENSE-MIT in the klad repository)
description: The craft of pixel art, for an agent that draws sprites, tiles and small animations. How to choose a size and a palette, how to build a sprite from its silhouette, how to keep lines clean, when to outline, how to make ramps with hue shift, how to shade without pillow shading or banding, how to anti-alias by hand, how to make tiles that repeat and animations that read, and a checklist to judge the result at its real size. Use it with the klad skill whenever you draw on a pixel layer, and before you critique pixel art.
---

# Pixel art

How to make a sprite look placed by hand, pixel by pixel, by someone who knows why each one is there. The klad skill (`klad skill`, its Pixel art part) is the tool: pixel layers, the palette, `klad pixel`, `klad pixels`, `klad check`. The drawing skill (`klad skill drawing`) is the craft that holds for every picture: one focal point, one light, values first. This page is what changes when the picture is 32 pixels wide.

What changes is that nothing is small enough to ignore. In a painting a stroke out of place is lost among thousands. In a 16 px sprite one pixel is a sixteenth of the width: it is an eye, a highlight or a mistake, and the viewer sees which. So pixel art is not a small painting. It is a few hundred decisions, and the work is to make every one of them on purpose.

## The order of work

1. **Size.** The smallest size that holds the idea. 16 px for an item or an icon, 16 to 32 for a character in a tile game, 48 to 64 for a hero or a portrait. Every doubling is four times the pixels to decide, and a large sprite drawn with small-sprite care looks empty. Size the drawing in pixels of the art (`klad new hero.klad --pixel 32x32`); enlarging is for the export.
2. **Palette.** Decide the colours before the pixels: a handful of ramps, three to five steps each, 8 to 16 colours for a sprite. Load one (`klad palette hero.klad --load pico-8`) or build ramps (`--ramp skin --base '#d99a6c' --steps 4`). Name the colours by what they are for (`skin`, `cloth`, `ink`), so a change to the palette changes the sprite.
3. **Silhouette.** Draw the whole figure as one flat colour and look at it at real size. If the filled shape does not say what it is, no shading will. Push what identifies it: a larger head, the hat, the weapon, the tail. Pose it off balance; a figure standing square is a box.
4. **Flat colour.** One colour per material, each as a few simple shapes.
5. **Light.** One direction for the whole sprite and for every sprite that will stand next to it; from the top left unless the game says otherwise. Put a shadow shape on the far side of each form and under each overhang, one ramp step down. Then the light shapes, one step up, smaller than the shadows.
6. **Outline,** or the choice not to have one (below).
7. **Clean up.** Lines, clusters, single pixels. Then anti-aliasing, last and sparingly.
8. **Check.** `klad check`, then real size, then on the background it will stand on, then in motion.

## Real size is the judge

You draw enlarged and the player sees it small, so what you see while working is not the picture. `klad render hero.klad --scale 1` is. Look at it after every stage, and look at it next to a neighbour: a second sprite, the tile under it. Two colours that are clearly different at eight times can be the same grey at one. If two neighbouring colours do not separate at real size, they are one colour and one of them is wasted; move their values apart or drop one.

## Clusters

A cluster is a group of touching pixels of one colour that reads as one shape. Pixel art is built from clusters, the way a painting is built from strokes.

- Make clusters simple and a little chunky: wedges, crescents, blocks with one cut corner. A shadow is a shape, not a row.
- A single pixel with no neighbour of its colour is noise unless it is a detail the viewer should find: an eye, a glint, a button. Decide which. `klad check` lists them as stray pixels.
- Avoid one-pixel strings of a colour running through a shape, and pixels scattered like salt. Both read as dirt.
- Every edge between two clusters is a line and wants the care of a line.

## Lines

A line of pixels is a sequence of runs: so many pixels straight, then a step. Its quality is in the runs.

- **No doubled corners.** Where a one-pixel line turns, the corner pixel that touches both neighbours makes an L, and the line looks thick there. Remove it, so each pixel touches the next at a side or at a corner, never both. A corner that is meant, the corner of a box, is the exception.
- **Straight lines** repeat one run: 1, 1, 1 for 45 degrees, 2, 2, 2 for a shallow slope, 3, 3, 3, and their upright twins. Mixed runs (2, 3, 2, 2, 3) are for angles that need them, and then the pattern still repeats evenly.
- **Curves** change their run lengths one way and then the other: 1, 1, 2, 3, 5, 3, 2, 1, 1. A run that is shorter than both its neighbours (4, 1, 4) is a jag: a step where the eye expects a sweep. Even it out.
- `klad pixel --path` and `--line` draw clean lines for you; fix by hand with `--cells` and `--erase`.

## Outlines

An outline is a choice with a price: on a 16 px sprite a full outline is two of sixteen pixels in each direction.

- **A dark outline all round** reads on any background. Use the darkest colour of the palette, not pure black, and let it take the hue of what it surrounds when you can afford the colours.
- **A coloured outline:** each stretch is a darker step of the fill it borders, and on the lit side it gets lighter or goes away. The sprite sits in its world instead of on it.
- **No outline:** for tiles, backgrounds and anything that will always stand on a known colour. Contrast with the background does the work.
- **Inner lines** only where two parts of the same colour overlap, an arm across a chest. Elsewhere let the change of colour be the edge.
- `klad pixel hero.klad l1 --color ink --outline` draws one round what the layer shows (`--inside` on its own edge pixels). Then break it where the light hits.

## Ramps and hue shift

A ramp is one material from dark to light. Darkening a colour by adding black, and lightening it by adding white, gives a ramp that looks like plastic in fog.

- **Shift the hue with the value.** Shadows lean cool, towards blue and violet. Lights lean warm, towards yellow. `klad palette --ramp` makes ramps that do this.
- **Saturation peaks in the middle.** The darkest and the lightest steps are duller than the mid-tones.
- **Steps you can see.** Each step clearly lighter than the last at real size, with about even gaps. Three to five steps per material; a small form uses two of them.
- **Share colours between ramps.** One dark for all the outlines and deepest shadows, one light for the brightest highlights; the shadow of skin can be the mid-tone of leather. Shared colours tie the sprite together and keep the palette small.
- **Every colour earns its place.** `klad strokes hero.klad l1 --entry <colour>` shows where a colour is used. A colour used for three pixels should be a neighbour's colour.
- Other colourings of the same sprite (teams, night, a hurt flash) are palette variants (`klad palette --new-variant`), which is one more reason to give each material its own palette entries.

## Shading

- **Shade the form, not the outline.** Pillow shading is light in the middle and darker towards every edge, whatever the light. It makes everything look like a cushion. Put the shadow on the side away from the light and under what overhangs: under the head on the chest, under the arm on the body.
- **No banding.** Bands of colour one pixel wide that follow each other, or follow the outline, look like contour lines on a map. Vary their width, let two steps meet at an angle, or merge them. `klad check` lists them.
- **Highlights are small** and belong to shiny things: one or two pixels on metal, an eye, wet skin. A highlight on cloth is a lighter plane, not a dot.
- **Dither with care.** A checker of two colours makes a third at a distance, and makes noise up close. Keep it for large areas and rough materials (`klad pixel --dither 50 --second`), and off characters that move, where it shimmers.
- **Detail is suggested.** Three pixels say brick; a full grid of mortar says graph paper. Put detail at the focal point, the face before the boots.

## Anti-aliasing by hand

A step in an edge can be softened with one pixel of a colour between the two that meet. That is the whole technique, and most sprites need little of it.

- Use it on curves and shallow diagonals inside the sprite, where two of its own colours meet.
- At the end of a run, extend the run with an in-between pixel. Long runs take two, short runs one, runs of one take none.
- Never on 45 degree lines, on straight horizontals and verticals, or on the outer edge against a background you do not know: there it makes a halo.
- The in-between colour comes from the palette. Usually a ramp step you have does it.
- Too much of it blurs the sprite, and a row of in-between pixels along an edge is banding. If in doubt, leave the step.

## Tiles

- **Edges have to meet.** Draw on a canvas that wraps (`klad new grass.klad --pixel 16 --wrap`) and look at it repeated (`klad render grass.klad --repeat 3`). In a tileset (`--tile 16`), read two neighbours side by side (`klad pixels ground.klad l1 --tile 0,0`, then `--tile 1,0`).
- **Nothing that counts.** One distinct pebble repeats every 16 px and the eye finds the grid. Spread small, similar features evenly, and make two or three versions of a tile to mix.
- **Quieter than what stands on it.** Less contrast and less saturation than the characters, so they read against it.
- **One light** for tiles and sprites alike.

## Animation

- **Few frames, strong poses.** An idle is two to four drawings, a walk four to six, an attack three to five: a pose that winds up, the strike, a pose that settles. Timing comes from how long each drawing holds, not from more drawings.
- **Keep the volume.** A limb that loses a pixel of width on one frame flickers. Move clusters whole (`klad lasso --box x,y,w,h --cut --move 1,0`) and then repair their edges.
- **Less than a pixel.** To move something by less than a pixel, move its shading instead of its outline: a highlight that shifts by one pixel makes a chest breathe.
- **One pixel is a lot.** At 16 px a one-pixel squash is a big gesture. Use it on purpose.
- **No jitter.** A pixel that changes between frames for no reason is seen. Compare each frame with the last (`klad render --frame 3 --overlay onion`), and build a frame from the one before: read it (`klad pixels hero.klad l1 --frame 0 --plain`), change the text, write it to the new frame (`klad pixel hero.klad l1 --frame 1 --grid -`).
- **Judge it moving,** at speed and at the size it will be seen: `klad export hero.klad --gif --scale 4`. Watch the loop where the last frame meets the first.

## What goes wrong, and why

- **Too many colours.** Soft gradients of many close steps are a painting shrunk down. Fewer, clearer steps.
- **Drawn enlarged, never looked at small.** Detail that only exists at eight times.
- **Mixed pixel sizes.** Every pixel on screen must be the same size. Export at whole-number scales (klad refuses others), rotate by quarter turns only, and redraw a sprite for any other angle instead of turning it in the engine.
- **A converted picture left as it is.** `klad pixelize` and `klad import picture.png` give a start: in-between colours along every edge, broken outlines, stray pixels. Redraw the outline, cut the colours, check it.
- **Symmetry left in.** `--mirror-x` is a good way to start a front view; then change one side a little.
- **Black outline, pure white highlight, primary colours.** Straight from the tube. Take them from the ramps.
- **Everything outlined, everything shaded, everything detailed.** Then nothing leads. Choose where the sprite is richest.

## Before you show it

Look at `klad render --scale 1`, and check in this order:

1. **Silhouette:** filled with one colour, does it say what it is?
2. **Read:** at real size, on its background, is the focal point the first thing seen?
3. **Palette:** how many colours, and is each one needed? Do neighbouring colours separate?
4. **Light:** one direction, shadows as shapes, no cushion, no contour lines.
5. **Lines:** runs that repeat or change smoothly, no doubled corners.
6. **`klad check`:** nothing to look at, or every finding is one you meant.
7. **Tiles:** repeated three times, is there a seam or a grid?
8. **Animation:** as a GIF at speed: does the volume hold, does the loop close, does anything flicker?

If you can name what would make it better, do that before you show it.
