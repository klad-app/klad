use egui::Pos2;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Down,
    Move,
    Up,
}

#[derive(Clone, Copy, Debug)]
pub struct PenEvent {
    pub phase: Phase,
    pub pos: Pos2,
    pub pressure: f32,
    pub tilt: (f32, f32),
    pub eraser: bool,
    pub tablet: bool,
    pub device: u64,
    pub at: Instant,
}

impl PenEvent {
    pub fn recorded_pressure(&self) -> f32 {
        if !self.tablet && self.pressure <= 0.0 {
            1.0
        } else {
            self.pressure
        }
    }
}

pub fn device_key(device: u64) -> String {
    if device == 0 {
        "tablet".into()
    } else {
        format!("pen-{device:016x}")
    }
}

pub fn from_egui(events: &[egui::Event]) -> Vec<PenEvent> {
    let at = Instant::now();
    let event = |phase, pos, pressure| PenEvent {
        phase,
        pos,
        pressure,
        tilt: (0.0, 0.0),
        eraser: false,
        tablet: false,
        device: 0,
        at,
    };
    events
        .iter()
        .filter_map(|e| match e {
            egui::Event::PointerMoved(pos) => Some(event(Phase::Move, *pos, 1.0)),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                ..
            } => Some(if *pressed {
                event(Phase::Down, *pos, 1.0)
            } else {
                event(Phase::Up, *pos, 0.0)
            }),
            _ => None,
        })
        .collect()
}

#[cfg(target_os = "macos")]
pub use mac::Pen;

#[cfg(not(target_os = "macos"))]
pub struct Pen;

#[cfg(not(target_os = "macos"))]
impl Pen {
    pub fn install() -> Option<Self> {
        None
    }

    pub fn drain(&self, _content_height: f32) -> Vec<PenEvent> {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::{PenEvent, Phase};
    use block2::RcBlock;
    use klad_core::rng::{FNV_OFFSET, fnv};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventSubtype, NSEventType, NSPointingDeviceType};
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    #[derive(Clone, Copy)]
    struct Raw {
        phase: Phase,
        x: f32,
        y_from_bottom: f32,
        pressure: f32,
        tilt: (f32, f32),
        eraser: bool,
        tablet: bool,
        device: u64,
        at: Instant,
    }

    #[derive(Default)]
    struct Proximity {
        eraser: AtomicBool,
        device: AtomicU64,
    }

    pub struct Pen {
        queue: Arc<Mutex<Vec<Raw>>>,
        _monitor: Retained<AnyObject>,
    }

    fn phase_of(kind: NSEventType) -> Option<Phase> {
        [
            (NSEventType::LeftMouseDown, Phase::Down),
            (NSEventType::LeftMouseDragged, Phase::Move),
            (NSEventType::LeftMouseUp, Phase::Up),
            (NSEventType::MouseMoved, Phase::Move),
        ]
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, p)| *p)
    }

    fn identify(event: &NSEvent) -> u64 {
        let parts = [
            event.vendorID() as u64,
            event.uniqueID(),
            event.pointingDeviceSerialNumber() as u64,
        ];
        if parts.iter().all(|p| *p == 0) {
            return 0;
        }
        fnv(FNV_OFFSET, &parts.map(u64::to_le_bytes).concat())
    }

    fn near(event: &NSEvent, proximity: &Proximity) {
        let entering = event.isEnteringProximity();
        proximity.eraser.store(
            entering && event.pointingDeviceType() == NSPointingDeviceType::Eraser,
            Ordering::Relaxed,
        );
        if entering {
            proximity.device.store(identify(event), Ordering::Relaxed);
        }
    }

    fn record(event: &NSEvent, proximity: &Proximity, queue: &Mutex<Vec<Raw>>) {
        let kind = event.r#type();
        if kind == NSEventType::TabletProximity {
            near(event, proximity);
            return;
        }
        let Some(phase) = phase_of(kind) else {
            return;
        };
        let subtype = event.subtype();
        if subtype == NSEventSubtype::TabletProximity {
            near(event, proximity);
            return;
        }
        let tablet = subtype == NSEventSubtype::TabletPoint;
        let tilt = if tablet {
            let t = event.tilt();
            (t.x as f32, t.y as f32)
        } else {
            (0.0, 0.0)
        };
        let location = event.locationInWindow();
        let raw = Raw {
            phase,
            x: location.x as f32,
            y_from_bottom: location.y as f32,
            pressure: event.pressure(),
            tilt,
            eraser: tablet && proximity.eraser.load(Ordering::Relaxed),
            tablet,
            device: if tablet {
                proximity.device.load(Ordering::Relaxed)
            } else {
                0
            },
            at: Instant::now(),
        };
        if let Ok(mut q) = queue.lock() {
            q.push(raw);
        }
    }

    impl Pen {
        pub fn install() -> Option<Self> {
            let queue = Arc::new(Mutex::new(Vec::new()));
            let proximity = Arc::new(Proximity::default());
            let (q, e) = (queue.clone(), proximity.clone());
            NSEvent::setMouseCoalescingEnabled(false);
            let mask = NSEventMask::LeftMouseDown
                | NSEventMask::LeftMouseUp
                | NSEventMask::LeftMouseDragged
                | NSEventMask::MouseMoved
                | NSEventMask::TabletProximity;
            let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
                record(unsafe { event.as_ref() }, &e, &q);
                event.as_ptr()
            });
            let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block) }?;
            Some(Self {
                queue,
                _monitor: monitor,
            })
        }

        pub fn drain(&self, content_height: f32) -> Vec<PenEvent> {
            let raw = self
                .queue
                .lock()
                .map(|mut q| std::mem::take(&mut *q))
                .unwrap_or_default();
            raw.into_iter()
                .map(|r| PenEvent {
                    phase: r.phase,
                    pos: egui::pos2(r.x, content_height - r.y_from_bottom),
                    pressure: r.pressure,
                    tilt: r.tilt,
                    eraser: r.eraser,
                    tablet: r.tablet,
                    device: r.device,
                    at: r.at,
                })
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egui_pointer_events_become_pen_events() {
        let events = [
            egui::Event::PointerButton {
                pos: egui::pos2(1.0, 2.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerMoved(egui::pos2(3.0, 4.0)),
            egui::Event::PointerButton {
                pos: egui::pos2(3.0, 4.0),
                button: egui::PointerButton::Secondary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ];
        let pen = from_egui(&events);
        assert_eq!(pen.len(), 2);
        assert_eq!(pen[0].phase, Phase::Down);
        assert_eq!(pen[1].pos, egui::pos2(3.0, 4.0));
    }

    #[test]
    fn mouse_drags_without_pressure_record_full_pressure() {
        let mut e = from_egui(&[egui::Event::PointerMoved(egui::pos2(3.0, 4.0))])[0];
        e.pressure = 0.0;
        assert_eq!(e.recorded_pressure(), 1.0);
        e.tablet = true;
        assert_eq!(e.recorded_pressure(), 0.0);
        e.pressure = 0.42;
        assert_eq!(e.recorded_pressure(), 0.42);
        assert_eq!(device_key(0), "tablet");
        assert_eq!(device_key(0xab), "pen-00000000000000ab");
    }
}
