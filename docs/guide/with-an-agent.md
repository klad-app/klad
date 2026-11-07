# Working with an agent

klad has no model inside. The agent is one you already run, Claude Code, Codex or anything else that can use a terminal, and it paints through the `klad` command line into the drawing you have open. Its strokes show up in your editor within about a second.

## Set it up

1. In the editor's document menu (at the foot of the tool strip), click "install the klad command". It puts `klad` on your terminal's path for you and your agent (`klad install` does the same).
2. Tell the agent to run `klad skill` and read what it prints: that's its manual.
3. Copy the drawing's path from the File menu and give it to the agent with what you want: "Paint a lantern on its own layer in art.klad, lit from the left."

## Point at things

- Press N and click the canvas to pin a note there ("more moss on this side"). The agent reads open notes with `klad notes`, and when it marks one done the pin turns green. The Notes pane lists them all.
- Whatever you select in the editor, the agent can read with `klad selection`, so "make the selected strokes thinner" works.

## Keep control

Every change the agent makes is a step in the same history as yours. The History pane says who made each step, and cmd Z undoes the agent's step just like your own. Its strokes are ordinary strokes marked as the agent's: repaint, move or delete them by hand, and its next pass starts from your version.
