# Animating frame by frame

![The creature's walk in the timeline, with onion skin on](../images/guide-animation.png)

## Read the timeline

The timeline runs along the bottom of the window: a ruler with the playhead, then one row per layer, top layer first. Each drawing is a block that runs across the frames it shows on (blue on paint layers, green on vector layers). A purple block marked `->c4` is a link: it shows a drawing from earlier in the row again. Clips sit in a strip above the frame numbers.

Click a cell to go to that frame and layer, and drag on the ruler to scrub.

## Draw the next frame

- **shift K** makes a blank drawing right after the current one and takes you there. The "shift K on" buttons in the timeline header pick whether it holds for 1, 2 or 3 frames, for animating on ones, twos or threes.
- **K** makes a blank drawing on the frame you're on.
- Painting on an empty frame starts a drawing there too.

## Step through frames

- **,** and **.** go a frame back and forward; alt , and alt . jump to the previous and next drawing on the active layer; Home and End go to the first and last frame.
- Hold **Q** and move the pen sideways to flip through the drawings around this one, like the corner of a stack of paper. Let go to stay on the frame you reached.
- Hold **W** to see the previous drawing, and let go to come back.

## Onion skin

O turns onion skin on and off: the drawings before show in red and the ones after in green, under the active layer. The onion settings in the timeline header set how many to show before and after, whether to count drawings or frames, and their opacity, falloff and tint. The dot at the right end of a layer's name shows that layer's onion as well.

## Change the timing

- **=** and **-** make the drawing under the playhead hold a frame longer or shorter. Later drawings move along.
- Drag the right end of a block to change its hold, or drag the dot on its first cell to move it. Alt left and alt right move it a frame at a time.
- **I** inserts a frame in the active layer and **shift I** in every layer; the remove buttons next to them take one out.

## Reuse a drawing

"duplicate to the next frame" copies the drawing right after itself, with the same stroke ids, so you can change the copy into the next pose. "show a drawing here" links one of the row's drawings at this frame: drawing on either changes both. "unlink" makes a link a drawing of its own.

## Play it

Return plays and stops. It plays the loop range when there is one (shift drag on the ruler, or the loop button), otherwise the clip under the playhead, otherwise the whole timeline, at the fps in the timeline header. If frames drop, the lightning button ("prepare frames") buffers the range first; the strip along the ruler turns green where frames are ready to show at once.

## Make clips

A clip is a named range of frames, like idle, walk or attack, and an export turns each clip into its own animation. Drag along the empty strip above the frame numbers to make one. Click its bar to name it and set whether it loops, its own fps and its first and last frame.

## Retime frames

Shift click a cell, or shift drag over cells, to select frames on one or more rows. The selection bar retimes them on ones, twos or threes, fits them into a number of frames, reverses them or adds them again backwards as a ping-pong, on the selected rows or on every layer. It also copies cells and pastes them as copies or as links.

## Let klad draw the in-betweens

1. Draw two keys on a row with empty frames between them. Name the parts of each key (a leaf, the near leg) under "part" in layer settings ([Selecting](selecting.md)): in-betweens pair strokes part by part, so keys drawn separately still match up.
2. Click the first key's cell, then shift click the second key's cell to select both and the frames between them.
3. Click "in-between" in the selection bar. The window shows both keys and asks how many drawings to make (or one on every frame), how they're spaced, with the chart an animator draws between keys, and what to do with strokes that are only in one key: leave them out, pop them, fade them or draw them on.
4. Click "make the in-betweens".

In-betweens show lighter in the timeline and their ids carry a star. The window then lists any stroke that found no partner; click it, pick its partner in the other key and the in-betweens are made again with that pair. When you change a key later, "redo the in-betweens" remakes them, and keeps what you drew on top of any of them unless you say otherwise.
