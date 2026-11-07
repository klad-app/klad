# Filling

## Fill inside lines

Press G and click inside closed lines. The bar over the canvas sets what the fill looks at:

- **reference**: this layer, all visible layers, or a layer you pick. Flat colour usually goes on its own layer under the lines, filled with the lines layer as the reference, so the colour never covers the lines.
- **gap**: closes openings in the lines up to that many pixels wide.
- **tolerance**: how different a pixel may be from the one you clicked and still be filled.
- **grow**: how far the fill reaches under the lines, so no paper shows between line and colour.
- **opacity**.

klad keeps a fill as a fill, not as pixels mixed into the layer, so you can pick it up with S like a stroke, recolour it and delete it.

## Fill a shape you draw

With G, drag a loop instead of clicking: the area inside the loop is filled.

## Fill again after the lines changed

When you move or redraw lines, find "refill this drawing" with cmd K: it makes the drawing's fills again from where you clicked them. "show gaps" outlines the closed areas that no fill covers yet.

## Colour every frame of an animation

Fill the first drawing, select it and the frames after it in the timeline, and run "fill like the first drawing" from the timeline's selection bar. klad finds each fill's area in the other drawings by its shape and fills it there.
