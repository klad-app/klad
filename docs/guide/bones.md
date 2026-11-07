# Posing with bones

Instead of redrawing a character on every frame, you can draw it once, give it a skeleton and pose it. The skeleton belongs to a layer or a group, and moves every layer in that group together.

![The rig sample's skeleton in the bone tool](../images/guide-bones.png)

## Draw a skeleton

1. Put the character's layers (lines, colour, shading) in a group and make the group the active layer.
2. Press J for the bone tool, with "draw" picked in the bar over the canvas.
3. Drag on empty canvas to draw the first bone. The chain carries on from its end: each click adds the next bone, and Esc ends the chain.

Drag a bone's end or middle to move it, and alt drag from a bone's end to start a child chain there. The skeleton list in the bar picks a bone by name, renames it (Return keeps the name) and changes the bone it hangs from.

## Paint how much each bone moves the drawing

Switch to "weights". "automatic weights" gives every bone a first set of weights by distance. Cmd click a bone to pick it, then paint its weight onto the drawing: size sets the brush's radius and weight how much each dab gives, and alt paints the weight away. "smooth" blurs every bone's weights a little. Hold rigid things, like a head or a hand, with full weight on one bone, so they don't bend.

## Pose it

Switch to "pose", go to a frame and drag a bone's end to turn it; its children follow. "make this pose a drawing" bends the drawing on this frame into the pose, and "back to rest" drops the turns, the move and the targets of this pose. Poses of the same drawing in-between on the bones ([Animating](animating.md)), so a limb swings around its joint instead of sliding.

## Let a limb reach (IK)

Pick the lower bone of a limb, say a shin, and click "make it a limb with" its parent. In pose mode, dragging the shin's end now turns both bones so the end reaches where you drag, bending the way the leg is drawn, or the other way if you switch it to bend clockwise or counterclockwise. Softness keeps the knee from snapping straight near full reach. "make it an aim" turns one bone alone to point at the target instead. For a pose where you'd rather turn the bones by hand, switch the limb off for that pose, or hold alt while you drag.

A planted foot is a target that stays where it is while the body moves, which is most of a walk cycle.
