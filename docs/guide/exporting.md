# Exporting

## Export a picture or frames

File > Export (cmd E) opens the export window, which writes the same files as `klad export`:

- **image**: the frame you're on as one PNG, the whole image or one layer or group, at any scale. It's transparent unless you tick "keep the background colour".
- **frames**: numbered PNGs, one per frame, for all frames, a clip or a range like 0-11.
- **sprite sheet**: one PNG holding each distinct picture once, trimmed to its pixels, and a JSON file in Aseprite's layout with a tag per clip. Holds and links share their picture. Padding and the largest page size are settings; more frames go on more pages.
- **Godot**: the sheet, a SpriteFrames resource with one animation per clip at its own speed and loop, and a scene with an AnimatedSprite2D. "as a rig" exports a Skeleton2D with the drawing's parts and an AnimationPlayer with the poses ([Posing with bones](bones.md)); "as a parallax scene" exports each top-level layer as a texture on a Parallax2D that scrolls by its depth.

An empty path writes next to the drawing; a relative path counts from the drawing's folder.

## Export straight into a Godot project

Click a clip's bar in the timeline, type the folder in your Godot project under "Godot folder" and click "export to Godot". The folder is kept in the drawing, so later exports go to the same place. Re-exports keep Godot's ids for the files, so scenes that use them keep working.

## From the command line

The same exports, for a build script or an agent:

```sh
klad export art.klad --scale 2
klad export art.klad --sheet all
klad export art.klad --godot ../game/art/hero
```

Every option is in the [command reference](../cli.md#klad-export).
