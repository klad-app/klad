# Samples

Drawings made from scratch with the `klad` CLI, each from a generator that prints ops as JSON lines into `klad apply`. They show how the pieces fit and give the tests and the parity checks something real to draw. `just samples` builds all of them (after `just build`); each `build.sh` also takes an output path, and refuses to overwrite a drawing that may hold edits unless `FORCE=1`.

| Sample | Shows |
|---|---|
| `lantern` | A prop blocked in as shapes, then inked, filled, lit and hatched (`PAINTED=1`: painted by form instead) |
| `ink-shapes` | The ink options side by side: weight from the light, breaks, gaps, wobble, tapers |
| `bounce` | Frame-by-frame timing: keys, holds, a clip and the exports |
| `creature` | A walk and an idle from keys with named parts, in-betweened, coloured from the first drawing, with arcs checked |
| `courier` | A game character drawn from shapes on every frame: a pose function moves the body, legs and lantern, then fill, light and ink make the drawing; an idle and a run as clips, exported to Godot |
| `ruin` | A game background lit by one lantern: stone blocks with chalk and dry-brush tool marks, relief per pillar facing the lamp, depth planes in fog, glow |
| `lemons` | Painting by form with `klad paint` on locked flats, and a shadow colour: lavender shadows instead of brown ones (`PLAIN=1`: without it; `RELIEF=1`: with a relief effect on every object, lighting the thickness of the oil strokes) |
| `sign` | Lettering in fonts that come with klad: a gilded inn sign on a painted board, the text clipped to the board with a shadow and grain over it |
| `tree` | Foliage that reads as leaves: clumps built from leaf marks with `klad foliage`, each lit as a form and turning with the whole crown, leaves breaking the outline, a bark trunk painted as a cylinder |
| `meadow` | Grass that reads as grass with `klad grass`: a field receding in soft light patches, a near bank whose blades break its crest, and tufts with seed heads from a brush the build script makes itself (pictures drawn in klad, added with `klad brushes --picture` and `--set`) |
| `paintover` | Brushwork from a picture: the lemons repainted with `klad paintover`, and the creature's walk painted over once and carried drawing to drawing with `--like` and `--frames`, so the strokes move with the creature and the background stays still |
| `rig` | Bones instead of a drawing per frame: a figure in a group with a skeleton, weights from the bones with the head, body and hand held rigidly by their parts, a wave posed at four keys and in-betweened on the bones, a lantern floating on keys, a far plane for parallax, and the Godot rig and parallax scene (`rig-godot/`) |
| `solids` | Painting stroke by stroke: a sphere, a cube and a cylinder built in 3D with `docs/skill/drawkit.py`, lit, painted with natural strokes |

The generated `.draw` bundles and pictures stay out of git.
