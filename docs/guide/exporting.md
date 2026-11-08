_Last updated: 2026-11-08 14:50_

# Exporting

Export a picture, numbered frames or a sprite sheet, or send your animation straight into a Godot project.

## Export a picture or frames

1. Press **cmd E**.
2. Pick **image** for the frame you're on, **frames** for one PNG per frame, or **sprite sheet**.
3. Click **export**. Without a path, the files go next to the drawing.

Pictures are transparent unless you tick **keep the background colour**. A sprite sheet comes with a JSON file in Aseprite's layout, with a tag per clip.

## Export into a Godot project

1. In the timeline, click a clip's bar.
2. Type the folder in your Godot project under **Godot folder**.
3. Click **export to Godot**.

You get the sheet, a SpriteFrames resource with one animation per clip and a scene to drop in. klad remembers the folder, and exporting again keeps Godot's ids, so scenes that use the files keep working.

## Export from a script

```sh
klad export art.klad --scale 2
klad export art.klad --godot ../game/art/hero
```

Every option is in the [command reference](../cli.md#klad-export).
