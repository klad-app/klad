# Changelog

## Unreleased

The first downloadable builds.

- Downloads for macOS (an app and a disk image, Apple silicon and Intel in one), Windows and Linux (archives and AppImages), each with the `klad` command line, the editor, the fonts, the licences and the agent skill.
- The editor runs on Windows and Linux (`klad open`, or `klad-editor`), with the platform's open dialog; pens that report pressure through the system (Windows) draw with it.
- On a Mac, `.draw` drawings show in Finder as one item and open in klad with a double click.
- `klad install` (and "install the klad command" in the editor's document menu) puts `klad` on the terminal's path, for you and for agents. `klad skill` prints the agent skill.
- The editor checks once a day whether a newer version is out and says so, with a link to what changed (off in the settings window).
- `klad --version` and About show the commit a build was made from.
- In-betweens blend colours evenly (in OKLab), so light can follow a turning limb when the second key is coloured for it.
- `klad arcs` follows the part of a foot on the ground: a heel-to-toe roll is no longer reported as a slide.
- `klad paintover` (and "paint over" in the layers panel): brushwork from any picture, strokes that follow its forms and stop at its colour edges, big to small, detail where the picture has it; `--like` and `--frames` carry it through an animation without boiling.
- `klad compare` (and "compare with the reference" in the editor): a drawing against a reference picture in numbers and words, with a heat map.
- Large stills render several times faster: strokes land only where they painted, and the CLI replays a drawing's tiles on every core (same pixels).

## 0.1.0 (2026-11-04)

The first public version: the `klad` command line on macOS and Linux and the editor on macOS, built from source.
