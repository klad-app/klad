# Selecting and changing strokes

A stroke stays a stroke after you draw it: klad keeps its path, its pressure, its brush and its colour. So you can pick up any stroke later, yours or an agent's, and move it, recolour it or bend it, and klad paints it again.

![Every stroke of the lantern's lines selected, with the selection bar above them](../images/guide-selection.png)

## Move something

Press V, then press on what you want to move and drag. V picks whatever you see under the pen, a brush stroke or a vector shape, and makes its layer the active one. Alt drag drags a copy and leaves the original where it was. A drag that starts on empty canvas lassoes strokes on the active layer.

## Select strokes

Press S, then:

- click a stroke to select it, and shift click to add or remove more;
- drag a loop around strokes to lasso them;
- press cmd A for every stroke and fill of the active layer at this frame.

In the bar over the canvas, "this layer" and "all layers" decide whether a click picks only on the active layer or on whatever layer shows paint there. With "a lasso cuts strokes at its edge" on, a lasso also takes the parts of strokes that cross it, and the first change splits them there; with it off, it takes only strokes that lie wholly inside.

## Change the selection

The selection shows a box. Drag inside it to move, a corner to scale, an edge to stretch (past the opposite edge it flips) and the knob to turn. The arrow keys nudge it 1 px, or 10 px with shift.

The bar next to the selection flips it, turns it a quarter turn, recolours it with the current colour, makes the strokes thinner or thicker, sets their opacity, copies, pastes, duplicates in place (cmd D), deletes (Backspace) and drops the selection (Esc). Each of these is one step in the history.

## Copy and paste strokes

With strokes selected under S, cmd C keeps them and cmd V pastes copies into the active layer at the current frame, in the same place. On a frame with no drawing yet, the paste starts one.

## Work on several layers at once

In the Layers pane, cmd click or shift click other paint layers to add them to the active one; they get a blue edge. S then selects on all of them, and the selection moves, turns, recolours and deletes as one change, with every stroke staying on its own layer.

## Bend a stroke

Press A and click a stroke or a vector shape to show its points. Drag a point or a handle to change the path; a brush stroke keeps its pressure along the new path, so its taper and weight stay where they were. Alt click on the outline adds a point, alt click on a point switches it between a corner and a smooth point, and alt drag on a handle breaks it from its partner. Backspace removes the selected points. Layer settings also make points corners or smooth and simplify a path.

## Name parts

Select the strokes of one piece (a leaf, a leg), type a name under "part" in layer settings and click "make them this part". Parts are how in-betweens know which strokes belong together ([Animating](animating.md)).
