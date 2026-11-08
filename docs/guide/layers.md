_Last updated: 2026-11-08 14:50_

# Layers, masks and clipping

Keep lines, colour and shading on their own layers, and decide where each one shows.

## Add a layer

At the top of the Layers pane, click **paint** for brush strokes or **vector** for [shapes and text](shapes-and-text.md). Drag a row to move the layer, and double click its name to rename it.

## Shade inside flat colour

1. Put the flat colour on one layer.
2. Add a paint layer above it and tick **clip to layer below** in layer settings.
3. Set the new layer's blend mode to **multiply** in the Layers pane's title row and paint the shadows. They stay inside the colour.

## Recolour line art

1. Select the lines layer and tick **lock alpha** in layer settings.
2. Paint over the lines with a big soft brush. Only the lines take the colour.
3. Untick **lock alpha** when you're done, or your next strokes on that layer won't show.

## Hide part of a layer

1. In layer settings, click **add mask**.
2. Paint where the layer should be hidden, and erase to bring it back. Hidden parts show red.
3. Click **back to the layer** to paint on the layer itself again.

## See one layer alone

Alt click its eye in the Layers pane. Alt click it again to bring the others back.

## Trace over a picture

Drop a PNG or JPEG on the window and tick **reference** in its layer settings. You keep seeing it while you draw, and exports leave it out.
