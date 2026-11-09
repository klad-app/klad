use klad_brush::{BrushSet, CelReplay};
use klad_core::bundle::Bundle;
use klad_core::markedit::{self, MarkChange, MarkTransform};
use std::time::Instant;

#[test]
#[ignore]
fn edit_time() {
    let Ok(path) = std::env::var("KLAD_EDIT_BUNDLE") else {
        eprintln!("set KLAD_EDIT_BUNDLE, KLAD_EDIT_LAYER and KLAD_EDIT_MARK (an address like c8:s120)");
        return;
    };
    let layer = std::env::var("KLAD_EDIT_LAYER").unwrap();
    let mark = std::env::var("KLAD_EDIT_MARK").unwrap();
    let (cel, id) = markedit::parse_address(&mark).unwrap();
    let bundle = Bundle::open(&path).unwrap();
    let brushes = BrushSet::for_bundle(&bundle).unwrap();
    let marks = bundle.read_marks(&layer, cel).unwrap();
    let (w, h) = (bundle.doc.canvas.width, bundle.doc.canvas.height);
    let start = Instant::now();
    let (mut replay, mut raster, _) = CelReplay::replay(w, h, marks.clone(), &brushes).unwrap();
    println!("replay in order: {:.0} ms for {} marks", start.elapsed().as_secs_f64() * 1000.0, marks.len());
    let mut current = marks;
    let times: Vec<(f64, usize, bool)> = [6.0f32, -6.0, 6.0, -6.0, 6.0]
        .iter()
        .map(|dx| {
            let mut change = MarkChange::Transform {
                ids: vec![id],
                transform: MarkTransform {
                    x: *dx,
                    ..Default::default()
                },
            };
            markedit::apply(&mut current, &mut change, cel, bundle.doc.mark_ids(cel)).unwrap();
            let start = Instant::now();
            let updated = replay.update(&mut raster, current.clone(), &brushes).unwrap();
            (start.elapsed().as_secs_f64() * 1000.0, updated.replayed, updated.full)
        })
        .collect();
    let mut ms: Vec<f64> = times.iter().map(|t| t.0).collect();
    ms.sort_by(f64::total_cmp);
    println!(
        "move {mark} by 6 px: median {:.1} ms, {} marks replayed, full rebuild {}",
        ms[ms.len() / 2],
        times[0].1,
        times[0].2
    );
}
