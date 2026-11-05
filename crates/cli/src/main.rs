mod agent;
mod arcs;
mod bench;
mod brushes;
mod colour;
mod compare;
mod demo;
mod edit;
mod export;
mod fills;
mod fonts;
mod frames;
mod history;
mod inbetween;
mod install;
mod layers;
mod looks;
mod marks;
mod paintover;
mod paths;
mod rig;
mod shapes;
mod sheet;
mod sprites;

use agent::{Look, Output};
use anyhow::{Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use klad_brush::BrushSet;
use klad_core::bundle::{Blank, Bundle};
use klad_core::doc::Kind;
use klad_core::fill::{EXPAND, FillSource, TOLERANCE};
use klad_core::history::Stack;
use klad_core::markedit::{LassoAction, MarkTransform};
use klad_core::ops::{
    BoneSpec, ClipEdit, CopySpec, CopyTarget, FillSpec, FoliageSpec, GrassSpec, HatchSpec, InbetweenSpec, InkSpec,
    LassoSpec, LightSpec, Op, PaintOverSpec, PaintSpec, PaletteEdit, Place, PoseSpec, ScatterSpec, StrokeSpec,
    WeightsSpec,
};
use klad_core::retime::{Retime, RetimeSpec};
use klad_core::stroke::{fills_of, strokes_of};
use klad_core::timing::{CelAction, CelSpec, FrameSpec};
use klad_core::tween::{Match, Spacing, Unpaired};
use klad_render::{View, compose};
use marks::{fill_json, hex8, stroke_json};
use std::path::{Path, PathBuf};
use std::time::Instant;

const VERSION: &str = match env!("KLAD_BUILD").is_empty() {
    true => env!("CARGO_PKG_VERSION"),
    false => concat!(env!("CARGO_PKG_VERSION"), " (", env!("KLAD_BUILD"), ")"),
};

#[derive(Parser)]
#[command(
    name = "klad",
    version = VERSION,
    about = "klad: draw and paint with an agent, from the command line"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct EditFlags {
    #[arg(long, help = "Render a preview after the edit and print its path")]
    render: bool,
    #[arg(long, help = "Preview scale (default: longest side 1024 px)")]
    scale: Option<f32>,
    #[arg(long, help = "Print the result as JSON")]
    json: bool,
    #[arg(long, help = "Draw on the preview: ids, bbox, grid or all (comma separated)")]
    overlay: Option<String>,
}

#[derive(Args)]
struct MarkFlags {
    #[arg(
        long,
        help = "Render a preview after the edit and print its path (longest side 1024 px)"
    )]
    render: bool,
    #[arg(long, help = "Print the result as JSON")]
    json: bool,
    #[arg(long, help = "Draw on the preview: ids, bbox, grid or all (comma separated)")]
    overlay: Option<String>,
}

impl MarkFlags {
    fn edit(&self) -> EditFlags {
        EditFlags {
            render: self.render,
            scale: None,
            json: self.json,
            overlay: self.overlay.clone(),
        }
    }
}

#[derive(Args)]
struct Where {
    #[arg(long = "in", help = "Container: group layer, vector layer, cel or group node")]
    into: Option<String>,
    #[arg(long, help = "Place just above this sibling")]
    above: Option<String>,
    #[arg(long, help = "Place just below this sibling")]
    below: Option<String>,
    #[arg(long, help = "Position in the container, 0 is the bottom")]
    index: Option<usize>,
    #[arg(long, help = "Frame whose cel receives new nodes (default 0)")]
    frame: Option<u32>,
}

impl Where {
    fn place(&self) -> Place {
        Place {
            above: self.above.clone(),
            below: self.below.clone(),
            into: self.into.clone(),
            index: self.index,
            frame: self.frame,
        }
    }
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Create an empty bundle with paint layers")]
    New {
        bundle: PathBuf,
        #[arg(long, default_value_t = 2048)]
        width: u32,
        #[arg(long, default_value_t = 2048)]
        height: u32,
        #[arg(long, default_value_t = 1)]
        layers: usize,
        #[arg(long, help = "Background colour, for example #ffffff")]
        background: Option<String>,
        #[arg(long, default_value_t = 12)]
        fps: u32,
        #[arg(long, default_value_t = 1)]
        frames: u32,
        #[arg(long, help = "Add a vector layer named sketch on top, for blocking in shapes")]
        sketch: bool,
    },
    #[command(
        about = "Exposure sheet: a row per frame, a column per layer (top first)",
        long_about = "Exposure sheet: a row per frame, a column per layer (top first).\n\nA cel id marks the frame a drawing starts, | that it is still showing, c5->c1 a link (c5 shows c1's drawing), . a blank frame. Clips are listed on the right ([run on their first frame, run] on their last)."
    )]
    Frames {
        bundle: PathBuf,
        #[arg(long)]
        from: Option<u32>,
        #[arg(long)]
        to: Option<u32>,
        #[arg(long)]
        json: bool,
    },
    #[command(
        about = "Insert or remove frames in every layer or the listed ones",
        long_about = "Insert or remove frames in every layer or the listed ones.\n\n  klad frame art.draw insert 4 --count 2\n  klad frame art.draw remove 4 --layers l3,l5\n\nInsert: what showed at the frame now shows there and for the inserted frames too (that exposure lengthens), everything after moves later. Remove: exposures over the removed frames get shorter, drawings left with no frames go (links to them take them over), everything after moves earlier. On every layer, clips and the timeline length follow; on listed layers an insert lengthens the timeline only as far as their drawings need (a last drawing without a hold keeps its length)."
    )]
    Frame {
        bundle: PathBuf,
        #[arg(value_parser = ["insert", "remove"])]
        action: String,
        at: u32,
        #[arg(long, default_value_t = 1)]
        count: u32,
        #[arg(long, value_delimiter = ',', help = "Layer ids or names (default: every layer)")]
        layers: Vec<String>,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Track marks, parts or points across frames: positions, spacing, ground contacts, feet crossing",
        long_about = "Track things across frames, the way an animator checks arcs and spacing: a mark (c4:s12, through the drawings that keep its id, as in-betweens do), a part (leaf, or lines:leaf: the centre of the part's marks in each drawing) or a point (x,y: carried by the stroke nearest to it). Prints each frame's position, the distance moved since the frame before, and the lowest point. --ground y (one for all tracks, or one per track) adds the frames where a track goes under the ground line or slides along it while touching it (for a cycle drawn in place, --travel gives the px per frame the planted foot should move back by); with two or more tracks, the frames where two of them swap sides (feet crossing) are listed. --check makes any of these an error, for scripts. klad render --overlay arcs --track ... draws the same paths.\n\n  klad arcs walk.draw --track 'near foot' --track 'far foot' --clip walk --ground 610,598 --travel 7 --check\n  klad arcs walk.draw --track c12:s4 --from 12 --to 27 --json"
    )]
    Arcs {
        bundle: PathBuf,
        #[arg(long, required = true)]
        track: Vec<String>,
        #[arg(long, conflicts_with_all = ["from", "to"], help = "The frames of this clip")]
        clip: Option<String>,
        #[arg(long)]
        from: Option<u32>,
        #[arg(long)]
        to: Option<u32>,
        #[arg(
            long,
            allow_hyphen_values = true,
            value_delimiter = ',',
            help = "The ground line's y, one for every track or one per track: report feet under it or sliding on it"
        )]
        ground: Vec<f32>,
        #[arg(
            long,
            default_value_t = 0.0,
            allow_hyphen_values = true,
            help = "For a cycle drawn in place: how far the character travels per frame (px, + to the right); a planted foot should move back by as much"
        )]
        travel: f32,
        #[arg(long, default_value_t = arcs::SLACK, help = "How many px off still counts as on the ground and not sliding")]
        slack: f32,
        #[arg(long)]
        json: bool,
        #[arg(
            long,
            help = "Exit with an error when a track goes under the ground, slides, or two tracks swap sides"
        )]
        check: bool,
    },
    #[command(
        about = "Name a group of marks in a drawing (a part), so in-betweens pair them whatever their ids",
        long_about = "Name a group of marks in a drawing: a part such as leaf, near leg or body. In-betweens pair marks part by part first (in order within a part, nearest shape when the counts differ), whatever their ids; arcs track a part's centre. A mark is in one part at most; naming it again moves it. dup, copy and in-betweens keep parts. --name '' takes the marks out of their part.\n\n  klad part art.draw c4:s14 c4:s15 --name leaf\n  klad part art.draw c9:s17 c9:s18 --name leaf\n  klad stroke art.draw lines --path \"M...\" --part leaf\n\nAlso an op: {\"op\":\"part\",\"ids\":[\"c4:s14\",\"c4:s15\"],\"name\":\"leaf\"}. klad strokes lists each mark's part."
    )]
    Part {
        bundle: PathBuf,
        ids: Vec<String>,
        #[arg(long)]
        name: String,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Duplicate a layer with its drawings, links and mask, above it",
        long_about = "Duplicate a layer: its drawings, links, in-between marks and mask are copied with new ids, and the copy, named \"<name> copy\", goes right above it.\n\n  klad duplicate art.draw l3\n\nAlso an op in klad apply: {\"op\":\"duplicate\",\"layer\":\"l3\"}."
    )]
    Duplicate {
        bundle: PathBuf,
        layer: String,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Make drawings on the timeline: new, dup, link, unlink, move",
        long_about = "Make drawings on the timeline.\n\n  klad cel art.draw new l3 --frame 4            a blank drawing on layer l3 at frame 4\n  klad cel art.draw dup c4 [--frame 6]           a copy of c4's drawing (default: right after c4's exposure)\n  klad cel art.draw link c4 --frame 8            show c4's drawing again at frame 8\n  klad cel art.draw unlink c9                    turn the link c9 into a drawing of its own\n  klad cel art.draw move c5 --frame 12           start c5 at frame 12, keeping its hold; drawings in the way are pushed along\n\nInside an exposure a new cel takes over the rest of it; elsewhere it lasts until the next cel, or --hold frames (dup and link default to their source's hold). --insert moves later cels of that layer back to make room (only at the start or end of an exposure). Strokes keep their ids in a dup."
    )]
    Cel {
        bundle: PathBuf,
        #[arg(value_parser = ["new", "dup", "link", "unlink", "move"])]
        action: String,
        #[arg(help = "new: the layer; dup, link, unlink, move: the cel")]
        target: String,
        #[arg(long)]
        frame: Option<u32>,
        #[arg(long)]
        hold: Option<u32>,
        #[arg(long)]
        insert: bool,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Copy strokes and fills into another drawing, with new ids, moving them on the way",
        long_about = "Copy strokes and fills (<cel>:s<id>) into another drawing. The copies get new ids, printed as a map (c1:s3 -> c9:s12), and land on top of that drawing in the order they had. --to names the drawing; --frame takes the drawing the layer shows at that frame (a new drawing on an empty frame or past the end; a frame that shows a link names its source instead). The marks' own layer unless --layer. The move, turn, scale and flip options are klad transform's, applied to the copies together about the middle of their bounds or --pivot. --cut keeps only the parts inside a polygon.\n\n  klad copy art.draw c1:s3 c1:s4 --frame 4\n  klad copy art.draw c1:s3 --to c9 --move 12,-4 --rotate 8\n  klad copy art.draw c1:s3 --frame 6 --layer shade --flip h --pivot 512,400\n\nAlso a copy op in klad apply: {\"op\":\"copy\",\"ids\":[\"c1:s3\"],\"frame\":4,\"x\":12,\"y\":-4} (or \"to\":\"c9\")."
    )]
    Copy {
        bundle: PathBuf,
        ids: Vec<String>,
        #[arg(long, conflicts_with = "frame", help = "The drawing (cel id) the copies go into")]
        to: Option<String>,
        #[arg(long, help = "The drawing shown at this frame (a new drawing on an empty frame)")]
        frame: Option<u32>,
        #[arg(long, requires = "frame", help = "With --frame: the layer (default: the marks' own)")]
        layer: Option<String>,
        #[arg(long = "move", value_parser = parse_pair, allow_hyphen_values = true, help = "dx,dy in px")]
        offset: Option<[f32; 2]>,
        #[arg(long, allow_hyphen_values = true, help = "Degrees, clockwise")]
        rotate: Option<f32>,
        #[arg(long, allow_hyphen_values = true, help = "Uniform scale")]
        scale: Option<f32>,
        #[arg(long, allow_hyphen_values = true)]
        sx: Option<f32>,
        #[arg(long, allow_hyphen_values = true)]
        sy: Option<f32>,
        #[arg(long, value_parser = parse_pair, allow_hyphen_values = true, help = "x,y to turn, scale and flip about")]
        pivot: Option<[f32; 2]>,
        #[arg(long, value_parser = ["h", "v"], help = "Mirror: h (left-right) or v (up-down)")]
        flip: Option<String>,
        #[arg(
            long,
            help = "Keep only the parts inside this polygon: x,y pairs separated by spaces"
        )]
        cut: Option<String>,
        #[command(flatten)]
        flags: MarkFlags,
    },
    #[command(
        about = "Retime a range of frames: on ones/twos/threes, fit into n frames, reverse, ping-pong",
        long_about = "Retime frames FROM-TO (both included) on every layer, or on --layers (with their masks and children). The range is cut where any of those layers changes what it shows; each piece is one picture.\n\n  klad retime art.draw 0-11 --on 2        every picture shows for 2 frames (ones to twos)\n  klad retime art.draw 0-23 --on 1        and back to ones\n  klad retime art.draw 0-11 --fit 16      the range becomes 16 frames; order and relative timing kept, at least one frame each\n  klad retime art.draw 0-7 --reverse\n  klad retime art.draw 0-7 --pingpong     the pictures again backwards as links, the ends not repeated: a b c d c b\n\nLater cels move by the change in length; on every layer clips move and stretch with it and the timeline grows or shrinks; on listed layers clips stay and the timeline only grows. A drawing that ends up in two places is shown again by a link. One history step. Also a retime op: {\"op\":\"retime\",\"from\":0,\"to\":11,\"on\":2,\"layers\":[\"l3\"]} (or \"fit\":16, \"reverse\":true, \"pingpong\":true)."
    )]
    Retime {
        bundle: PathBuf,
        #[arg(value_parser = parse_frames_span, help = "FROM-TO, both included")]
        range: (u32, u32),
        #[arg(long, help = "Frames per picture: 1, 2, 3 ...")]
        on: Option<u32>,
        #[arg(long, help = "The range becomes this many frames")]
        fit: Option<u32>,
        #[arg(long)]
        reverse: bool,
        #[arg(long)]
        pingpong: bool,
        #[arg(long, value_delimiter = ',', help = "Layer ids or names (default: every layer)")]
        layers: Vec<String>,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Make the drawings between two keys: a new cel on each frame between them",
        long_about = "Make in-betweens of two keys on paint layers: the drawings shown at --from and --to (the second key is the frame where a drawing starts; a link there names its source, so a loop's last in-betweens can lead back to its first drawing). By default a new cel on every frame between them; --count n spreads n of them, each holding until the next.\n\n  klad inbetween art.draw --layers lines,colour --from 0 --to 6\n  klad inbetween art.draw --layers lines --from 0 --to 8 --count 3 --spacing in-out\n  klad inbetween art.draw --layers arm --from 4 --to 10 --match near --pair s3=s7\n  klad inbetween art.draw --redo                     after a key changed\n\nStrokes pair by id (a key duplicated from the other keeps its ids), by --match order or by nearest shape; --pair forces pairs (first key id = second key id). Each pair is resampled along its length; points, pressure, tilt, size, opacity, flow, taper and colour move at the spacing (colour evenly in OKLab, so colour the second key for its light and the in-betweens carry the change), and the pair turns and scales along an arc about the point its best-fit rotation turns around, so a swinging limb keeps its length. A line end that touches another line in both keys touches it in every in-between. Lasso fills are interpolated; flood fills are recomputed in each in-between from a seed carried with the lines next to it (the layers they fill against go first). Unpaired marks and fills that fail (a seed on a line, a leak) are left out and listed.\n\nSpacing: even, in (eases in: the in-betweens bunch near the first key), out (they bunch near the second key), in-out, or the positions themselves, 0.2,0.5,0.9. Every in-between remembers its keys (tween in klad show), so --redo remakes them after a key changed; it refuses cels changed since they were made unless --force. One history step."
    )]
    Inbetween {
        bundle: PathBuf,
        #[arg(
            long,
            value_delimiter = ',',
            help = "Paint layers (ids or names; a group means its paint layers)"
        )]
        layers: Vec<String>,
        #[arg(long, help = "The frame of the first key")]
        from: Option<u32>,
        #[arg(long, help = "The frame where the second key starts")]
        to: Option<u32>,
        #[arg(long, help = "How many in-betweens (default: one per frame between the keys)")]
        count: Option<u32>,
        #[arg(
            long,
            default_value = "even",
            help = "even, in, out, in-out, or positions like 0.25,0.5,0.9"
        )]
        spacing: String,
        #[arg(
            long = "match",
            default_value = "id",
            help = "Pair strokes by id, order or near (nearest shape)"
        )]
        matching: String,
        #[arg(
            long,
            value_delimiter = ',',
            help = "Force pairs: s3=s7 (first key's s3 with the second key's s7)"
        )]
        pair: Vec<String>,
        #[arg(
            long,
            help = "Remake the in-betweens (of --layers, between --from and --to when given)"
        )]
        redo: bool,
        #[arg(
            long,
            help = "Replace drawings between the keys, or redo in-betweens changed since they were made"
        )]
        force: bool,
        #[arg(
            long = "loop",
            help = "The second key is the loop's first drawing (the clip holding --from, else frame 0); --to defaults to the loop's end"
        )]
        looping: bool,
        #[arg(
            long,
            requires = "redo",
            help = "With --redo: remake the agent's marks and keep the marks drawn by hand on top, carried with the lines under them"
        )]
        keep_drawn: bool,
        #[arg(
            long,
            value_parser = ["leave", "pop", "fade", "draw-on"],
            default_value = "leave",
            help = "Marks in one key only: leave them out (listed), pop at --at, fade, or draw on along their length"
        )]
        unpaired: String,
        #[arg(long, help = "With --unpaired pop: when it pops, 0 to 1 (default 0.5)")]
        at: Option<f32>,
        #[arg(
            long,
            value_delimiter = ',',
            requires = "redo",
            help = "With --redo: only these in-betweens (cel ids)"
        )]
        only: Vec<String>,
        #[arg(
            long,
            value_delimiter = ',',
            requires = "redo",
            help = "With --redo: these in-betweens keep their hand-drawn marks on top"
        )]
        keep_cels: Vec<String>,
        #[arg(
            long,
            value_delimiter = ',',
            requires = "redo",
            help = "With --redo: these in-betweens are remade with their hand-drawn marks"
        )]
        force_cels: Vec<String>,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Copy a font into the bundle for text nodes, by name or from a file, or list the fonts it has",
        long_about = "Copy one of klad's fonts (by family, or family and style) or a font file (TTF or OTF) into the bundle's fonts folder, so text nodes render the same on every machine; a family that klad does not have yet but Google Fonts does is downloaded first. With no font, list the bundle's fonts. Text nodes also take a family name directly (font=Inter, font=\"Inter Bold\") and copy it in themselves.\n\n  klad font art.draw Cinzel\n  klad font art.draw \"Barlow Bold\"\n  klad font art.draw ~/fonts/Signage.ttf\n  klad add art.draw text --in l4 text=INN font=Cinzel size=48 x=120 y=80 fill=#2b1d12"
    )]
    Font { bundle: PathBuf, font: Option<String> },
    #[command(
        about = "List the fonts that come with klad, search the open fonts it can download, or download one",
        long_about = "List the font families that come with klad and the ones downloaded before, by group (sans, serif, mono, display, handwriting, pixel), with their licence and styles; with a bundle, also the fonts it holds. --search looks through every open family on Google Fonts (about 1,900) and --get downloads one into the per-user fonts folder, after which it works like the ones klad ships.\n\n  klad fonts\n  klad fonts --group pixel\n  klad fonts --search \"slab\"\n  klad fonts --get Lobster"
    )]
    Fonts {
        bundle: Option<PathBuf>,
        #[arg(long, help = "Only this group: sans, serif, mono, display, handwriting or pixel")]
        group: Option<String>,
        #[arg(
            long,
            help = "Search the open fonts that can be downloaded, by words of the family name or the group"
        )]
        search: Option<String>,
        #[arg(long, help = "Download this family from Google Fonts into the per-user fonts folder")]
        get: Option<String>,
        #[arg(long, help = "Print JSON")]
        json: bool,
    },
    #[command(
        about = "Fog and haze by depth: show, set or turn off the document's atmosphere",
        long_about = "The atmosphere fades layers at the top of the stack that have a depth (klad set art.draw g2 depth=2) towards the fog colour, blurs them and takes colour out, more the deeper they are.\n\n  klad atmosphere art.draw\n  klad atmosphere art.draw color=#9aa8b8 top=#d8e0e8 density=0.5 blur=1 desaturate=0.15\n  klad atmosphere art.draw --off"
    )]
    Atmosphere {
        bundle: PathBuf,
        props: Vec<String>,
        #[arg(long, help = "Remove the atmosphere")]
        off: bool,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Show the timeline (fps, length, origin, clips) or set fps, length and origin",
        long_about = "Show the timeline (fps, length, origin, clips) or set fps, length and origin.\n\n  klad timeline art.draw\n  klad timeline art.draw --fps 12 --frames 24\n  klad timeline art.draw --origin 512,700      the canvas point a Godot export puts on the node's position\n  klad timeline art.draw --origin centre       back to the middle of the canvas (the default)"
    )]
    Timeline {
        bundle: PathBuf,
        #[arg(long)]
        fps: Option<u32>,
        #[arg(long)]
        frames: Option<u32>,
        #[arg(long, help = "x,y on the canvas, or centre")]
        origin: Option<String>,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "List, add, change, rename and remove clips (named frame ranges)",
        long_about = "List, add, change, rename and remove clips: named frame ranges such as idle, run or attack. Exports turn each clip into an animation; a clip may have its own fps.\n\n  klad clip art.draw                                        list them\n  klad clip art.draw --add run --from 0 --to 11 --loop --fps 12\n  klad clip art.draw --set run --to 15 --no-loop\n  klad clip art.draw --set run --name sprint               rename\n  klad clip art.draw --set run --fps doc                   use the document's fps again\n  klad clip art.draw --rm run\n\nFrames inserted or removed on every layer move and stretch clips; a clip ends where the timeline ends."
    )]
    Clip {
        bundle: PathBuf,
        #[arg(long, conflicts_with_all = ["set", "rm"], help = "Add a clip with this name")]
        add: Option<String>,
        #[arg(long, conflicts_with = "rm", help = "Change the clip with this name")]
        set: Option<String>,
        #[arg(long, help = "Remove the clip with this name")]
        rm: Option<String>,
        #[arg(long, help = "With --set: the clip's new name")]
        name: Option<String>,
        #[arg(long, help = "First frame")]
        from: Option<u32>,
        #[arg(long, help = "Last frame (included)")]
        to: Option<u32>,
        #[arg(long = "loop", conflicts_with = "no_loop", help = "The clip loops")]
        looping: bool,
        #[arg(long = "no-loop", help = "The clip plays once")]
        no_loop: bool,
        #[arg(long, help = "The clip's own fps, or doc for the document's")]
        fps: Option<String>,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(about = "Layer tree with ids, kinds, contents and bounds at a frame, top layer first")]
    Ls {
        bundle: PathBuf,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "One layer, cel, node, stroke or fill (<cel>:s<id>) in full (JSON)")]
    Show { bundle: PathBuf, id: String },
    #[command(
        about = "Add a layer, cel or shape",
        long_about = "Add a layer, cel or shape.\n\n  klad add art.draw layer vector name=sky\n  klad add art.draw rect --in l2 x=0 y=0 w=2048 h=900 fill=#1b2a41\n  klad add art.draw ellipse --in l2 cx=500 cy=300 rx=80 ry=80 'fill={\"radial\":{\"center\":[500,300],\"radius\":80,\"stops\":[[0,\"#fff6d5\"],[1,\"#f3c76a\"]]}}'\n  klad add art.draw cel --in l3 frame=4 hold=2\n\nValues are JSON when they parse as JSON, otherwise strings. Dotted keys nest: transform.rotate=15, stroke.width=3."
    )]
    Add {
        bundle: PathBuf,
        #[arg(help = "layer, cel, mask, rect, ellipse, path, group, rays, image, scatter, repeat, mirror or text")]
        what: String,
        #[arg(
            help = "For layers the kind first (paint, vector, group, image with src=photo.jpg, or adjust and its type: hsl, levels, curves, balance, gradient-map, fill), then key=value pairs"
        )]
        rest: Vec<String>,
        #[command(flatten)]
        place: Where,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Change properties: klad set art.draw n4 fill=#ff0000 transform.rotate=15 (null removes)",
        long_about = "Change properties of a layer, cel, shape, stroke or fill. Values are JSON when they parse as JSON, otherwise strings; null removes a key. Dotted keys nest.\n\n  klad set art.draw n4 fill=#ff0000 transform.rotate=15\n  klad set art.draw l2 opacity=0.5 blend=multiply\n  klad set art.draw c4:s12 color=#c84a3a size=6 brush=pencil\n  klad set art.draw c4:s7 color=skin opacity=0.8\n\nStrokes take color, size, opacity, flow, brush and taper ([in,out] or null); fills take color and opacity. Colours may be palette names."
    )]
    Set {
        bundle: PathBuf,
        id: String,
        props: Vec<String>,
        #[arg(long, default_value_t = 0, help = "Frame to render with --render")]
        frame: u32,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(about = "Remove layers, cels or nodes")]
    Rm {
        bundle: PathBuf,
        ids: Vec<String>,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Move, rotate or scale strokes and fills (<cel>:s<id>)",
        long_about = "Move, rotate or scale strokes and fills, addressed as <cel>:s<id> (klad strokes lists them). Rotation and scale turn about --pivot, by default the middle of the marks' bounds. Stroke sizes and tapers scale with them; fills move by whole pixels and are resampled when turned or scaled.\n\n  klad transform art.draw c4:s12 c4:s13 --move 40,-10\n  klad transform art.draw c4:s12 --rotate 15 --scale 1.2\n  klad transform art.draw c4:s3 c4:s5 --sx -1 --pivot 512,400\n\nAlso a transform op in klad apply: {\"op\":\"transform\",\"ids\":[\"c4:s12\"],\"x\":40,\"y\":-10,\"rotate\":15,\"sx\":1.2,\"sy\":1.2}"
    )]
    Transform {
        bundle: PathBuf,
        ids: Vec<String>,
        #[arg(long = "move", value_parser = parse_pair, allow_hyphen_values = true, help = "dx,dy in px")]
        offset: Option<[f32; 2]>,
        #[arg(long, allow_hyphen_values = true, help = "Degrees, clockwise")]
        rotate: Option<f32>,
        #[arg(long, allow_hyphen_values = true, help = "Uniform scale")]
        scale: Option<f32>,
        #[arg(long, allow_hyphen_values = true)]
        sx: Option<f32>,
        #[arg(long, allow_hyphen_values = true)]
        sy: Option<f32>,
        #[arg(long, value_parser = parse_pair, allow_hyphen_values = true, help = "x,y to turn and scale about")]
        pivot: Option<[f32; 2]>,
        #[arg(long, value_parser = ["h", "v"], help = "Mirror: h (left-right) or v (up-down)")]
        flip: Option<String>,
        #[arg(long, default_value_t = 0, help = "Frame to render with --render")]
        frame: u32,
        #[command(flatten)]
        flags: MarkFlags,
    },
    #[command(
        about = "Lasso strokes and fills on a paint layer; --cut also takes the parts crossing the edge",
        long_about = "Select the strokes and fills of a paint layer inside a polygon and move, turn, scale, flip or delete them in one step. Without --cut only whole marks move: strokes whose points are all inside and fills whose pixels are. With --cut every stroke or fill crossing the edge is split at the edge: the part outside stays where it is and the part inside moves with the rest, so nothing outside the lasso changes. Without a move, turn, scale, flip or --rm it only lists what is inside and what crosses.\n\n  klad lasso art.draw l3 --polygon '100,100 400,120 380,400 90,380'\n  klad lasso art.draw l3 --polygon '100,100 400,120 380,400 90,380' --cut --move 60,-20\n  klad lasso art.draw l3 --polygon '...' --cut --rotate 15 --scale 1.2\n  klad lasso art.draw l3 --polygon '...' --flip h\n  klad lasso art.draw l3 --polygon '...' --cut --rm\n\nAlso a lasso op in klad apply: {\"op\":\"lasso\",\"layer\":\"l3\",\"polygon\":[[100,100],[400,120],[380,400]],\"cut\":true,\"x\":60,\"y\":-20} (or \"rm\":true)."
    )]
    Lasso {
        bundle: PathBuf,
        layer: String,
        #[arg(long, help = "x,y pairs separated by spaces")]
        polygon: String,
        #[arg(
            long,
            help = "Split strokes and fills crossing the edge and take the inside parts too"
        )]
        cut: bool,
        #[arg(long = "move", value_parser = parse_pair, allow_hyphen_values = true, help = "dx,dy in px")]
        offset: Option<[f32; 2]>,
        #[arg(long, allow_hyphen_values = true, help = "Degrees, clockwise")]
        rotate: Option<f32>,
        #[arg(long, allow_hyphen_values = true, help = "Uniform scale")]
        scale: Option<f32>,
        #[arg(long, allow_hyphen_values = true)]
        sx: Option<f32>,
        #[arg(long, allow_hyphen_values = true)]
        sy: Option<f32>,
        #[arg(long, value_parser = parse_pair, allow_hyphen_values = true, help = "x,y to turn, scale and flip about")]
        pivot: Option<[f32; 2]>,
        #[arg(long, value_parser = ["h", "v"], help = "Mirror: h (left-right) or v (up-down)")]
        flip: Option<String>,
        #[arg(long, help = "Delete what the lasso holds")]
        rm: bool,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[command(flatten)]
        flags: MarkFlags,
    },
    #[command(
        about = "List the points of a path, rect or ellipse, or edit them",
        long_about = "Without edit flags: list the points (anchors) of a path, rect or ellipse with their handles, numbered across subpaths. Coordinates are the shape's own, as in its d, before its transform. Any edit turns a rect or ellipse into a path that looks the same.\n\n  klad path art.draw n7\n  klad path art.draw n7 --move 3 412,230\n  klad path art.draw n7 --nudge 3,4,5 0,-20\n  klad path art.draw n7 --handle 3 out 430,210\n  klad path art.draw n7 --insert 3,0.5      (split the segment after point 3)\n  klad path art.draw n7 --add 418,236       (a point on the nearest segment)\n  klad path art.draw n7 --rm 4,5 --corner 2 --smooth 6\n  klad path art.draw n7 --simplify 1.5      (fewer, smooth points within 1.5 px)\n\nAs ops: {\"op\":\"path\",\"id\":\"n7\",\"move\":3,\"to\":[412,230]}, \"move\":[3,4],\"by\":[0,-20]; \"handle\":3,\"out\":[430,210]; \"insert\":3,\"t\":0.5; \"add\":[418,236]; \"rm\":[4,5]; \"corner\":2; \"smooth\":6; \"simplify\":1.5. A smooth point keeps its handles in line unless \"free\":true."
    )]
    Path {
        bundle: PathBuf,
        node: String,
        #[arg(long = "move", num_args = 2, value_names = ["POINT", "X,Y"], allow_hyphen_values = true)]
        move_to: Option<Vec<String>>,
        #[arg(long, num_args = 2, value_names = ["POINTS", "DX,DY"], allow_hyphen_values = true)]
        nudge: Option<Vec<String>>,
        #[arg(long, num_args = 3, value_names = ["POINT", "in|out", "X,Y"], allow_hyphen_values = true)]
        handle: Option<Vec<String>>,
        #[arg(long, value_name = "POINT[,T]")]
        insert: Option<String>,
        #[arg(long, value_parser = parse_pair, allow_hyphen_values = true, value_name = "X,Y")]
        add: Option<[f32; 2]>,
        #[arg(long, value_name = "POINTS")]
        rm: Option<String>,
        #[arg(long, value_name = "POINTS")]
        corner: Option<String>,
        #[arg(long, value_name = "POINTS")]
        smooth: Option<String>,
        #[arg(long, value_name = "PX")]
        simplify: Option<f32>,
        #[arg(long, help = "Turn a rect or ellipse into a path without other changes")]
        convert: bool,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(about = "Move a layer or node: --above, --below, --in, --index")]
    Mv {
        bundle: PathBuf,
        id: String,
        #[command(flatten)]
        place: Where,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Apply many edits from JSON lines (stdin or --file) in one write",
        long_about = "Apply many edits from JSON lines in one transaction: all apply or none.\n\n  {\"op\":\"add\",\"layer\":{\"kind\":\"vector\",\"name\":\"sky\"}}\n  {\"op\":\"add\",\"node\":{\"type\":\"rect\",\"x\":0,\"y\":0,\"w\":100,\"h\":50,\"fill\":\"#336699\"},\"in\":\"l1\"}\n  {\"op\":\"set\",\"id\":\"n2\",\"fill\":\"#ff0000\",\"transform.rotate\":15}\n  {\"op\":\"rm\",\"id\":\"n2\"}\n  {\"op\":\"mv\",\"id\":\"l1\",\"below\":\"l3\"}\n  {\"op\":\"add\",\"cel\":{\"frame\":4,\"hold\":2},\"in\":\"l3\"}\n\nNew ids are printed in order."
    )]
    Apply {
        bundle: PathBuf,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long, default_value_t = 0, help = "Frame to render with --render")]
        frame: u32,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(about = "Render a frame to PNG and print the path")]
    Render {
        bundle: PathBuf,
        #[arg(short, long)]
        out: Option<PathBuf>,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[arg(long, help = "Scale (default 1, or longest side 1024 px with --focus)")]
        scale: Option<f32>,
        #[arg(long, value_parser = parse_region, help = "Only this canvas region: x,y,w,h")]
        region: Option<[i32; 4]>,
        #[arg(long, help = "Zoom to a layer, cel or node, with a margin")]
        focus: Option<String>,
        #[arg(
            long,
            help = "Draw on top: ids, bbox, grid, onion, parts, arcs or all (comma separated); onion shows the drawings before (red) and after (green) under each animated layer, parts boxes each named part, arcs the paths of --track"
        )]
        overlay: Option<String>,
        #[arg(
            long,
            help = "With --overlay arcs (implied): a mark (c4:s12), a part (leaf, or lines:leaf) or a point (x,y) whose path across the clip or sheet is drawn; repeat for more"
        )]
        track: Vec<String>,
        #[arg(long, default_value_t = 1, help = "How deep into groups the overlay labels go")]
        depth: usize,
        #[arg(long, help = "Replay every stroke instead of using cached tiles")]
        no_cache: bool,
        #[arg(long, help = "Show reference layers (photos to trace over), which renders leave out")]
        references: bool,
        #[arg(long, value_name = "BEFORE,AFTER", value_parser = parse_onion_counts, help = "Onion skin: how many drawings (or frames) before and after (default 2,2)")]
        onion: Option<(u32, u32)>,
        #[arg(
            long,
            value_name = "drawings|frames",
            help = "Count the onion skin in drawings (default) or in frames"
        )]
        onion_by: Option<String>,
        #[arg(
            long,
            value_name = "LAYERS",
            value_delimiter = ',',
            help = "Layers that show onion skin (ids or names; default: every layer with more than one drawing)"
        )]
        onion_layers: Option<Vec<String>>,
        #[arg(long, help = "Onion skin without the red and green tint")]
        no_tint: bool,
        #[arg(long, value_name = "FROM-TO", value_parser = parse_span, help = "Treat these frames as a loop: the onion skin wraps around inside them")]
        onion_loop: Option<(u32, u32)>,
        #[arg(
            long,
            value_name = "all|FROM-TO|CLIP",
            help = "Render many frames in one grid, labelled with frame numbers, drawing ids and holds"
        )]
        sheet: Option<String>,
        #[arg(
            long,
            help = "With --sheet: one cell per drawing (frames that look the same share a cell)"
        )]
        drawings: bool,
        #[arg(
            long,
            default_value_t = 256,
            help = "With --sheet: the longest side of each cell in px"
        )]
        cell: u32,
        #[arg(long, help = "With --sheet: cells per row (default: about square)")]
        columns: Option<usize>,
    },
    #[command(
        about = "Paint a stroke with a brush along an SVG path or explicit points",
        long_about = "Paint with a brush along an SVG path (M, L, H, V, C, S, Q, T, Z; several subpaths make several strokes) or explicit points.\n\n  klad stroke art.draw l3 --brush ink --size 14 --color #1b1b1b --path 'M100 400 C 300 200 500 600 700 400' --pressure 0.1,1,0.05\n  klad stroke art.draw l3 --brush ink --size 6 --taper 12,30 --path 'M100 100 Q 200 40 300 120'\n  klad stroke art.draw l3 --points '10,10,0.5 40,30,1 80,35,0.2'\n\nPressure is spread evenly along the stroke. --taper thins the first and last px of the line whatever the pressure. Brushes are a bundle brush id or a library brush (klad brushes). Strokes are marked as agent strokes."
    )]
    Stroke {
        bundle: PathBuf,
        layer: String,
        #[arg(long)]
        path: Option<String>,
        #[arg(long, help = "x,y,pressure triples separated by spaces")]
        points: Option<String>,
        #[arg(long, default_value = "ink")]
        brush: String,
        #[arg(long, default_value = "#000000")]
        color: String,
        #[arg(long, default_value_t = 8.0)]
        size: f32,
        #[arg(long, default_value_t = 1.0)]
        opacity: f32,
        #[arg(long, default_value_t = 1.0)]
        flow: f32,
        #[arg(long, help = "Pressure along the stroke, for example 0.2,1,0.1")]
        pressure: Option<String>,
        #[arg(
            long,
            default_value_t = 700.0,
            help = "Pen speed in px/s, sets sample spacing and timing"
        )]
        speed: f32,
        #[arg(long)]
        seed: Option<u32>,
        #[arg(
            long,
            help = "Taper in and out by length in px, for example 12,30 (default: the brush's)"
        )]
        taper: Option<String>,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[arg(long, help = "Put the new marks in this part of the drawing (see klad part)")]
        part: Option<String>,
        #[arg(
            long,
            help = "The second colour, for brushes that mix towards it with pressure or along the stroke"
        )]
        second: Option<String>,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Fill a region of a paint layer, inside the lines (flood) or inside a polygon (lasso)",
        long_about = "Fill a region of a paint layer. A flood fill starts at --at and spreads over pixels within --tolerance of that pixel in the reference: this layer (default), all (everything visible) or a layer id or name. --gap closes openings in the lines up to that many px wide; --expand grows the fill under the lines by that many px. --polygon fills a lasso polygon instead. The fill is stored with its coverage, so it renders the same later; klad refill recomputes it after the lines change.\n\n  klad fill art.draw l2 --at 820,640 --ref lines --gap 4 --color '#c84a3a'\n  klad fill art.draw l2 --at 820,640 --ref all --color skin\n  klad fill art.draw l2 --polygon '100,100 400,120 260,380' --color '#20304a'\n\nColours are #rrggbb, #rrggbbaa or a palette name. Also a fill op in klad apply: {\"op\":\"fill\",\"layer\":\"l2\",\"at\":[820,640],\"ref\":\"lines\",\"gap\":4,\"color\":\"#c84a3a\"}"
    )]
    Fill {
        bundle: PathBuf,
        layer: String,
        #[arg(long, value_parser = parse_pair, help = "Seed point x,y for a flood fill")]
        at: Option<[f32; 2]>,
        #[arg(long, help = "Lasso polygon: x,y pairs separated by spaces")]
        polygon: Option<String>,
        #[arg(long = "ref", help = "this (default), all, or a layer id or name")]
        reference: Option<String>,
        #[arg(long, default_value_t = TOLERANCE, help = "0..1, how different a pixel may be from the seed")]
        tolerance: f32,
        #[arg(long, default_value_t = 0.0, help = "Close openings in the lines up to this many px")]
        gap: f32,
        #[arg(long, default_value_t = EXPAND, help = "Grow the fill under the lines by this many px")]
        expand: f32,
        #[arg(long, default_value = "#000000")]
        color: String,
        #[arg(long, default_value_t = 1.0)]
        opacity: f32,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[arg(
            long,
            conflicts_with_all = ["at", "polygon"],
            help = "Carry the flood fills of this drawing (a cel of the layer) to the layer's other drawings"
        )]
        like: Option<String>,
        #[arg(
            long,
            requires = "like",
            help = "With --like: the frames whose drawings get them: all, FROM-TO or a clip"
        )]
        frames: Option<String>,
        #[arg(
            long,
            conflicts_with_all = ["at", "polygon", "like"],
            help = "Fill exactly the area of a vector shape (a group: each closed shape in it), grown by --expand"
        )]
        shape: Option<String>,
        #[arg(long, help = "Put the new marks in this part of the drawing (see klad part)")]
        part: Option<String>,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Ink the outlines of vector shapes as tapered strokes, the way an inker would",
        long_about = "Ink the outlines of vector shapes (rects, ellipses, paths, groups of them; often on a hidden sketch layer) as strokes on a paint layer. An outline is split into separate strokes at corners sharper than --break degrees, each stroke tapers in and out, and its weight follows rules instead of pressure written by hand: heavier on the side facing away from the light, heavier on the silhouette (the outer edge of the shapes inked together) than on inner lines, thinner along tight curves, with a small seeded wobble.\n\n  klad ink art.draw lines --from body,eye,leg --weight 6 --light -1,-1\n  klad ink art.draw lines --from rock --weight 9 --break 45 --gap 3 --wobble 1\n  klad ink art.draw lines --path \"M10 10 C 80 0 120 60 200 40\" --weight 4\n\nThe strokes are ordinary agent strokes: klad set, klad transform and the editor edit them. Also an ink op: {\"op\":\"ink\",\"layer\":\"l3\",\"from\":[\"n5\"],\"weight\":6,\"light\":[-1,-1],\"break\":60,\"gap\":0,\"wobble\":0.6}."
    )]
    Ink {
        bundle: PathBuf,
        layer: String,
        #[arg(
            long,
            value_delimiter = ',',
            help = "Vector shape ids; a group inks every shape in it"
        )]
        from: Vec<String>,
        #[arg(long, help = "SVG path data to ink instead of shapes")]
        path: Option<String>,
        #[arg(long, default_value_t = 6.0, help = "The full line weight in px")]
        weight: f32,
        #[arg(long, value_parser = parse_pair, allow_hyphen_values = true, help = "Where the light comes from, x,y (-1,-1: top left)")]
        light: Option<[f32; 2]>,
        #[arg(
            long = "break",
            default_value_t = 60.0,
            help = "Split strokes at corners sharper than this many degrees"
        )]
        break_deg: f32,
        #[arg(
            long,
            default_value_t = 0.0,
            help = "Leave this many px open at the ends of each stroke"
        )]
        gap: f32,
        #[arg(long, default_value_t = 0.6, help = "Seeded wobble in px (0: none)")]
        wobble: f32,
        #[arg(long, value_parser = parse_pair, help = "Taper in,out in px (default: the brush's)")]
        taper: Option<[f32; 2]>,
        #[arg(long, default_value = "ink")]
        brush: String,
        #[arg(long, default_value = "#000000")]
        color: String,
        #[arg(long, default_value_t = 1)]
        seed: u32,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[arg(long, help = "Put the new marks in this part of the drawing (see klad part)")]
        part: Option<String>,
        #[arg(
            long,
            default_value_t = 1.0,
            help = "How much each stroke is drawn like a hand: its own pressure curve, tapers and width (0 gives even strokes)"
        )]
        hand: f32,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Shade and rim light vector shapes from one light direction",
        long_about = "Make the two lighting layers of the flat-colour recipe from the shapes: on --shade, the side of each shape facing away from the light (the shape minus itself moved towards the light by --depth, in --soft overlapping steps so the inner edge is soft); on --rim, a thin band along the edge facing the light. Both are lasso fills with a cut, so with the layers clipped to the flats (multiply for shade, screen or add for rim) the light follows the shapes.\n\n  klad light art.draw --from body,head --light -1,-1 --shade shade --rim rim\n  klad light art.draw --from rock --shade shade --depth 24 --soft 4 --shade-color '#3a2f50'\n\nAlso a light op: {\"op\":\"light\",\"from\":[\"n5\"],\"light\":[-1,-1],\"shade\":\"l5\",\"rim\":\"l6\",\"depth\":14,\"soft\":3,\"rimWidth\":4}."
    )]
    Light {
        bundle: PathBuf,
        #[arg(
            long,
            value_delimiter = ',',
            help = "Vector shape ids; a group lights every closed shape in it"
        )]
        from: Vec<String>,
        #[arg(long, value_parser = parse_pair, allow_hyphen_values = true, help = "Where the light comes from, x,y (-1,-1: top left)")]
        light: Option<[f32; 2]>,
        #[arg(long, help = "The paint layer for the shade")]
        shade: Option<String>,
        #[arg(long, help = "The paint layer for the rim light")]
        rim: Option<String>,
        #[arg(long, default_value_t = 14.0, help = "How far the shade reaches into a shape, px")]
        depth: f32,
        #[arg(long, default_value_t = 3, help = "Overlapping shade steps (1: a hard edge)")]
        soft: u32,
        #[arg(long = "rim-width", default_value_t = 4.0)]
        rim_width: f32,
        #[arg(long = "shade-color", default_value = "#5a4a6a")]
        shade_color: String,
        #[arg(long = "rim-color", default_value = "#fff1cc")]
        rim_color: String,
        #[arg(long = "shade-opacity", default_value_t = 0.6)]
        shade_opacity: f32,
        #[arg(long = "rim-opacity", default_value_t = 0.85)]
        rim_opacity: f32,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[arg(
            long,
            allow_hyphen_values = true,
            help = "A lamp at x,y,height in canvas px: each shape is shaded away from it"
        )]
        lamp: Option<String>,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Fill a shape's area with short tapered strokes: hatching, bark, fur, stone grain",
        long_about = "Fill the area of closed vector shapes (or a polygon) with short tapered strokes, parallel at --angle or following the outline (--along), clipped to the area, with seeded variation.\n\n  klad hatch art.draw shade --in body --angle 35 --spacing 7 --length 10-24\n  klad hatch art.draw lines --in trunk --along --spacing 9 --weight 1.5 --color '#3b2a1e'\n\nAlso a hatch op: {\"op\":\"hatch\",\"layer\":\"l5\",\"in\":[\"n5\"],\"angle\":30,\"spacing\":6,\"length\":[12,30],\"jitter\":0.3,\"weight\":2}."
    )]
    Hatch {
        bundle: PathBuf,
        layer: String,
        #[arg(long = "in", value_delimiter = ',', help = "Closed vector shapes to hatch inside")]
        within: Vec<String>,
        #[arg(long, help = "Or a polygon: x,y pairs separated by spaces")]
        polygon: Option<String>,
        #[arg(long, default_value_t = 30.0, allow_hyphen_values = true)]
        angle: f32,
        #[arg(long, help = "Follow the outline instead of parallel lines")]
        along: bool,
        #[arg(long, default_value_t = 6.0)]
        spacing: f32,
        #[arg(long, value_parser = parse_range, default_value = "12-30", help = "Stroke lengths MIN-MAX px")]
        length: [f32; 2],
        #[arg(long, default_value_t = 0.3)]
        jitter: f32,
        #[arg(long, default_value_t = 2.0, help = "Stroke size in px")]
        weight: f32,
        #[arg(long, default_value = "ink")]
        brush: String,
        #[arg(long, default_value = "#000000")]
        color: String,
        #[arg(long, default_value_t = 1.0)]
        opacity: f32,
        #[arg(long, default_value_t = 1)]
        seed: u32,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[arg(long, help = "Put the new marks in this part of the drawing (see klad part)")]
        part: Option<String>,
        #[arg(
            long,
            default_value_t = 1.0,
            help = "How much each stroke is drawn like a hand: its own pressure curve, tapers and width (0 gives even strokes)"
        )]
        hand: f32,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Paint a shape as a form, with strokes that follow it and colours from a light model",
        long_about = "Paint closed vector shapes (or a polygon) as forms. --form round bulges any closed shape like a pillow (its height grows with the distance to its outline), flat is a plane facing --facing x,y,z, cylinder is round across --axis degrees. The light model: a key light from --light x,y at --elevation (or a --lamp x,y,height in canvas px; the shape nearest the lamp gets the full key light and farther ones less, by inverse square), cool fill from above (--fill, --fill-strength), warm bounce from below (--bounce, --bounce-strength) and a darker core just past the terminator (--core). --color is the local colour in full light.\n\nStrokes follow the form: along bands of equal light on round forms, along the axis on cylinders, at --angle on planes. Each takes its colour from the model where it lies and stops where the light changes, so no stroke carries the lit colour into the shadow. Passes go from 2x --weight down to half of it (the small ones only where the form turns), then a decisive pass with --bold in the lights and a few quiet strokes in the shadows (--decisive 0 skips both). Every stroke is drawn like a hand (--hand): its own pressure curve, tapers and width.\n\nPaint on a layer clipped to the flats, or lock the flats layer and paint into it: then strokes run past the outline and a pass of short strokes along it cuts the edge in, so no flat colour shows at the rim. On a plain layer strokes stay inside the outline.\n\n  klad paint art.draw paint --in rock --color '#8a8f96' --light -1,-1\n  klad paint art.draw paint --in pillar --form cylinder --axis 90 --color '#c9b8a0' --lamp 200,600,150\n  klad paint art.draw paint --in wall --form flat --facing 0,0,1 --angle 0 --weight 10\n\nAlso a paint op: {\"op\":\"paint\",\"layer\":\"l5\",\"in\":[\"n5\"],\"color\":\"#5d6a78\",\"light\":[-1,-1],\"weight\":14,\"form\":\"round\"}."
    )]
    Paint {
        bundle: PathBuf,
        layer: String,
        #[arg(long = "in", value_delimiter = ',', help = "Closed vector shapes to paint inside")]
        within: Vec<String>,
        #[arg(long, help = "Or a polygon: x,y pairs separated by spaces")]
        polygon: Option<String>,
        #[arg(long, default_value = "oil")]
        brush: String,
        #[arg(
            long,
            default_value = "oil-load",
            help = "The brush for the decisive strokes in the lights"
        )]
        bold: String,
        #[arg(long, default_value = "#808080", help = "The local colour in full light")]
        color: String,
        #[arg(long, default_value = "round", help = "round, flat or cylinder")]
        form: String,
        #[arg(
            long,
            default_value_t = 90.0,
            allow_hyphen_values = true,
            help = "A cylinder's axis in degrees (90 is vertical)"
        )]
        axis: f32,
        #[arg(
            long,
            default_value = "0,0,1",
            allow_hyphen_values = true,
            help = "A flat form's facing x,y,z (z toward the viewer, y down)"
        )]
        facing: String,
        #[arg(long, value_parser = parse_pair, default_value = "-1,-1", allow_hyphen_values = true, help = "Where the light comes from, as a direction x,y")]
        light: [f32; 2],
        #[arg(
            long,
            default_value_t = 0.6,
            help = "How far the light is toward the viewer (0 grazes the picture)"
        )]
        elevation: f32,
        #[arg(
            long,
            allow_hyphen_values = true,
            help = "A lamp at x,y,height in canvas px instead of a direction"
        )]
        lamp: Option<String>,
        #[arg(long, default_value = "#fff3e2", help = "The key light's colour")]
        key: String,
        #[arg(long, default_value = "#8a9ab8", help = "The fill light's colour, from above")]
        fill: String,
        #[arg(long, default_value_t = 0.16)]
        fill_strength: f32,
        #[arg(long, default_value = "#b08a66", help = "The bounce light's colour, from below")]
        bounce: String,
        #[arg(long, default_value_t = 0.1)]
        bounce_strength: f32,
        #[arg(
            long,
            default_value_t = 0.35,
            help = "How much darker the core just past the terminator is"
        )]
        core: f32,
        #[arg(
            long,
            help = "The shadow side takes this colour's hue and chroma and keeps its own values (a colour or a palette name)"
        )]
        shadow: Option<String>,
        #[arg(
            long,
            default_value_t = 0.7,
            help = "How far the shadow side goes towards --shadow, 0 to 1"
        )]
        shadow_mix: f32,
        #[arg(
            long,
            default_value_t = 30.0,
            allow_hyphen_values = true,
            help = "Stroke direction on flat forms, degrees"
        )]
        angle: f32,
        #[arg(long, default_value_t = 14.0, help = "Stroke size in px of the middle pass")]
        weight: f32,
        #[arg(
            long,
            default_value_t = 1.0,
            help = "How many decisive and quiet strokes, 0 for none"
        )]
        decisive: f32,
        #[arg(long, default_value_t = 0.03, help = "How much each stroke's value strays")]
        vary: f32,
        #[arg(
            long,
            default_value_t = 1.0,
            help = "How much each stroke is drawn like a hand (0 gives even strokes)"
        )]
        hand: f32,
        #[arg(long, default_value_t = 1.0)]
        opacity: f32,
        #[arg(long, default_value_t = 1)]
        seed: u32,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[arg(long, help = "Put the new marks in this part of the drawing (see klad part)")]
        part: Option<String>,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Buffer the drawings of a span of frames, as the editor's prepare does",
        long_about = "Replay every painted drawing shown in frames --from to --to (all frames by default) into the bundle's cache, so the editor and later renders read them instead of replaying their strokes. Run it after a big change made by the CLI, before the editor opens or plays the file.\n\n  klad prepare art.draw\n  klad prepare art.draw --from 0 --to 23"
    )]
    Prepare {
        bundle: PathBuf,
        #[arg(long)]
        from: Option<u32>,
        #[arg(long)]
        to: Option<u32>,
    },
    #[command(
        about = "Scatter copies of a small mark over a shape: grass, cracks, dots",
        long_about = "Scatter copies of a small mark (SVG path data around 0,0) inside closed vector shapes (or a polygon), avoiding other shapes, each copy scaled and turned a little (seeded).\n\n  klad scatter art.draw lines --in ground --mark \"M0 0 L0 -18\" --count 40 --size 0.7-1.2 --turn -15,15\n  klad scatter art.draw lines --in rock --mark \"M0 0 L6 3 L10 -2\" --spacing 22 --avoid moss\n\nAlso a scatter op: {\"op\":\"scatter\",\"layer\":\"l3\",\"in\":[\"n5\"],\"mark\":\"M0 0 L0 -18\",\"count\":40,\"size\":[0.7,1.2],\"turn\":[-15,15],\"avoid\":[\"n9\"]}."
    )]
    Scatter {
        bundle: PathBuf,
        layer: String,
        #[arg(long = "in", value_delimiter = ',', help = "Closed vector shapes to scatter inside")]
        within: Vec<String>,
        #[arg(long, help = "Or a polygon: x,y pairs separated by spaces")]
        polygon: Option<String>,
        #[arg(long, value_delimiter = ',', help = "Shapes to keep clear of")]
        avoid: Vec<String>,
        #[arg(long, default_value = "M0 0 L0 -18", allow_hyphen_values = true)]
        mark: String,
        #[arg(long, help = "How many marks (default: one every --spacing px)")]
        count: Option<usize>,
        #[arg(long, default_value_t = 14.0)]
        spacing: f32,
        #[arg(long, value_parser = parse_range, default_value = "0.7-1.2", help = "Scale of each copy MIN-MAX")]
        size: [f32; 2],
        #[arg(long, value_parser = parse_pair, allow_hyphen_values = true, default_value = "-15,15", help = "Turn of each copy MIN,MAX degrees")]
        turn: [f32; 2],
        #[arg(long, default_value_t = 2.0, help = "Stroke size in px")]
        weight: f32,
        #[arg(long, default_value = "ink")]
        brush: String,
        #[arg(long, default_value = "#000000")]
        color: String,
        #[arg(long, default_value_t = 1.0)]
        opacity: f32,
        #[arg(long, default_value_t = 1)]
        seed: u32,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[arg(long, help = "Put the new marks in this part of the drawing (see klad part)")]
        part: Option<String>,
        #[arg(
            long,
            default_value_t = 1.0,
            help = "How much each stroke is drawn like a hand: its own pressure curve, tapers and width (0 gives even strokes)"
        )]
        hand: f32,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Paint closed shapes as clumps of leaves, lit as forms",
        long_about = "Build each closed shape out of leaves: leaf marks across the clump take their colour from a round form lit by --light (or a --lamp x,y,height), in a few value steps from --shade through --color to --lit (by default darker and cooler, lighter and warmer than --color), with the most variety in the light; then leaves along the outline, most on the side facing the light and the sky, a few glowing (--glow) at the shadow side's edge, so the silhouette breaks up and the sky shows between them. --mass names a shape around the whole crown, so every clump also turns with the big form. --leaf is the leaf size in px, --density how close the leaves sit. Fill a darker, slightly smaller core under each clump first and paint clumps back to front on one layer. The brush is leaves (several leaf shapes, one per dab).\n\n  klad foliage tree.draw canopy --in clump1 --color '#4d7a3b' --light -0.6,-0.8 --leaf 16 --mass crown\n\nAlso a foliage op: {\"op\":\"foliage\",\"layer\":\"canopy\",\"in\":[\"clump1\"],\"color\":\"#4d7a3b\",\"light\":[-0.6,-0.8],\"leaf\":16,\"mass\":\"crown\"}."
    )]
    Foliage {
        bundle: PathBuf,
        layer: String,
        #[arg(
            long = "in",
            value_delimiter = ',',
            help = "Closed vector shapes, each a clump of leaves"
        )]
        within: Vec<String>,
        #[arg(long, default_value = "#4f7a3c", help = "The leaves' middle colour")]
        color: String,
        #[arg(long, help = "The lit leaves' colour (default: lighter and warmer)")]
        lit: Option<String>,
        #[arg(long, help = "The shadowed leaves' colour (default: darker and cooler)")]
        shade: Option<String>,
        #[arg(
            long,
            help = "Light through thin leaves at the shadow edge (default: a warm yellow-green)"
        )]
        glow: Option<String>,
        #[arg(long, value_parser = parse_pair, allow_hyphen_values = true, default_value = "-1,-1")]
        light: [f32; 2],
        #[arg(long, help = "A lamp at x,y,height in canvas px instead of a direction")]
        lamp: Option<String>,
        #[arg(long, default_value_t = 14.0, help = "Leaf size in px")]
        leaf: f32,
        #[arg(long, default_value_t = 1.0, help = "How close the leaves sit (2 is twice as many)")]
        density: f32,
        #[arg(
            long,
            help = "A shape around the whole crown, so each clump also turns with the big form"
        )]
        mass: Option<String>,
        #[arg(long, default_value = "leaves")]
        brush: String,
        #[arg(long, help = "Only the leaves along the outline, not across the clump")]
        edges_only: bool,
        #[arg(long, default_value_t = 1)]
        seed: u32,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[arg(long, help = "Put the new marks in this part of the drawing (see klad part)")]
        part: Option<String>,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Add, remove, rename, move or reparent a bone of a layer's or group's skeleton",
        long_about = "Bones go on a paint layer or a group (a group's skeleton poses every paint layer in it together). A bone has a name, an optional parent, a head and a tail in canvas px; a child usually starts where its parent ends. Removing a bone removes its children.\n\n  klad bone art.draw body --add hip --head 400,500 --tail 400,420\n  klad bone art.draw body --add thigh --parent hip --head 400,500 --tail 410,620\n  klad bone art.draw body --rename thigh --as femur\n  klad bone art.draw body --move femur --tail 420,630\n  klad bone art.draw body --reparent femur --parent pelvis\n  klad bone art.draw body --rm femur\n\nAlso a bone op: {\"op\":\"bone\",\"layer\":\"body\",\"add\":\"thigh\",\"parent\":\"hip\",\"head\":[400,500],\"tail\":[410,620]}."
    )]
    Bone {
        bundle: PathBuf,
        layer: String,
        #[arg(long)]
        add: Option<String>,
        #[arg(long)]
        rm: Option<String>,
        #[arg(long)]
        rename: Option<String>,
        #[arg(long = "as")]
        to_name: Option<String>,
        #[arg(long = "move")]
        moved: Option<String>,
        #[arg(long)]
        reparent: Option<String>,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long, value_parser = parse_pair, allow_hyphen_values = true)]
        head: Option<[f32; 2]>,
        #[arg(long, value_parser = parse_pair, allow_hyphen_values = true)]
        tail: Option<[f32; 2]>,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(about = "List the skeletons, their bones and weights, and the posed drawings")]
    Bones {
        bundle: PathBuf,
        #[arg(long)]
        json: bool,
    },
    #[command(
        about = "Set how much each bone moves each part of the drawing: automatic, painted dabs, a whole part, smoothed",
        long_about = "Every bone has a weight map over the rest drawing (a grid at a quarter of the canvas resolution); where weights blend, lines bend smoothly; a part held by one bone at weight 1 moves rigidly. --auto starts from the bones (by distance, blending across the joints). --set paints soft round dabs of a weight onto --bone: 'x,y,radius,weight ...'. --part makes a named part of the drawing (at --frame) fully that bone's (and no other's). --smooth blurs the maps a little. render --overlay weights:<bone> shows a bone's map.\n\n  klad weights art.draw body --auto\n  klad weights art.draw body --bone shin --set '400,600,30,1 410,640,20,0.8'\n  klad weights art.draw body --bone head --part head\n\nAlso a weights op: {\"op\":\"weights\",\"layer\":\"body\",\"bone\":\"shin\",\"set\":[[400,600,30,1]]}."
    )]
    Weights {
        bundle: PathBuf,
        layer: String,
        #[arg(long)]
        auto: bool,
        #[arg(long)]
        bone: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        set: Option<String>,
        #[arg(long)]
        part: Option<String>,
        #[arg(long, default_value_t = 1.0)]
        value: f32,
        #[arg(long)]
        smooth: bool,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Pose a drawing with its bones: a new drawing at --to, bent from the rest drawing at --from",
        long_about = "A pose is a turn per bone (degrees from its rest angle, children follow their parent) and a move of the whole (--root). Every paint layer under the skeleton gets a new drawing at --to: each stroke point and fill moves by its bones' weighted blend, the same way Godot's Skeleton2D bends a Polygon2D. The pose is kept on the drawing; posing that frame again re-poses it from the same rest drawing, and posing from a posed drawing starts from its rest drawing. klad inbetween between two poses of one rest drawing turns the bones instead of matching strokes.\n\n  klad pose art.draw body --from 0 --to 4 --bone thigh=25 --bone shin=-40 --root 0,6\n\nAlso a pose op: {\"op\":\"pose\",\"layer\":\"body\",\"from\":0,\"to\":4,\"bones\":{\"thigh\":25,\"shin\":-40},\"root\":[0,6]}."
    )]
    Pose {
        bundle: PathBuf,
        layer: String,
        #[arg(long, default_value_t = 0)]
        from: u32,
        #[arg(long)]
        to: u32,
        #[arg(long = "bone", help = "name=degrees; repeat for more", allow_hyphen_values = true)]
        bones: Vec<String>,
        #[arg(long, value_parser = parse_pair, allow_hyphen_values = true, default_value = "0,0")]
        root: [f32; 2],
        #[arg(long)]
        hold: Option<u32>,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Measure a drawing against a reference picture: values, saturation, texture, edges, colour per region",
        long_about = "Render the drawing (or read a PNG or JPEG) at the reference's size and compare the two in OKLab: the value range (L at the 5th, 50th and 95th percentile), mean chroma, stroke texture (mean absolute Laplacian of L), edge sharpness (99th percentile of the gradient) and the mean colour difference, with what differs in words (lighter, bluer, busier, softer edges). --region adds a row per named rectangle; --heat writes a picture of where they differ most (black, red, yellow, white as the difference grows to 20).\n\n  klad compare cafe.draw reference.png --region shade=40,300,200,120 --region cup=610,180,90,120 --heat diff.png"
    )]
    Compare {
        drawing: PathBuf,
        reference: PathBuf,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[arg(long = "region", help = "name=x,y,w,h in the reference's pixels; repeat for more")]
        regions: Vec<String>,
        #[arg(long)]
        heat: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    #[command(
        about = "Paint over a picture with brushwork: strokes that follow its forms and stop at its colour edges, big to small",
        long_about = "Repaint what is below a paint layer (or the layers in --from) with strokes on that layer: big brushes first, then smaller ones only where the painting still differs from the source by more than --threshold (a colour difference in OKLab, x100), so detail lands where the source has it. Each stroke takes the source's colour where it starts, broken a little warm and cool (--vary), and runs along the forms (across the colour gradient) until the colour under it changes, it reaches --length (in brush widths, min,max) or it would bend more than --curve allows (0 straight, 1 follows every turn). --in limits it to closed shapes; --focus lets the smallest brush work only in and near one shape. The strokes are ordinary strokes in the part \"paint over\" (--part), one history step, so they take body and relief and can be edited.\n\nFor animation, paint over one key, then carry it to the next with --like <cel>: every stroke moves with the marks under it, takes its colour from the new drawing, and strokes are only added (part \"paint over added\") where the new drawing needs them. The carried strokes keep their order, so klad inbetween pairs them part by part and nothing boils.\n\n  klad paintover still.draw brushwork --sizes 32,16,8 --threshold 6\n  klad paintover walk.draw brushwork --frame 4 --like c12\n\nAlso a paintover op, alone in its batch: {\"op\":\"paintover\",\"layer\":\"brushwork\",\"sizes\":[32,16,8],\"threshold\":6}."
    )]
    Paintover {
        bundle: PathBuf,
        layer: String,
        #[arg(
            long,
            value_delimiter = ',',
            help = "Layers to paint from (default: everything below the layer, with the background)"
        )]
        from: Vec<String>,
        #[arg(long = "in", value_delimiter = ',', help = "Closed vector shapes to paint inside")]
        within: Vec<String>,
        #[arg(long, value_delimiter = ',', default_values_t = [32.0, 16.0, 8.0], help = "Brush sizes in px, biggest first")]
        sizes: Vec<f32>,
        #[arg(
            long,
            default_value_t = 6.0,
            help = "How far the painting may differ from the source (OKLab difference x100)"
        )]
        threshold: f32,
        #[arg(long, default_value = "1,8", help = "Stroke length in brush widths: min,max")]
        length: String,
        #[arg(
            long,
            default_value_t = 0.5,
            help = "How closely strokes follow the forms' turns (0 to 1)"
        )]
        curve: f32,
        #[arg(long, default_value = "oil")]
        brush: String,
        #[arg(long, help = "A closed shape: the smallest brush only paints in and near it")]
        focus: Option<String>,
        #[arg(
            long,
            default_value_t = 0.025,
            help = "Warm and cool, light and dark break per stroke (OKLab)"
        )]
        vary: f32,
        #[arg(
            long,
            help = "Carry the paint-over of this drawing (a cel id on the same layer) to this frame"
        )]
        like: Option<String>,
        #[arg(
            long,
            help = "With --like: carry it from drawing to drawing over these frames (a-b), making a drawing on the layer wherever the source starts one"
        )]
        frames: Option<String>,
        #[arg(long, default_value = "paint over")]
        part: String,
        #[arg(long, default_value_t = 1)]
        seed: u32,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Grow grass in closed shapes: blades lit by the ground, smaller with distance, breaking the top edge",
        long_about = "Fill closed shapes with grass blades from the far (top) edge to the near (bottom) one: each blade rises from its root, leans with --wind (degrees) and bends, and is smaller and denser towards the top (--far is the far size against the near one). The ground under them rolls in soft patches about --patch px wide, lit by --light (or a --lamp x,y,height): each blade runs from a dark root to a lit tip in a few value families from --shade through --color to --lit. Along the top edge taller blades (--edge) break the outline against what is behind. Near blades are drawn last.\n\nWith a stroke brush (blade, the default, or any round or chalk tip) each blade is a tapered stroke whose colour runs from --color at the root to the lit colour at the tip when the brush blends to a second colour by pressure. With a picture brush (a tip made of pictures, like your own tufts) each blade is one stamp of the picture, turned to the blade. Build your own brush for your grass (klad brushes --picture, --set) and pass it with --brush: the op decides where the marks go and how they are lit, the brush decides what they look like.\n\n  klad grass meadow.draw field --in field-shape --color '#6f8a3c' --light -0.9,-0.45 --blade 30 --far 0.22\n  klad grass meadow.draw bank --in bank --brush my-tuft --blade 160 --density 0.1\n\nAlso a grass op: {\"op\":\"grass\",\"layer\":\"field\",\"in\":[\"field-shape\"],\"color\":\"#6f8a3c\",\"blade\":30,\"far\":0.22,\"wind\":12}."
    )]
    Grass {
        bundle: PathBuf,
        layer: String,
        #[arg(long = "in", value_delimiter = ',', help = "Closed vector shapes to grow grass in")]
        within: Vec<String>,
        #[arg(long, default_value = "#5f8a3e", help = "The grass's middle colour")]
        color: String,
        #[arg(long, help = "The lit tips' colour (default: lighter and warmer)")]
        lit: Option<String>,
        #[arg(long, help = "The shadowed roots' colour (default: darker and cooler)")]
        shade: Option<String>,
        #[arg(long, value_parser = parse_pair, allow_hyphen_values = true, default_value = "-1,-1")]
        light: [f32; 2],
        #[arg(long, help = "A lamp at x,y,height in canvas px instead of a direction")]
        lamp: Option<String>,
        #[arg(long, default_value_t = 36.0, help = "Blade length in px at the near edge")]
        blade: f32,
        #[arg(long, help = "Blade width in px at the near edge (default: an eighth of the length)")]
        width: Option<f32>,
        #[arg(long, default_value_t = 0.45, help = "Size at the far edge against the near one")]
        far: f32,
        #[arg(
            long,
            default_value_t = 8.0,
            allow_hyphen_values = true,
            help = "How far the blades lean, degrees (negative leans left)"
        )]
        wind: f32,
        #[arg(
            long,
            default_value_t = 1.0,
            help = "How close the blades stand (2 is twice as many)"
        )]
        density: f32,
        #[arg(
            long,
            default_value_t = 1.35,
            help = "How much taller the blades along the top edge are"
        )]
        edge: f32,
        #[arg(
            long,
            help = "Width of the ground's light patches in px (default: four blade lengths)"
        )]
        patch: Option<f32>,
        #[arg(long, default_value = "blade")]
        brush: String,
        #[arg(
            long,
            help = "The species, as JSON: [{\"name\":\"fine\",\"share\":0.5,\"width\":0.7},{\"brush\":\"my-clover\",\"low\":true,\"share\":0.2}] (default: a meadow mix of fine, broad, dry, seed heads and clover)"
        )]
        kinds: Option<String>,
        #[arg(long, default_value_t = 1)]
        seed: u32,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[arg(long, help = "Put the new marks in this part of the drawing (see klad part)")]
        part: Option<String>,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "List closed regions of the lines that no fill on the layer covers",
        long_about = "List the closed regions of a fill layer's reference (the lines its flood fills fill against) that no fill on the layer covers: pockets a fill missed, or drawings not coloured yet. Regions open to the canvas edge are the outside and do not count. Each pocket prints its bounds and size, and a render of the frame marks them in magenta with a box.\n\n  klad gaps art.draw colour --frame 4\n  klad gaps art.draw colour --sheet all\n  klad gaps art.draw colour --sheet walk --ref lines --gap 4\n\nThe reference, gap and tolerance default to those of the layer's flood fills on those frames."
    )]
    Gaps {
        bundle: PathBuf,
        layer: String,
        #[arg(long, conflicts_with = "sheet")]
        frame: Option<u32>,
        #[arg(long, value_name = "all|FROM-TO|CLIP", help = "Check many frames")]
        sheet: Option<String>,
        #[arg(long = "ref", help = "this, all or a layer (default: what the layer's fills use)")]
        reference: Option<String>,
        #[arg(long, help = "Close openings up to this many px (default: the fills' gap)")]
        gap: Option<f32>,
        #[arg(long)]
        tolerance: Option<f32>,
        #[arg(long, default_value_t = 12, help = "Ignore pockets smaller than this many px")]
        min: usize,
        #[arg(long, help = "Render the frames even when nothing is found")]
        render: bool,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Recompute the flood fills of a paint cel after the lines they were made from changed")]
    Refill {
        bundle: PathBuf,
        layer: String,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[arg(long, help = "Only this fill (klad strokes lists fill ids)")]
        id: Option<u32>,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "The document palette: list, --add a colour (with --name), --rm a name, colour or index",
        long_about = "The document palette, kept in doc.json. Named colours can be used wherever a colour goes (--color skin).\n\n  klad palette art.draw\n  klad palette art.draw --add '#f0c8a0' --name skin\n  klad palette art.draw --add '#d0a080' --name skin    (changes skin)\n  klad palette art.draw --rm skin\n\nAs ops: {\"op\":\"palette\",\"add\":\"#f0c8a0\",\"name\":\"skin\"}, {\"op\":\"palette\",\"rm\":\"skin\"}, {\"op\":\"palette\",\"set\":[{\"color\":\"#1b1b1b\",\"name\":\"ink\"}]}"
    )]
    Palette {
        bundle: PathBuf,
        #[arg(long)]
        add: Option<String>,
        #[arg(long, requires = "add")]
        name: Option<String>,
        #[arg(long, conflicts_with = "add")]
        rm: Option<String>,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Export PNGs, sprite sheets, numbered frames or a Godot SpriteFrames and scene",
        long_about = "Export a transparent PNG of the whole image or one layer or group, at a frame, at any scale from 1/16 to 8 (vectors stay sharp; paint layers are resampled). 8-bit PNGs are dithered from the 16-bit float pixels so gradients do not band; --depth 16 writes 16-bit PNGs. --background keeps the canvas background colour.\n\n  klad export art.draw\n  klad export art.draw --scale 2\n  klad export art.draw --layer lines --scale 4 --depth 16 -o lines.png\n\nAnimations (each clip, or the whole timeline when there are no clips):\n\n  klad export art.draw --sheet all [--scale 0.5] [--padding 2] [--max 4096]\n      a PNG holding each distinct picture once, trimmed to its pixels (more pages, art-sheet-2.png ..., when it does not fit --max), plus art-sheet.json in Aseprite's array layout: one entry per exposure with its duration in ms, frameTags for clips\n  klad export art.draw --sheet run            one clip; --sheet 0-11 a range\n  klad export art.draw --frames run           numbered full-size PNGs, one per frame (run-0000.png ...) in art-run/\n  klad export art.draw --godot ../game/art    the sheet, art.tres (SpriteFrames: one animation per clip with its fps, loop and per-frame durations) and art.tscn (an AnimatedSprite2D placed so the timeline's origin sits on the node's position). The first folder given is kept in the document (export.godot); later exports and the editor's button write there, and Godot uids are kept so scenes using them keep working. --store replaces a kept folder."
    )]
    Export {
        bundle: PathBuf,
        #[arg(long, help = "PNG output (the default for a single image)")]
        png: bool,
        #[arg(long, help = "A layer or group id or name instead of the whole image")]
        layer: Option<String>,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[arg(long, default_value_t = 1.0)]
        scale: f32,
        #[arg(long, default_value_t = 8, help = "8 (dithered) or 16 bits per channel")]
        depth: u8,
        #[arg(long, help = "Keep the canvas background instead of transparency")]
        background: bool,
        #[arg(long, help = "Keep reference layers (photos to trace over), which exports leave out")]
        references: bool,
        #[arg(
            long,
            conflicts_with_all = ["frames", "godot"],
            help = "Sprite sheet and Aseprite JSON: all, a clip or a range like 0-11"
        )]
        sheet: Option<String>,
        #[arg(
            long,
            conflicts_with = "godot",
            help = "Numbered PNGs, one per frame: all, a clip or a range"
        )]
        frames: Option<String>,
        #[arg(
            long,
            num_args = 0..=1,
            help = "Godot SpriteFrames and AnimatedSprite2D scene into this folder (default: the document's export.godot)"
        )]
        godot: Option<Option<PathBuf>>,
        #[arg(
            long,
            requires = "godot",
            help = "Keep the --godot folder as the document's, replacing the one it has"
        )]
        store: bool,
        #[arg(long, default_value_t = 2, help = "Sheets: transparent pixels around each picture")]
        padding: u32,
        #[arg(long, default_value_t = 4096, help = "Sheets: largest page side in px")]
        max: u32,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    #[command(
        about = "List brushes: the library, and with a bundle the ones it embeds",
        long_about = "List brushes: the built-in library, and with a bundle the brushes it embeds (a document keeps every brush it uses in brushes/<id>.json, so it renders the same when the library changes).\n\n  klad brushes\n  klad brushes art.draw\n  klad brushes art.draw --show ink"
    )]
    Brushes {
        bundle: Option<PathBuf>,
        #[arg(long, help = "Print one brush in full (JSON)")]
        show: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, num_args = 2, value_names = ["ID", "FILE"], help = "Add a brush to the bundle from a JSON file (- for stdin)")]
        set: Option<Vec<String>>,
        #[arg(long, num_args = 2, value_names = ["NAME", "PNG"], help = "Add a picture (tip or texture) to the bundle's brushes/images")]
        picture: Option<Vec<String>>,
    },
    #[command(
        about = "List, add or remove a layer's effects: klad effect art.draw l3 glow radius=24 color=#ffd080",
        long_about = "Effects apply in order when the layer composes: blur (radius), glow (radius, strength, threshold, color, blend), outline (width, color, inside), shadow (x, y, radius, color), noise (amount, size, seed, mono), displace (amount, size, seed, by: a layer id whose red and green push the pixels). With no effect name it lists them."
    )]
    Effect {
        bundle: PathBuf,
        layer: String,
        #[arg(help = "blur, glow, outline, shadow, noise or displace, then key=value pairs")]
        rest: Vec<String>,
        #[arg(long, help = "Remove the effect at this position (0 is the first)")]
        rm: Option<usize>,
        #[arg(long, help = "Remove every effect")]
        clear: bool,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(
        about = "Bend a brush stroke along a new path; it keeps its pressure, tilt and timing (klad strokes --json gives each stroke's fitted path)"
    )]
    Reshape {
        bundle: PathBuf,
        #[arg(help = "The stroke, <cel>:s<id>")]
        stroke: String,
        #[arg(long, help = "SVG path data the stroke follows from start to end")]
        path: String,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(about = "Undo the last change (from the CLI or the editor)")]
    Undo {
        bundle: PathBuf,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(about = "Redo the last undone change")]
    Redo {
        bundle: PathBuf,
        #[command(flatten)]
        flags: EditFlags,
    },
    #[command(about = "Recent changes, newest first")]
    Log {
        bundle: PathBuf,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "What the human has selected in the editor (layer and shapes, with bounds)")]
    Selection {
        bundle: PathBuf,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Notes the human pinned on the canvas; --done <id> marks one as handled")]
    Notes {
        bundle: PathBuf,
        #[arg(long)]
        done: Option<u32>,
        #[arg(long, help = "Include notes already marked done")]
        all: bool,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Before and after renders of a change, side by side (default: the last one)")]
    Diff {
        bundle: PathBuf,
        #[arg(long, help = "History step number from klad log")]
        step: Option<u64>,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[arg(long)]
        scale: Option<f32>,
    },
    #[command(about = "Paint cels with stroke counts and file sizes")]
    Info { bundle: PathBuf },
    #[command(about = "List the strokes and fills of a paint layer's cel at a frame")]
    Strokes {
        bundle: PathBuf,
        layer: String,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[arg(long, default_value_t = 0, help = "Only strokes with an id above this")]
        after: u32,
        #[arg(long, help = "Print strokes as JSON with their points (x, y, pressure)")]
        json: bool,
    },
    #[command(about = "Print pixel hashes, for comparing macOS and Linux renders")]
    Hash {
        bundle: PathBuf,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[arg(long, help = "One line per frame of the timeline")]
        all: bool,
        #[arg(long)]
        no_cache: bool,
    },
    #[command(about = "Write a sample bundle (vector sky and hills, painted glow, clouds, hatching, grass)")]
    Demo {
        bundle: PathBuf,
        #[arg(long, default_value_t = 2048)]
        size: u32,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        #[arg(long, help = "Replace an existing bundle")]
        force: bool,
    },
    #[command(about = "Create a large bundle of synthetic paint layers for editor performance tests")]
    Stress {
        bundle: PathBuf,
        #[arg(long, default_value_t = 4096)]
        size: u32,
        #[arg(long, default_value_t = 30)]
        layers: usize,
        #[arg(long, default_value_t = 300)]
        strokes: usize,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        #[arg(
            long,
            default_value_t = 1,
            help = "Timeline length; a drawing per layer every --every frames"
        )]
        frames: u32,
        #[arg(long, default_value_t = 2)]
        every: u32,
    },
    #[command(about = "Append synthetic ink strokes to a paint layer (sync test)")]
    Scribble {
        bundle: PathBuf,
        #[arg(long)]
        layer: Option<String>,
        #[arg(long, default_value_t = 0)]
        frame: u32,
        #[arg(long, default_value_t = 1)]
        count: usize,
        #[arg(long)]
        seed: Option<u64>,
        #[arg(long, help = "Brush size for every stroke (default: varied)")]
        size: Option<f32>,
        #[arg(long, default_value = "#bf2620")]
        color: String,
    },
    #[command(about = "Print editor saves as they arrive, with how long they took to show up here")]
    Watch {
        bundle: PathBuf,
        #[arg(long, help = "Poll instead of native file events")]
        poll: bool,
        #[arg(long, default_value_t = 120)]
        seconds: u64,
    },
    #[command(
        about = "Print the licence and the third-party notices (every open source part klad is built on)",
        alias = "licenses"
    )]
    Licences,
    #[command(
        about = "Put klad on the terminal's path, for you and for agents",
        long_about = "Put klad on the terminal's path, for you and for agents. macOS: a link in /usr/local/bin (asks for your password when that folder is not yours). Linux: a link in ~/.local/bin. Windows: this folder is added to your PATH. --dir puts the link in another folder.\n\n  klad install\n  klad install --dir ~/bin"
    )]
    Install {
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    #[command(about = "Print the agent skill: how an agent draws with klad (the same text as docs/skill/SKILL.md)")]
    Skill,
    #[command(about = "Measure stroke storage, brush speed, replay, cache and compositing")]
    Bench {
        #[arg(long, default_value_t = 10_000)]
        strokes: usize,
        #[arg(long, default_value_t = 4096)]
        size: u32,
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
    #[cfg(feature = "editor")]
    #[command(about = "Open the editor, on a bundle or on the start window (new, open, recent)")]
    Open { bundle: Option<PathBuf> },
}

fn output(flags: &EditFlags, frame: u32) -> Output {
    Output {
        json: flags.json,
        render: flags.render || flags.overlay.is_some(),
        scale: flags.scale,
        frame,
        overlay: flags.overlay.clone(),
    }
}

fn mark_transform(
    offset: Option<[f32; 2]>,
    rotate: Option<f32>,
    (scale, sx, sy): (Option<f32>, Option<f32>, Option<f32>),
    pivot: Option<[f32; 2]>,
    flip: Option<&str>,
) -> Result<Option<MarkTransform>> {
    if offset.is_none() && rotate.is_none() && scale.is_none() && sx.is_none() && sy.is_none() && flip.is_none() {
        return Ok(None);
    }
    let [x, y] = offset.unwrap_or_default();
    let k = scale.unwrap_or(1.0);
    let (fx, fy) = match flip {
        Some("h") => (-1.0, 1.0),
        Some("v") => (1.0, -1.0),
        _ => (1.0, 1.0),
    };
    let transform = MarkTransform {
        x,
        y,
        rotate: rotate.unwrap_or(0.0),
        sx: sx.unwrap_or(1.0) * k * fx,
        sy: sy.unwrap_or(1.0) * k * fy,
        pivot,
    };
    if transform.sx == 0.0 || transform.sy == 0.0 {
        return Err(anyhow!("scale cannot be 0"));
    }
    Ok(Some(transform))
}

fn parse_numbers(text: &str) -> Result<Vec<f32>> {
    text.split(',')
        .map(|v| v.trim().parse::<f32>().map_err(|e| anyhow!("{v:?}: {e}")))
        .collect()
}

fn parse_range(text: &str) -> Result<[f32; 2]> {
    let (a, b) = text
        .split_once('-')
        .ok_or_else(|| anyhow!("{text:?} is not MIN-MAX, for example 12-30"))?;
    Ok([a.trim().parse()?, b.trim().parse()?])
}

fn polygon_arg(polygon: Option<String>) -> Result<Option<Vec<[f32; 2]>>> {
    polygon
        .as_deref()
        .map(|p| p.split_whitespace().map(parse_pair).collect::<Result<Vec<_>>>())
        .transpose()
}

fn layer_id(bundle: &std::path::Path, name: &str) -> Result<String> {
    let opened = klad_core::bundle::Bundle::open(bundle)?;
    Ok(opened.doc.find_layer(name).map_or(name.to_string(), |l| l.id.clone()))
}

fn parse_pair(text: &str) -> Result<[f32; 2]> {
    parse_numbers(text)?
        .try_into()
        .map_err(|_| anyhow!("{text:?} is not x,y"))
}

fn parse_origin(text: &str) -> Result<Option<[f32; 2]>> {
    match text {
        "centre" | "center" => Ok(None),
        _ => Ok(Some(parse_pair(text)?)),
    }
}

fn parse_clip_fps(text: &str) -> Result<Option<u32>> {
    match text {
        "doc" => Ok(None),
        _ => text
            .parse::<u32>()
            .ok()
            .filter(|f| (1..=240).contains(f))
            .map(Some)
            .ok_or_else(|| anyhow!("--fps takes 1 to 240, or doc for the document's fps")),
    }
}

fn parse_points(text: &str) -> Result<Vec<[f32; 3]>> {
    text.split_whitespace()
        .map(|triple| {
            let v = parse_numbers(triple)?;
            match v[..] {
                [x, y] => Ok([x, y, 1.0]),
                [x, y, p] => Ok([x, y, p]),
                _ => Err(anyhow!("{triple:?} is not x,y or x,y,pressure")),
            }
        })
        .collect()
}

const CHANGED_EXIT: i32 = 75;

fn main() {
    let traced = std::env::var_os("KLAD_BACKTRACE").is_some_and(|v| v != "0");
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    match traced {
        true => unsafe { std::env::set_var("RUST_BACKTRACE", "1") },
        false => std::panic::set_hook(Box::new(|info| eprintln!("error: {}", bug(info)))),
    }
    if let Err(e) = run() {
        match traced {
            true => eprintln!("error: {e:?}"),
            false => eprintln!("error: {e:#}"),
        }
        let changed = e.downcast_ref::<klad_core::bundle::Changed>().is_some();
        std::process::exit(if changed { CHANGED_EXIT } else { 1 });
    }
}

fn bug(info: &std::panic::PanicHookInfo) -> String {
    let what = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_default();
    let place = info
        .location()
        .map(|l| format!(" at {}:{}", l.file(), l.line()))
        .unwrap_or_default();
    format!(
        "klad hit a bug: {}{place}. KLAD_BACKTRACE=1 shows the details; please report it at https://github.com/klad-app/klad/issues",
        what.replace('\n', " ")
    )
}

fn run() -> Result<()> {
    match Cli::parse().command {
        Command::New {
            bundle,
            width,
            height,
            layers,
            background,
            fps,
            frames,
            sketch,
        } => {
            edit::new(
                &bundle,
                &Blank {
                    width,
                    height,
                    layers,
                    background,
                    fps,
                    frames,
                    sketch,
                },
            )?;
            println!("{}", bundle.display());
        }
        Command::Frames { bundle, from, to, json } => frames::sheet(&bundle, from, to, json)?,
        Command::Frame {
            bundle,
            action,
            at,
            count,
            layers,
            flags,
        } => {
            let spec = FrameSpec {
                insert: action == "insert",
                at,
                count,
                layers: (!layers.is_empty()).then_some(layers),
            };
            agent::edit(&bundle, &[Op::Frames(spec)], &output(&flags, at))?;
        }
        Command::Arcs {
            bundle,
            track,
            clip,
            from,
            to,
            ground,
            travel,
            slack,
            json,
            check,
        } => {
            let opened = Bundle::open(&bundle)?;
            let span = match clip {
                Some(name) => {
                    let c = opened
                        .doc
                        .timeline
                        .clip(&name)
                        .ok_or_else(|| anyhow!("no clip {name}"))?;
                    (c.from, c.to)
                }
                None => (
                    from.unwrap_or(0),
                    to.unwrap_or(opened.doc.timeline.frames.saturating_sub(1)),
                ),
            };
            let tracks = track
                .iter()
                .map(|t| arcs::Track::parse(t))
                .collect::<Result<Vec<_>>>()?;
            let checks = arcs::Checks {
                ground,
                travel,
                slack,
                json,
                check,
            };
            arcs::report(&opened, &tracks, span, &checks)?;
        }
        Command::Part {
            bundle,
            ids,
            name,
            flags,
        } => {
            agent::edit(&bundle, &[Op::Part { ids, name }], &output(&flags, 0))?;
        }
        Command::Duplicate { bundle, layer, flags } => {
            let layer = layer_id(&bundle, &layer)?;
            agent::edit(&bundle, &[Op::Duplicate { layer }], &output(&flags, 0))?;
        }
        Command::Cel {
            bundle,
            action,
            target,
            frame,
            hold,
            insert,
            flags,
        } => {
            let action = match action.as_str() {
                "new" => CelAction::New { layer: target },
                "dup" => CelAction::Dup { cel: target },
                "link" => CelAction::Link { cel: target },
                "move" => CelAction::Move { cel: target },
                _ => CelAction::Unlink { cel: target },
            };
            let spec = CelSpec {
                action,
                id: None,
                frame,
                hold,
                insert,
            };
            agent::edit(&bundle, &[Op::Cel(spec)], &output(&flags, frame.unwrap_or(0)))?;
        }
        Command::Copy {
            bundle,
            ids,
            to,
            frame,
            layer,
            offset,
            rotate,
            scale,
            sx,
            sy,
            pivot,
            flip,
            cut,
            flags,
        } => {
            if ids.is_empty() {
                return Err(anyhow!("give the strokes and fills to copy (<cel>:s<id>)"));
            }
            let target = match (to, frame) {
                (Some(cel), None) => CopyTarget::Cel(cel),
                (None, Some(frame)) => CopyTarget::Frame { frame, layer },
                _ => return Err(anyhow!("say where the copies go: --to <cel> or --frame <n>")),
            };
            let cut = cut
                .map(|text| text.split_whitespace().map(parse_pair).collect::<Result<Vec<_>>>())
                .transpose()?;
            let shown = match &target {
                CopyTarget::Frame { frame, .. } => *frame,
                CopyTarget::Cel(_) => 0,
            };
            let spec = CopySpec {
                ids,
                to: target,
                transform: mark_transform(offset, rotate, (scale, sx, sy), pivot, flip.as_deref())?,
                cut,
            };
            agent::edit(&bundle, &[Op::Copy(spec)], &output(&flags.edit(), shown))?;
        }
        Command::Inbetween {
            bundle,
            layers,
            from,
            to,
            count,
            spacing,
            matching,
            pair,
            redo,
            force,
            looping,
            keep_drawn,
            unpaired,
            at,
            only,
            keep_cels,
            force_cels,
            flags,
        } => {
            let pairs = pair.iter().map(|p| parse_mark_pair(p)).collect::<Result<Vec<_>>>()?;
            let unpaired = match (unpaired.as_str(), at) {
                ("pop", Some(at)) => Unpaired::parse(&format!("pop {at}"))?,
                (_, Some(_)) => return Err(anyhow!("--at goes with --unpaired pop")),
                (mode, None) => Unpaired::parse(mode)?,
            };
            let spec = InbetweenSpec {
                layers,
                from,
                to,
                count,
                spacing: Spacing::parse(&spacing)?,
                matching: Match::parse(&matching)?,
                pairs,
                redo,
                force,
                looping,
                keep_drawn,
                unpaired,
                only,
                keep_cels,
                force_cels,
                resolved: None,
            };
            if !redo && (spec.layers.is_empty() || from.is_none() || (to.is_none() && !looping)) {
                return Err(anyhow!(
                    "give --layers, --from and --to (the two keys; with --loop --to may be left out), or --redo"
                ));
            }
            agent::edit(&bundle, &[Op::Inbetween(spec)], &output(&flags, from.unwrap_or(0)))?;
        }
        Command::Retime {
            bundle,
            range: (from, to),
            on,
            fit,
            reverse,
            pingpong,
            layers,
            flags,
        } => {
            let action = match (on, fit, reverse, pingpong) {
                (Some(n), None, false, false) => Retime::On(n),
                (None, Some(n), false, false) => Retime::Fit(n),
                (None, None, true, false) => Retime::Reverse,
                (None, None, false, true) => Retime::Pingpong,
                _ => return Err(anyhow!("say how, with one of --on n, --fit n, --reverse or --pingpong")),
            };
            let spec = RetimeSpec {
                from,
                to,
                action,
                layers: (!layers.is_empty()).then_some(layers),
            };
            agent::edit(&bundle, &[Op::Retime(spec)], &output(&flags, from))?;
        }
        Command::Timeline {
            bundle,
            fps,
            frames,
            origin,
            flags,
        } => {
            let origin = origin.as_deref().map(parse_origin).transpose()?;
            if fps.is_none() && frames.is_none() && origin.is_none() {
                frames::show_timeline(&Bundle::open(&bundle)?);
            } else {
                agent::edit(&bundle, &[Op::Timeline { fps, frames, origin }], &output(&flags, 0))?;
            }
        }
        Command::Clip {
            bundle,
            add,
            set,
            rm,
            name,
            from,
            to,
            looping,
            no_loop,
            fps,
            flags,
        } => {
            let fps = fps.as_deref().map(parse_clip_fps).transpose()?;
            let looped = (looping || no_loop).then_some(looping);
            let edit = match (add, set, rm) {
                (Some(add), None, None) => {
                    let (Some(from), Some(to)) = (from, to) else {
                        return Err(anyhow!("--add needs --from and --to"));
                    };
                    Some(ClipEdit::Add(klad_core::doc::Clip {
                        fps: fps.flatten(),
                        ..klad_core::doc::Clip::new(&add, from, to, looping)
                    }))
                }
                (None, Some(set), None) => Some(ClipEdit::Set {
                    name: set,
                    rename: name,
                    from,
                    to,
                    looping: looped,
                    fps,
                }),
                (None, None, Some(rm)) => Some(ClipEdit::Remove { name: rm }),
                _ => None,
            };
            match edit {
                Some(edit) => agent::edit(&bundle, &[Op::Clip(edit)], &output(&flags, 0))?,
                None => frames::list_clips(&Bundle::open(&bundle)?, flags.json)?,
            }
        }
        Command::Ls { bundle, frame, json } => agent::ls(&bundle, frame, json)?,
        Command::Show { bundle, id } => agent::show(&bundle, &id)?,
        Command::Add {
            bundle,
            what,
            rest,
            place,
            flags,
        } => {
            let op = match (what.as_str(), rest.split_first()) {
                ("layer", Some((kind, props))) if kind == "image" => Op::AddLayer {
                    layer: looks::image_layer(&bundle, props)?,
                    place: place.place(),
                },
                ("image", _) => Op::AddNode {
                    node: looks::image_node(&bundle, &rest)?,
                    place: place.place(),
                },
                _ => agent::add_op(&what, &rest, place.place())?,
            };
            agent::edit(&bundle, &[op], &output(&flags, place.frame.unwrap_or(0)))?;
        }
        Command::Set {
            bundle,
            id,
            props,
            frame,
            flags,
        } => {
            let op = Op::Set {
                id,
                props: agent::parse_props(&props)?,
            };
            agent::edit(&bundle, &[op], &output(&flags, frame))?;
        }
        Command::Rm { bundle, ids, flags } => {
            if ids.is_empty() {
                return Err(anyhow!("give the ids to remove"));
            }
            agent::edit(&bundle, &[Op::Remove { ids }], &output(&flags, 0))?;
        }
        Command::Mv {
            bundle,
            id,
            place,
            flags,
        } => {
            let op = Op::Move {
                id,
                place: place.place(),
            };
            agent::edit(&bundle, &[op], &output(&flags, place.frame.unwrap_or(0)))?;
        }
        Command::Apply {
            bundle,
            file,
            frame,
            flags,
        } => agent::apply(&bundle, file, &output(&flags, frame))?,
        Command::Path {
            bundle,
            node,
            move_to,
            nudge,
            handle,
            insert,
            add,
            rm,
            corner,
            smooth,
            simplify,
            convert,
            flags,
        } => {
            let edits = paths::Edits {
                move_to,
                nudge,
                handle,
                insert,
                add,
                rm,
                corner,
                smooth,
                simplify,
                convert,
            };
            paths::run(&bundle, &node, &edits, flags.json, &output(&flags, 0))?;
        }
        Command::Transform {
            bundle,
            ids,
            offset,
            rotate,
            scale,
            sx,
            sy,
            pivot,
            flip,
            frame,
            flags,
        } => {
            if ids.is_empty() {
                return Err(anyhow!("give the strokes and fills to transform (<cel>:s<id>)"));
            }
            let transform = mark_transform(offset, rotate, (scale, sx, sy), pivot, flip.as_deref())?
                .ok_or_else(|| anyhow!("say how: --move, --rotate, --scale, --sx, --sy or --flip"))?;
            agent::edit(
                &bundle,
                &[Op::Transform { ids, transform }],
                &output(&flags.edit(), frame),
            )?;
        }
        Command::Lasso {
            bundle,
            layer,
            polygon,
            cut,
            offset,
            rotate,
            scale,
            sx,
            sy,
            pivot,
            flip,
            rm,
            frame,
            flags,
        } => {
            let polygon = polygon.split_whitespace().map(parse_pair).collect::<Result<Vec<_>>>()?;
            if polygon.len() < 3 {
                return Err(anyhow!("--polygon needs at least 3 x,y points"));
            }
            let transform = mark_transform(offset, rotate, (scale, sx, sy), pivot, flip.as_deref())?;
            let action = match (rm, transform) {
                (true, Some(_)) => return Err(anyhow!("--rm deletes; leave out the move, turn, scale and flip")),
                (true, None) => LassoAction::Remove,
                (false, Some(t)) => LassoAction::Transform(t),
                (false, None) => return marks::lassoed(&bundle, &layer, frame, &polygon, flags.json),
            };
            let op = Op::Lasso(LassoSpec {
                layer,
                frame,
                polygon,
                cut,
                action,
            });
            agent::edit(&bundle, &[op], &output(&flags.edit(), frame))?;
        }
        Command::Render {
            bundle,
            out,
            frame,
            scale,
            region,
            focus,
            overlay,
            depth,
            no_cache,
            references,
            onion,
            onion_by,
            onion_layers,
            no_tint,
            onion_loop,
            sheet,
            drawings,
            cell,
            columns,
            track,
        } => {
            let defaults = klad_render::onion::Onion::default();
            let (before, after) = onion.unwrap_or((defaults.before, defaults.after));
            let by = match onion_by.as_deref() {
                None | Some("drawings") => klad_render::onion::By::Drawings,
                Some("frames") => klad_render::onion::By::Frames,
                Some(other) => return Err(anyhow!("--onion-by takes drawings or frames, not {other:?}")),
            };
            let kinds = match (
                &overlay,
                onion.is_some() || onion_layers.is_some() || onion_loop.is_some(),
            ) {
                (Some(k), true) if !k.split(',').any(|k| k.trim() == "onion") => Some(format!("{k},onion")),
                (None, true) => Some("onion".to_string()),
                _ => overlay,
            };
            let kinds = match (kinds, track.is_empty()) {
                (Some(k), false) if !k.split(',').any(|k| k.trim() == "arcs") => Some(format!("{k},arcs")),
                (None, false) => Some("arcs".to_string()),
                (k, _) => k,
            };
            let look = Look {
                kinds,
                focus,
                depth,
                onion: klad_render::onion::Onion {
                    before,
                    after,
                    by,
                    tint: !no_tint,
                    wrap: onion_loop,
                    ..defaults
                },
                onion_layers,
                tracks: track,
                references,
            };
            match sheet {
                Some(spec) => {
                    if look.focus.is_some() || region.is_some() || scale.is_some() {
                        return Err(anyhow!(
                            "--sheet renders whole frames: use --cell for the size, not --focus, --region or --scale"
                        ));
                    }
                    let args = sheet::Sheet {
                        spec,
                        drawings,
                        cell,
                        columns,
                    };
                    sheet::render(&bundle, &args, &look, out)?
                }
                None => render(&bundle, out, frame, (scale, region), &look, !no_cache)?,
            }
        }
        Command::Stroke {
            bundle,
            layer,
            path,
            points,
            brush,
            color,
            size,
            opacity,
            flow,
            pressure,
            speed,
            seed,
            taper,
            frame,
            part,
            second,
            flags,
        } => {
            let taper = taper
                .as_deref()
                .map(|t| {
                    parse_numbers(t)?
                        .try_into()
                        .map_err(|_| anyhow!("--taper takes two lengths: in,out"))
                })
                .transpose()?;
            let spec = StrokeSpec {
                layer,
                frame,
                brush,
                color,
                size,
                opacity,
                flow,
                path,
                points: points.as_deref().map(parse_points).transpose()?,
                pressure: pressure.as_deref().map(parse_numbers).transpose()?.unwrap_or_default(),
                tilt: [0.0, 0.0],
                speed,
                seed,
                taper,
                part,
                second,
            };
            agent::edit(&bundle, &[Op::Stroke(spec)], &output(&flags, frame))?;
        }
        Command::Fill {
            bundle,
            layer,
            at,
            polygon,
            reference,
            tolerance,
            gap,
            expand,
            color,
            opacity,
            frame,
            like,
            frames,
            shape,
            part,
            flags,
        } => {
            if let Some(shape) = shape {
                let spec = FillSpec {
                    at: None,
                    shape: Some(shape),
                    expand,
                    color,
                    opacity,
                    frame,
                    part,
                    ..FillSpec::seeded(&layer_id(&bundle, &layer)?, [0.0, 0.0])
                };
                spec.check()?;
                return agent::edit(&bundle, &[Op::Fill(spec)], &output(&flags, frame));
            }
            if like.is_some() {
                let spec = FillSpec {
                    at: None,
                    like,
                    frames,
                    ..FillSpec::seeded(&layer, [0.0, 0.0])
                };
                spec.check()?;
                return agent::edit(&bundle, &[Op::Fill(spec)], &output(&flags, frame));
            }
            let polygon = polygon
                .as_deref()
                .map(|p| p.split_whitespace().map(parse_pair).collect::<Result<Vec<_>>>())
                .transpose()?;
            let spec = FillSpec {
                at,
                polygon,
                reference,
                tolerance,
                gap,
                expand,
                color,
                opacity,
                frame,
                part,
                ..FillSpec::seeded(&layer, [0.0, 0.0])
            };
            spec.check()?;
            agent::edit(&bundle, &[Op::Fill(spec)], &output(&flags, frame))?;
        }
        Command::Ink {
            bundle,
            layer,
            from,
            path,
            weight,
            light,
            break_deg,
            gap,
            wobble,
            taper,
            brush,
            color,
            seed,
            frame,
            part,
            hand,
            flags,
        } => {
            let spec = InkSpec {
                layer: layer_id(&bundle, &layer)?,
                from,
                path,
                frame,
                weight,
                light: light.unwrap_or([-1.0, -1.0]),
                break_deg,
                gap,
                wobble,
                taper,
                brush,
                color,
                opacity: 1.0,
                seed,
                part,
                hand,
            };
            agent::edit(&bundle, &[Op::Ink(spec)], &output(&flags, frame))?;
        }
        Command::Light {
            bundle,
            from,
            light,
            shade,
            rim,
            depth,
            soft,
            rim_width,
            shade_color,
            rim_color,
            shade_opacity,
            rim_opacity,
            frame,
            lamp,
            flags,
        } => {
            let spec = LightSpec {
                from,
                frame,
                light: light.unwrap_or([-1.0, -1.0]),
                lamp: lamp
                    .as_deref()
                    .map(|l| -> Result<[f32; 3]> {
                        parse_numbers(l)?
                            .try_into()
                            .map_err(|_| anyhow!("--lamp {l:?} is not x,y,height"))
                    })
                    .transpose()?,
                shade: shade.map(|l| layer_id(&bundle, &l)).transpose()?,
                rim: rim.map(|l| layer_id(&bundle, &l)).transpose()?,
                depth,
                soft,
                rim_width,
                shade_color,
                rim_color,
                shade_opacity,
                rim_opacity,
            };
            agent::edit(&bundle, &[Op::Light(spec)], &output(&flags, frame))?;
        }
        Command::Hatch {
            bundle,
            layer,
            within,
            polygon,
            angle,
            along,
            spacing,
            length,
            jitter,
            weight,
            brush,
            color,
            opacity,
            seed,
            frame,
            part,
            hand,
            flags,
        } => {
            let spec = HatchSpec {
                layer: layer_id(&bundle, &layer)?,
                frame,
                within,
                polygon: polygon_arg(polygon)?,
                angle,
                along,
                spacing,
                length,
                jitter,
                weight,
                brush,
                color,
                opacity,
                seed,
                part,
                hand,
            };
            agent::edit(&bundle, &[Op::Hatch(spec)], &output(&flags, frame))?;
        }
        Command::Paint {
            bundle,
            layer,
            within,
            polygon,
            brush,
            bold,
            color,
            form,
            axis,
            facing,
            light,
            elevation,
            lamp,
            key,
            fill,
            fill_strength,
            bounce,
            bounce_strength,
            core,
            shadow,
            shadow_mix,
            angle,
            weight,
            decisive,
            vary,
            hand,
            opacity,
            seed,
            frame,
            part,
            flags,
        } => {
            let triple = |text: &str, what: &str| -> Result<[f32; 3]> {
                parse_numbers(text)?
                    .try_into()
                    .map_err(|_| anyhow!("{what} {text:?} is not x,y,z"))
            };
            let spec = PaintSpec {
                layer: layer_id(&bundle, &layer)?,
                frame,
                within,
                polygon: polygon_arg(polygon)?,
                brush,
                bold,
                color,
                form,
                axis,
                facing: triple(&facing, "--facing")?,
                light,
                elevation,
                lamp: lamp.as_deref().map(|l| triple(l, "--lamp")).transpose()?,
                key,
                fill,
                fill_strength,
                bounce,
                bounce_strength,
                core,
                shadow,
                shadow_mix,
                angle,
                weight,
                decisive,
                vary,
                hand,
                opacity,
                seed,
                part,
            };
            agent::edit(&bundle, &[Op::Paint(spec)], &output(&flags, frame))?;
        }
        Command::Prepare { bundle, from, to } => {
            let bundle_ = Bundle::open(&bundle)?;
            let last = bundle_.doc.timeline.frames.saturating_sub(1);
            let (from, to) = (from.unwrap_or(0), to.unwrap_or(last).min(last));
            let brushes = BrushSet::for_bundle(&bundle_)?;
            let start = std::time::Instant::now();
            let done = layers::prepare(&bundle_, &brushes, from, to)?;
            println!(
                "frames {from} to {to}: {} drawings, {} replayed, {} already buffered, {} ms",
                done.replayed + done.updated + done.cached,
                done.replayed + done.updated,
                done.cached,
                start.elapsed().as_millis()
            );
        }
        Command::Scatter {
            bundle,
            layer,
            within,
            polygon,
            avoid,
            mark,
            count,
            spacing,
            size,
            turn,
            weight,
            brush,
            color,
            opacity,
            seed,
            frame,
            part,
            hand,
            flags,
        } => {
            let spec = ScatterSpec {
                layer: layer_id(&bundle, &layer)?,
                frame,
                within,
                polygon: polygon_arg(polygon)?,
                avoid,
                mark,
                count,
                spacing,
                size,
                turn,
                weight,
                brush,
                color,
                opacity,
                seed,
                part,
                hand,
            };
            agent::edit(&bundle, &[Op::Scatter(spec)], &output(&flags, frame))?;
        }
        Command::Foliage {
            bundle,
            layer,
            within,
            color,
            lit,
            shade,
            glow,
            light,
            lamp,
            leaf,
            density,
            mass,
            brush,
            edges_only,
            seed,
            frame,
            part,
            flags,
        } => {
            let lamp = lamp
                .as_deref()
                .map(|l| -> Result<[f32; 3]> {
                    parse_numbers(l)?
                        .try_into()
                        .map_err(|_| anyhow!("--lamp {l:?} is not x,y,height"))
                })
                .transpose()?;
            let spec = FoliageSpec {
                layer: layer_id(&bundle, &layer)?,
                frame,
                within,
                color,
                lit,
                shade,
                glow,
                light,
                lamp,
                leaf,
                mass,
                density,
                brush,
                body: !edges_only,
                seed,
                part,
            };
            agent::edit(&bundle, &[Op::Foliage(spec)], &output(&flags, frame))?;
        }
        Command::Bone {
            bundle,
            layer,
            add,
            rm,
            rename,
            to_name,
            moved,
            reparent,
            parent,
            head,
            tail,
            flags,
        } => {
            let spec = BoneSpec {
                layer: layer_id(&bundle, &layer)?,
                add,
                rm,
                rename,
                to_name,
                moved,
                reparent,
                parent,
                head,
                tail,
            };
            agent::edit(&bundle, &[Op::Bone(spec)], &output(&flags, 0))?;
        }
        Command::Bones { bundle, json } => rig::list(&Bundle::open(&bundle)?, json)?,
        Command::Weights {
            bundle,
            layer,
            auto,
            bone,
            set,
            part,
            value,
            smooth,
            frame,
            flags,
        } => {
            let set = set
                .as_deref()
                .map(|text| {
                    text.split_whitespace()
                        .map(|d| -> Result<[f32; 4]> {
                            parse_numbers(d)?
                                .try_into()
                                .map_err(|_| anyhow!("dab {d:?} is not x,y,radius,weight"))
                        })
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?
                .unwrap_or_default();
            if !auto && set.is_empty() && part.is_none() && !smooth {
                bail!("say what to do: --auto, --set with --bone, --part with --bone, or --smooth");
            }
            let spec = WeightsSpec {
                layer: layer_id(&bundle, &layer)?,
                frame,
                auto,
                bone,
                set,
                part,
                value,
                smooth,
                resolved: None,
            };
            agent::edit(&bundle, &[Op::Weights(spec)], &output(&flags, frame))?;
        }
        Command::Pose {
            bundle,
            layer,
            from,
            to,
            bones,
            root,
            hold,
            flags,
        } => {
            let bones = bones
                .iter()
                .map(|b| -> Result<(String, f32)> {
                    let (name, deg) = b
                        .split_once('=')
                        .ok_or_else(|| anyhow!("--bone {b:?} is not name=degrees"))?;
                    Ok((name.trim().to_string(), deg.trim().parse()?))
                })
                .collect::<Result<_>>()?;
            let spec = PoseSpec {
                layer: layer_id(&bundle, &layer)?,
                from,
                to,
                bones,
                root,
                hold,
                resolved: None,
            };
            agent::edit(&bundle, &[Op::Pose(spec)], &output(&flags, to))?;
        }
        Command::Compare {
            drawing,
            reference,
            frame,
            regions,
            heat,
            json,
        } => {
            let regions = regions
                .iter()
                .map(|r| compare::parse_region(r))
                .collect::<Result<Vec<_>>>()?;
            compare::compare(&drawing, &reference, frame, &regions, heat, json)?;
        }
        Command::Paintover {
            bundle,
            layer,
            from,
            within,
            sizes,
            threshold,
            length,
            curve,
            brush,
            focus,
            vary,
            like,
            frames,
            part,
            seed,
            frame,
            flags,
        } => {
            let length: [f32; 2] = parse_numbers(&length)?
                .try_into()
                .map_err(|_| anyhow!("--length {length:?} is not min,max"))?;
            let spec = PaintOverSpec {
                layer: layer_id(&bundle, &layer)?,
                frame,
                from,
                within,
                sizes,
                threshold,
                length,
                curve,
                brush,
                focus,
                vary,
                like,
                part,
                seed,
            };
            match frames {
                Some(range) => {
                    let (a, b) = range
                        .split_once('-')
                        .and_then(|(a, b)| Some((a.trim().parse().ok()?, b.trim().parse().ok()?)))
                        .ok_or_else(|| anyhow!("--frames {range:?} is not first-last"))?;
                    paintover::chain(&bundle, &spec, (a, b), &output(&flags, frame))?;
                }
                None => agent::edit(&bundle, &[Op::PaintOver(spec)], &output(&flags, frame))?,
            }
        }
        Command::Grass {
            bundle,
            layer,
            within,
            color,
            lit,
            shade,
            light,
            lamp,
            blade,
            width,
            far,
            wind,
            density,
            edge,
            patch,
            brush,
            kinds,
            seed,
            frame,
            part,
            flags,
        } => {
            let kinds = match kinds {
                Some(text) => {
                    serde_json::from_str(&text).map_err(|e| anyhow!("--kinds is not a list of species: {e}"))?
                }
                None => Vec::new(),
            };
            let lamp = lamp
                .as_deref()
                .map(|l| -> Result<[f32; 3]> {
                    parse_numbers(l)?
                        .try_into()
                        .map_err(|_| anyhow!("--lamp {l:?} is not x,y,height"))
                })
                .transpose()?;
            let spec = GrassSpec {
                layer: layer_id(&bundle, &layer)?,
                frame,
                within,
                color,
                lit,
                shade,
                light,
                lamp,
                blade,
                width,
                far,
                wind,
                density,
                edge,
                patch,
                brush,
                kinds,
                seed,
                part,
            };
            agent::edit(&bundle, &[Op::Grass(spec)], &output(&flags, frame))?;
        }
        Command::Gaps {
            bundle,
            layer,
            frame,
            sheet,
            reference,
            gap,
            tolerance,
            min,
            render,
            json,
        } => {
            let ask = colour::GapAsk {
                layer,
                frames: sheet.unwrap_or_else(|| {
                    let f = frame.unwrap_or(0);
                    format!("{f}-{f}")
                }),
                reference,
                gap,
                tolerance,
                min,
            };
            let out = Output {
                json,
                render,
                scale: None,
                frame: frame.unwrap_or(0),
                overlay: None,
            };
            colour::gaps(&bundle, &ask, &out)?;
        }
        Command::Refill {
            bundle,
            layer,
            frame,
            id,
            flags,
        } => fills::refill(&bundle, &layer, frame, id, &output(&flags, frame))?,
        Command::Palette {
            bundle,
            add,
            name,
            rm,
            flags,
        } => {
            let edit = match (add, rm) {
                (Some(color), _) => Some(PaletteEdit::Add {
                    color,
                    name: name.unwrap_or_default(),
                    index: None,
                }),
                (_, Some(key)) => Some(PaletteEdit::Remove { key }),
                _ => None,
            };
            match edit {
                Some(edit) => agent::edit(&bundle, &[Op::Palette(edit)], &output(&flags, 0))?,
                None => palette(&bundle, flags.json)?,
            }
        }
        Command::Export {
            bundle,
            png: _,
            layer,
            frame,
            scale,
            depth,
            background,
            references,
            sheet,
            frames,
            godot,
            store,
            padding,
            max,
            out,
        } => {
            let look = sprites::Look {
                layer: layer.clone(),
                scale,
                depth,
                background,
            };
            let packing = sprites::Packing { padding, max };
            match (sheet, frames, godot) {
                (Some(spec), _, _) => sprites::sheet(&bundle, &spec, &look, &packing, out)?,
                (_, Some(spec), _) => sprites::frames(&bundle, &spec, &look, out)?,
                (_, _, Some(dir)) => sprites::godot(&bundle, dir.or(out), store, &look, &packing)?,
                _ => export::run(
                    &bundle,
                    export::Export {
                        layer,
                        frame,
                        scale,
                        depth,
                        background,
                        references,
                        out,
                    },
                )?,
            }
        }
        Command::Brushes {
            bundle,
            show,
            json,
            set,
            picture,
        } => match (bundle.as_deref(), set, picture) {
            (Some(b), Some(set), _) => brushes::set(b, &set[0], Path::new(&set[1]))?,
            (Some(b), None, Some(picture)) => brushes::picture(b, &picture[0], Path::new(&picture[1]))?,
            (None, Some(_), _) | (None, None, Some(_)) => anyhow::bail!("--set and --picture need a bundle"),
            (bundle, None, None) => brushes::list(bundle, show.as_deref(), json)?,
        },
        Command::Font { bundle, font } => fonts::font(&bundle, font.as_deref())?,
        Command::Fonts {
            bundle,
            group,
            search,
            get,
            json,
        } => fonts::list(
            bundle.as_deref(),
            group.as_deref(),
            search.as_deref(),
            get.as_deref(),
            json,
        )?,
        Command::Atmosphere {
            bundle,
            props,
            off,
            flags,
        } => looks::atmosphere(&bundle, &props, off, &output(&flags, 0))?,
        Command::Effect {
            bundle,
            layer,
            rest,
            rm,
            clear,
            flags,
        } => {
            let change = match (rest.split_first(), rm, clear) {
                (_, _, true) => looks::Change::Clear,
                (_, Some(i), _) => looks::Change::Remove(i),
                (Some((kind, props)), None, false) => looks::Change::Add(kind.clone(), props.to_vec()),
                (None, None, false) => looks::Change::List,
            };
            looks::effect(&bundle, &layer, change, &output(&flags, 0))?;
        }
        Command::Reshape {
            bundle,
            stroke,
            path,
            flags,
        } => {
            let props: serde_json::Map<String, serde_json::Value> =
                [("path".to_string(), serde_json::json!(path))].into_iter().collect();
            agent::edit(&bundle, &[Op::Set { id: stroke, props }], &output(&flags, 0))?;
        }
        Command::Undo { bundle, flags } => history::walk(&bundle, Stack::Undo, &output(&flags, 0))?,
        Command::Redo { bundle, flags } => history::walk(&bundle, Stack::Redo, &output(&flags, 0))?,
        Command::Log { bundle, limit, json } => history::log(&bundle, limit, json)?,
        Command::Selection { bundle, json } => history::selection(&bundle, json)?,
        Command::Notes {
            bundle,
            done,
            all,
            json,
        } => history::notes(&bundle, done, all, json)?,
        Command::Diff {
            bundle,
            step,
            frame,
            scale,
        } => history::diff(&bundle, step, frame, scale)?,
        Command::Info { bundle } => info(&bundle)?,
        Command::Strokes {
            bundle,
            layer,
            frame,
            after,
            json,
        } => strokes(&bundle, &layer, frame, after, json)?,
        Command::Hash {
            bundle,
            frame,
            all,
            no_cache,
        } => match all {
            true => hash_all(&bundle, !no_cache)?,
            false => hash(&bundle, frame, !no_cache)?,
        },
        Command::Demo {
            bundle,
            size,
            seed,
            force,
        } => {
            if force && bundle.join("doc.json").exists() {
                std::fs::remove_dir_all(&bundle)?;
            }
            anyhow::ensure!(
                !bundle.join("doc.json").exists(),
                "{} exists, use --force",
                bundle.display()
            );
            demo::write(&bundle, size, seed)?;
            println!("{}", bundle.display());
        }
        Command::Stress {
            bundle,
            size,
            layers,
            strokes,
            seed,
            frames,
            every,
        } => {
            edit::stress(&bundle, size, layers, strokes, seed, (frames, every))?;
            println!("{}", bundle.display());
        }
        Command::Scribble {
            bundle,
            layer,
            frame,
            count,
            seed,
            size,
            color,
        } => edit::scribble(
            &bundle,
            edit::Scribble {
                layer,
                frame,
                count,
                seed,
                size,
                color,
            },
        )?,
        Command::Watch { bundle, poll, seconds } => edit::watch(&bundle, poll, seconds)?,
        Command::Licences => print!("{}\n{}", klad_core::notices::LICENSE, klad_core::notices::NOTICES),
        Command::Install { dir } => install::install(dir.as_deref())?,
        Command::Skill => print!("{}", include_str!("../../../docs/skill/SKILL.md")),
        Command::Bench { strokes, size, seed } => bench::run(strokes, size, seed)?,
        #[cfg(feature = "editor")]
        Command::Open { bundle } => klad_app::run(bundle)?,
    }
    Ok(())
}

fn parse_onion_counts(text: &str) -> Result<(u32, u32), String> {
    let (a, b) = text.split_once(',').ok_or("give before,after, for example 2,2")?;
    Ok((
        a.trim().parse().map_err(|e| format!("{e}"))?,
        b.trim().parse().map_err(|e| format!("{e}"))?,
    ))
}

fn parse_span(text: &str) -> Result<(u32, u32), String> {
    let (a, b) = text.split_once('-').ok_or("give from-to, for example 0-7")?;
    let span: (u32, u32) = (
        a.trim().parse().map_err(|e| format!("{e}"))?,
        b.trim().parse().map_err(|e| format!("{e}"))?,
    );
    match span.0 <= span.1 {
        true => Ok(span),
        false => Err("the loop must start before it ends".into()),
    }
}

fn parse_mark_pair(text: &str) -> Result<[u32; 2]> {
    let id = |v: &str| {
        v.trim()
            .trim_start_matches('s')
            .parse::<u32>()
            .map_err(|_| anyhow!("--pair takes s3=s7 (a mark of the first key = one of the second), not {text:?}"))
    };
    let (a, b) = text
        .split_once('=')
        .ok_or_else(|| anyhow!("--pair takes s3=s7, not {text:?}"))?;
    Ok([id(a)?, id(b)?])
}

fn parse_frames_span(text: &str) -> Result<(u32, u32), String> {
    let (a, b) = text.split_once('-').ok_or("give FROM-TO, for example 0-11")?;
    Ok((
        a.trim().parse().map_err(|e| format!("{e}"))?,
        b.trim().parse().map_err(|e| format!("{e}"))?,
    ))
}

fn parse_region(text: &str) -> Result<[i32; 4], String> {
    let parts: Vec<i32> = text
        .split(',')
        .map(|p| p.trim().parse::<i32>().map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    parts.try_into().map_err(|_| "region is x,y,w,h".to_string())
}

fn render(
    path: &Path,
    out: Option<PathBuf>,
    frame: u32,
    (scale, region): (Option<f32>, Option<[i32; 4]>),
    look: &Look,
    use_cache: bool,
) -> Result<()> {
    let start = Instant::now();
    let bundle = Bundle::open(path)?;
    let view = match &look.focus {
        Some(id) => agent::focus_view(&bundle, id, frame, scale)?,
        None => View {
            scale: scale.unwrap_or(1.0),
            region,
            references: false,
        },
    };
    let done = agent::render_look(&bundle, frame, view, look, out, use_cache)?;
    println!("{}", done.path.display());
    eprintln!(
        "{}x{}  hash {:016x}  {}  {:.0} ms",
        done.frame.width,
        done.frame.height,
        done.frame.hash(),
        done.note,
        start.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

fn hash_all(path: &Path, use_cache: bool) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let brushes = BrushSet::for_bundle(&bundle)?;
    (0..bundle.doc.timeline.frames).try_for_each(|frame| {
        let loaded = layers::load(&bundle, frame, &brushes, use_cache)?;
        let image = compose(&bundle.doc, frame, &loaded.rasters, View::scaled(1.0))?;
        println!("frame {frame:<4} {:016x}", image.hash());
        Ok(())
    })
}

fn hash(path: &Path, frame: u32, use_cache: bool) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let loaded = layers::load(&bundle, frame, &BrushSet::for_bundle(&bundle)?, use_cache)?;
    let image = compose(&bundle.doc, frame, &loaded.rasters, View::scaled(1.0))?;
    println!("frame  {:016x}", image.hash());
    bundle.doc.paint_content_at(frame).iter().for_each(|(layer, cel)| {
        if let Some(raster) = loaded.rasters.get(&cel.id) {
            println!("{:<6} {:016x}", layer.id, raster.hash());
        }
    });
    Ok(())
}

fn info(path: &Path) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let doc = &bundle.doc;
    println!(
        "{}x{}  rev {}  {}  {} frames at {} fps",
        doc.canvas.width, doc.canvas.height, doc.rev, doc.format, doc.timeline.frames, doc.timeline.fps
    );
    doc.all_layers()
        .iter()
        .filter(|l| l.kind() == Kind::Paint)
        .try_for_each(|layer| {
            layer.cels().iter().filter(|c| c.link.is_none()).try_for_each(|cel| {
                let marks = bundle.read_marks(&layer.id, &cel.id)?;
                let strokes: Vec<_> = strokes_of(&marks).collect();
                let fills = marks.len() - strokes.len();
                let bytes = std::fs::metadata(bundle.strokes_path(&layer.id, &cel.id)).map_or(0, |m| m.len());
                let samples: usize = strokes.iter().map(|s| s.samples.len()).sum();
                println!(
                    "{:<7} {:<7} {:<14} {} strokes, {fills} fills, {} samples, {} B on disk ({:.0} B per mark)",
                    layer.id,
                    cel.id,
                    layer.name,
                    strokes.len(),
                    samples,
                    bytes,
                    bytes as f64 / marks.len().max(1) as f64
                );
                Ok::<_, anyhow::Error>(())
            })
        })
}

fn strokes(path: &Path, layer: &str, frame: u32, after: u32, as_json: bool) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let owner = bundle.doc.layer(layer).ok_or_else(|| anyhow!("no layer {layer}"))?;
    let cel = owner
        .content_at(frame)
        .ok_or_else(|| anyhow!("layer {layer} has no cel at frame {frame}"))?;
    let marks: Vec<_> = bundle
        .read_marks(layer, &cel.id)?
        .into_iter()
        .filter(|m| m.id() > after)
        .collect();
    let strokes: Vec<_> = strokes_of(&marks).collect();
    let fills: Vec<_> = fills_of(&marks).collect();
    if as_json {
        let list: Vec<serde_json::Value> = strokes.iter().map(|s| stroke_json(&cel.id, s, true)).collect();
        let fills: Vec<serde_json::Value> = fills.iter().map(|f| fill_json(&cel.id, f)).collect();
        println!(
            "{}",
            serde_json::json!({ "cel": cel.id, "strokes": list, "fills": fills, "order": marks.iter().map(|m| m.id()).collect::<Vec<_>>(), "parts": cel.parts })
        );
        return Ok(());
    }
    println!(
        "cel {}: address a stroke or fill as {}:s<id>, for example {}:s{}",
        cel.id,
        cel.id,
        cel.id,
        marks.first().map_or(1, |m| m.id())
    );
    if !strokes.is_empty() {
        println!("id     author brush    size  samples    ms    Hz  pressure     tilt  bounds");
    }
    strokes.iter().for_each(|s| {
        let part = cel.part_of(s.id).map_or(String::new(), |p| format!("  part {p}"));
        let ms: u32 = s.samples.iter().map(|v| v.dt_ms as u32).sum();
        let hz = s.samples.len().saturating_sub(1) as f64 / (ms.max(1) as f64 / 1000.0);
        let (lo, hi) = s.samples.iter().fold((f32::MAX, 0.0f32), |(lo, hi), v| {
            (lo.min(v.pressure()), hi.max(v.pressure()))
        });
        let tilt = s
            .samples
            .iter()
            .map(|v| v.tilt())
            .fold(0.0f32, |m, (x, y)| m.max(x.abs()).max(y.abs()));
        let [x0, y0, x1, y1] = s.bounds().unwrap_or_default();
        println!(
            "{:<6} {:<6} {:<7} {:>5.1} {:>8} {:>5} {:>5.0}  {lo:.2}-{hi:.2}  {tilt:>7.2}  {x0:.0},{y0:.0} {:.0}x{:.0}{}{part}",
            format!("s{}", s.id),
            s.author.name(),
            s.brush,
            s.size,
            s.samples.len(),
            ms,
            hz,
            x1 - x0,
            y1 - y0,
            marks::flags(s.lock, &s.cuts)
        );
    });
    if !fills.is_empty() {
        println!("fills: id     author colour     source                              pixels  bounds");
    }
    fills.iter().for_each(|f| {
        let part = cel.part_of(f.id).map_or(String::new(), |p| format!("  part {p}"));
        let source = match &f.source {
            FillSource::Seed { seed, reference } => format!(
                "at {:.0},{:.0} ref {} gap {} tol {}",
                seed.x,
                seed.y,
                reference.name(),
                seed.gap,
                seed.tolerance
            ),
            FillSource::Polygon(points) => format!("polygon of {} points", points.len()),
        };
        let [x0, y0, x1, y1] = f.bounds().unwrap_or_default();
        println!(
            "       {:<6} {:<6} {} {source:<35} {:>7}  {x0:.0},{y0:.0} {:.0}x{:.0}{}{part}",
            format!("s{}", f.id),
            f.author.name(),
            hex8(f.color()),
            f.coverage.pixels(),
            x1 - x0,
            y1 - y0,
            marks::flags(f.lock, &f.cuts)
        );
    });
    let samples: usize = strokes.iter().map(|s| s.samples.len()).sum();
    let packed = klad_core::stroke::encode_cel(&marks)?.len();
    println!(
        "{} strokes, {} fills, {samples} samples, {packed} B packed together ({:.0} B per mark)",
        strokes.len(),
        fills.len(),
        packed as f64 / marks.len().max(1) as f64,
    );
    Ok(())
}

fn palette(path: &Path, as_json: bool) -> Result<()> {
    let bundle = Bundle::open(path)?;
    let palette = &bundle.doc.palette;
    if as_json {
        println!("{}", serde_json::to_string_pretty(palette)?);
        return Ok(());
    }
    if palette.is_empty() {
        println!("the palette is empty: klad palette <bundle> --add '#rrggbb' --name <name>");
    }
    palette
        .iter()
        .enumerate()
        .for_each(|(i, s)| println!("{i:<3} {:<10} {}", s.color, s.name));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn every_command_parses_its_flags() {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(flags_parse)
            .unwrap()
            .join()
            .unwrap();
    }

    fn flags_parse() {
        Cli::command().debug_assert();
        let lasso = Cli::try_parse_from([
            "draw",
            "lasso",
            "a.draw",
            "l1",
            "--polygon",
            "0,0 9,0 9,9",
            "--cut",
            "--scale",
            "2",
            "--render",
        ])
        .unwrap();
        assert!(matches!(
            lasso.command,
            Command::Lasso {
                scale: Some(2.0),
                cut: true,
                ..
            }
        ));
    }
}
