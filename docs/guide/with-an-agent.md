_Last updated: 2026-11-12 23:55_

# Working with an agent

Let an agent you already use, such as Claude Code or Codex, paint into the drawing you have open. klad has no model inside: the agent works through the `klad` command line, and its strokes show up in your editor within about a second.

## Set it up

1. In the document menu at the foot of the tool strip, click **install the klad command**.
2. Ask your agent to run `klad skill` and read what it prints. For pixel art, ask it to run `klad skill pixel` too.
3. Copy the drawing's path from the File menu and tell the agent what you want: "Paint a lantern on its own layer in art.klad, lit from the left."

## Point at something

Press **N** and click the canvas to pin a note there, such as "more moss on this side". The agent reads your notes and what you've selected, and a pin turns green when it's done.

## Undo what the agent did

Press **cmd Z**: the agent's steps are in the same history as yours, and the History pane says who made each one. Its strokes are ordinary strokes, so you can also repaint, move or delete them by hand.
