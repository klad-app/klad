# Painting

![Painting the lemons with the oil brush](../images/guide-painting.png)

## Paint a stroke

Press B for the brush and paint on the canvas. The stroke goes into the active paint layer at the current frame; the status line says which drawing it lands in. On a vector layer the brush won't paint and tells you so: pick a paint layer in the Layers pane, or add one.

## Pick a brush

The bar over the canvas holds the brush picker, and the colour and brush pane shows the same brushes as tiles, each with a sample stroke. klad comes with 23 brushes and an eraser. Inking pen, Pencil and Marker are for line work. Oil, Loaded oil, Round bristle, Flat bristle and Dry brush are for painting. Wet round, Smudge and Blender mix and push the paint already on the layer, and Leaves, Grass and Splatter stamp little pictures along the stroke.

The mixing brushes only read the layer they paint on, so put the colours you want to mix on one layer.

## Change the size, opacity and flow

The bar has sliders for size, opacity and flow. [ and ] make the brush smaller and bigger. To size it right on the canvas, hold cmd alt and drag sideways: the circle shows the size, and it doubles for every 120 points you drag.

"more" opens hardness, the size at the lightest press, the pressure curve, the taper at both ends, a stabiliser and smoothing.

## Erase

E switches between the brush and the eraser, and the eraser end of the pen erases too. On a layer with lock alpha on, the eraser does nothing (see [Layers](layers.md)).

## Pick colours

- The hue ring and the square in the colour and brush pane pick the colour; the hex field and the alpha slider sit next to them.
- The palette belongs to the drawing. Click a swatch to use it, drag swatches to reorder them, and right-click one (or its x) to remove it. "add colour" adds the current one, with a name if you type one.
- The two squares under the tools in the strip are the current colour and the one before it. X, or a click on them, swaps them.
- Hold alt and click to pick the colour under the pointer. With shift alt it reads only the active layer.

## Make your own brush

![The brush editor over the canvas](../images/guide-brush.png)

F5, or "edit" in the brush bar, opens the brush editor with a live sample stroke at the top. From there you set:

- the tip: round, chalk or a picture, with its hardness, spacing, roundness and angle (fixed, or turning with the stroke or the pen's tilt), and jitter and scatter for each dab;
- a dual tip and a texture;
- how pressure and tilt change the size, flow and opacity, and the curve between the lightest and the heaviest press;
- colour dynamics;
- the kind of paint: dry, wet (picks up and mixes the colour under it), smudge or blend, and body.

Give it a name and click "save in this drawing". A drawing keeps every brush its strokes use, so it looks the same on any machine and the brush travels with it.

## Thick paint

Brushes with body, such as Oil and Loaded oil, lay paint with a height: ridges along the bristles and lips at the edges. The height only shows once the layer has a relief effect. In layer settings, add relief under the effects and click or drag on the light dial towards where the light comes from. Use a strong relief for a figure or fruit and a light one for a wall or a table.

## Paint in mirror

Turn on "mirror" in the brush bar to paint every stroke and its mirror image across an upright line at the same time. Both land as ordinary strokes, so you can change either one afterwards.
