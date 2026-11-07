# Layers, masks and clipping

## Add a layer

The buttons at the top of the Layers pane add a layer above the active one:

- **paint** for brush strokes and fills;
- **vector** for rectangles, ellipses, paths and text ([Shapes, paths and text](shapes-and-text.md));
- **group** to hold other layers;
- **adjust** for an adjustment layer: it starts as curves, and layer settings switch it to levels, hue, colour balance, a gradient map or a colour fill. It changes what is under it in its group.

The same pane duplicates the active layer, puts it in a new group and deletes it. cmd Z brings a deleted layer back.

## Pick, rename and reorder layers

Click a row to make that layer the active one. Double-click its name to rename it: Return keeps the new name, Esc leaves the old one. Drag a row to move the layer: drop it on the top or bottom half of another row to put it above or below, or on the middle of a group to put it inside.

## Show, hide and lock

The eye shows and hides a layer. Alt click an eye to see only that layer; alt click it again and everything comes back as it was. A locked layer can't be picked or painted on.

## Opacity and blend mode

The active layer's opacity and blend mode sit in the Layers pane's title row. Multiply is the usual one for shading, screen and add for light.

## Clip a layer to the one below

Tick "clip to layer below" in layer settings and the layer only shows where the layer under it has paint. This is how shading stays inside flat colour: flat colours on one layer, a multiply layer for the shadows and a layer for the rim light above it, both clipped.

## Paint only where there's paint already

"lock alpha" in layer settings makes new strokes and fills land only where the layer already has paint. A big soft stroke over line art then recolours just the lines. Turn it off again when you're done, or your next strokes on that layer will seem to vanish.

## Hide parts of a layer with a mask

"add mask" gives the layer an empty mask, and "add mask hiding all" one that hides everything. While you paint on the mask, your strokes hide the layer and the eraser shows it again, and what's hidden is tinted red so you can see it. The mask has a chip in the layer's row: click it to paint on the mask, and use "back to the layer" to paint on the layer itself again. Alt click the chip to turn the mask off and on.

## Trace over a picture

"import a picture" in the File menu (or dropping a PNG or JPEG on the window) places it on a new layer. Tick "reference" in its layer settings: you keep seeing it while you draw, but it stays out of exports.
