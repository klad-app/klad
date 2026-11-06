# Changelog

## Unreleased

The first builds, and klad's licences.

- Drawings are `.klad` folders in a fresh format, `klad/1`. Drawings from before (`.draw` folders) do not open and are not converted.
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
- "Report a problem" copies what an issue needs.
- Bones: a layer or group gets a skeleton (`klad bone`, the bone tool J), weights from the bones or painted (`klad weights`), and poses that bend the drawing (`klad pose`, pose mode); in-betweens of two poses turn the bones. The bone tool lists the skeleton, picks a bone in every mode, and moves ends, joints and whole bones with their children (`klad bone --move` with `--by`).
- Menus: a menu bar (the Mac's own, an in-window one on Windows and Linux) holds every command, with its keys, ticks for what is on and greyed items for what cannot run now; File has close (cmd W) and recent drawings.
- Keys: layers, groups and shapes move, turn, scale and fade between frames with eases (`klad key`, the keys section of the inspector).
- Godot: `--godot-rig` exports a Skeleton2D with Polygon2D parts and the poses as animations; `--godot-scene` a parallax scene with a Parallax2D per depth plane.
- `klad paintover` (and "paint over" in the layers panel): brushwork from any picture, strokes that follow its forms and stop at its colour edges, big to small, detail where the picture has it; `--like` and `--frames` carry it through an animation without boiling.
- `klad compare` (and "compare with the reference" in the editor): a drawing against a reference picture in numbers and words, with a heat map.
- Large stills render several times faster: strokes land only where they painted, and the CLI replays a drawing's tiles on every core (same pixels).

## 0.1.0 (2026-11-04)

The first public version: the `klad` command line on macOS and Linux and the editor on macOS, built from source.
