# Changelog

## Unreleased

The first builds, and klad's licences.

- Drawings are `.klad` folders in a fresh format, `klad/3` (drawings from the first builds, `klad/1` and `klad/2`, open as they are). Drawings from before (`.draw` folders) do not open and are not converted.
- klad is source-available, not open source: the app is under the PolyForm Internal Use License 1.0.0, with permission for personal use, to propose contributions and to share the drawings you make (`LICENSE`); bought builds are under the same terms. The file format (`docs/format.md`), the command reference (`docs/cli.md`), the agent skill and the new `klad-client` crate are MIT (`LICENSE-MIT`), so any program or agent can read, write and make klad drawings. Contributions come in under a contributor licence agreement (`CLA.md`).
- `klad-client`: run klad from Rust, apply edits as JSON ops, read drawings back as JSON, render them.
- Builds for macOS (an app, Apple silicon and Intel in one) and Windows, sold on itch.io; Linux from source. Each has the `klad` command line, the editor, the fonts, the licences and the agent skill.
- The editor runs on Windows and Linux (`klad open`, or `klad-editor`), with the platform's open dialog; pens that report pressure through the system (Windows) draw with it.
- On a Mac, `.klad` drawings show in Finder as one item and open in klad with a double click.
- `klad install` (and "install the klad command" in the editor's document menu) puts `klad` on the terminal's path, for you and for agents. `klad skill` prints the agent skill.
- The editor checks once a day whether a newer version is out and says so, with a link to what changed (off in the settings window); About's "check now" and `klad updates` check on demand and say why a check failed.
- `klad --version`, About and the start window show the version and the commit a build was made from; `just build` counts the version up for every build.
- In-betweens blend colours evenly (in OKLab), so light can follow a turning limb when the second key is coloured for it.
- `klad arcs` follows the part of a foot on the ground: a heel-to-toe roll is no longer reported as a slide.
- Help > Report a problem shows the report before it is copied (version, system, the last messages and lines of klad's log, with your home folder hidden) and can zip the drawing to attach; `klad report` prints the same. Errors and crashes are kept in a small log for it.
- Messages say what happened and what to do next; when klad itself goes wrong it says so, and that your work is saved.
- Bones: a layer or group gets a skeleton (`klad bone`, the bone tool J), weights from the bones or painted (`klad weights`), and poses that bend the drawing (`klad pose`, pose mode); in-betweens of two poses turn the bones. The bone tool lists the skeleton, picks a bone in every mode, and moves ends, joints and whole bones with their children (`klad bone --move` with `--by`).
- Transform a whole layer or group: cmd T puts handles round it, and a drag moves, turns or scales every drawing on it, its shapes, and its bones and poses (`klad transform --layer`).
- Attach a layer to a bone and it moves with that bone alone, as a cut-out: a character drawn in pieces needs no weights (`klad attach`, the picked bone's layers menu, or the bone button on a layer's row). Layers not attached bend by the weights. Pose mode shows the drawing posed as you drag; "make this pose a drawing" keeps it.
- Bones are drawn as chains: letting go of a bone starts the next one from its end, each click adds one, shift fans them from one joint, Esc ends the chain (`klad bone --chain`).
- IK: a bone and its parent become a limb (or a bone alone an aim) that reaches for a target, bending the way it is drawn, with softness so knees do not snap straight (`klad ik`, the bone tool's options). In pose mode, drag a limb's end and the limb follows; move the whole by a root bone's head and planted feet stay where they are (`klad pose --target`, `--mix`); in-betweens keep them planted. Exports to Godot carry the solved turns.
- A first start: the start window welcomes, offers first steps (draw a shape, fill it, hand the drawing to an agent, ticked off as you go; Help > First steps opens them again) and six sample drawings that open as copies (`klad samples` lists and copies them).
- Picking strokes takes what you see: the topmost layer that shows paint under the pen and the mark covering the point, on all layers or this layer only (the S and V options).
- A selected shape takes the colour as its fill or its outline (the shape's bar, Edit > Shape).
- Alt click on a layer's eye shows only that layer, and again the rest as it was (`klad solo`).
- Menus: a menu bar (the Mac's own, an in-window one on Windows and Linux) holds every command in File, Edit, Tools, Layer, Frame, Animation, View, Window and Help, with short labels, its keys, ticks for what is on and greyed items for what cannot run now; File has close (cmd W) and recent drawings.
- Keys: layers, groups and shapes move, turn, scale and fade between frames with eases (`klad key`, the keys section of layer settings).
- Godot: `--godot-rig` exports a Skeleton2D with Polygon2D parts and the poses as animations; `--godot-scene` a parallax scene with a Parallax2D per depth plane.
- `klad paintover` (and "paint over" in the layers panel): brushwork from any picture, strokes that follow its forms and stop at its colour edges, big to small, detail where the picture has it; `--like` and `--frames` carry it through an animation without boiling.
- `klad compare` (and "compare with the reference" in the editor): a drawing against a reference picture in numbers and words, with a heat map.
- Large stills render several times faster: strokes land only where they painted, and the CLI replays a drawing's tiles on every core (same pixels).
- Dragging in a large drawing: shapes and strokes move under the pointer from the first move, wherever they are (in groups with effects, clipped, masked, under any blend), drawn in the background tile by tile; the layers above and the rest of the dragged layer stay in place (dragging a shape on the topmost layer no longer hid the shapes above it), and letting go redraws only the tiles the move touched.
- Edits on vector layers redraw only the tiles their shapes covered before and after, not the whole layer.
- Noise and haze no longer make every edit redraw the tiles around it.
- Drawings with many clipped layers and groups redraw several times faster: each tile skips the stacks that have nothing in it.
- A tap that wobbles selects a shape or strokes without nudging them: a drag starts once the pointer has moved 3 points on screen.
- The numbers panel shows how long the canvas takes each frame, next to the rest of the interface.
- Switching layers no longer redraws the whole canvas when nothing on it changes.
- Fixed tiles that could stay blank or out of date when a panel or popup was laid out for the first time while the canvas was being redrawn.

## 0.1.0 (2026-11-04)

The first public version: the `klad` command line on macOS and Linux and the editor on macOS, built from source.
