# klad-client

Drive [klad](https://github.com/klad-app/klad), the 2D art app you share with an AI agent, from Rust. It runs the `klad` command line: create drawings, edit them with JSON ops (one transaction per call, undoable in the editor), read them back as JSON and render them to PNG. Someone with the drawing open in the klad editor sees every change within a second.

It needs `klad` on the path (or its path in `KLAD`, or `Klad::at`). The ops are the ones `klad apply` takes, described in the [command reference](https://github.com/klad-app/klad/blob/main/docs/cli.md); the drawing itself is described in the [file format](https://github.com/klad-app/klad/blob/main/docs/format.md).

```rust,no_run
use klad_client::{Klad, json};
use std::path::Path;

fn main() -> klad_client::Result<()> {
    let klad = Klad::new();
    let art = klad.drawing("art.klad");
    art.create(800, 600, Some("#e9e4da"))?;
    let ids = art.apply([
        json!({"op": "add", "layer": {"kind": "vector", "name": "sky"}}),
        json!({"op": "stroke", "layer": "l1", "brush": "ink", "size": 8, "color": "#1b1b1b",
               "path": "M100 400 C 300 200 500 600 700 400", "pressure": [0.1, 1, 0.05]}),
    ])?;
    println!("added {ids:?}");
    let tree = art.ls(0)?;
    println!("{}", tree);
    art.render(Path::new("art.png"), 0, 0.5)?;
    let notes = art.notes()?;
    println!("{notes}");
    Ok(())
}
```

Any other command runs through `Drawing::command` (text) or `Drawing::json` (for commands with `--json`), and `Klad::run` for those that take no drawing.

## Licence

MIT, copyright Indicode BV (`LICENSE`). klad itself is source-available under its own licence; this crate, the file format and the command reference are MIT so that any program or agent can work with klad drawings.
