use crate::commands::Cmd;
#[cfg(any(target_os = "macos", test))]
use crate::commands::Item;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum Picked {
    Run(Cmd),
    Open(PathBuf),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    pub off: BTreeSet<Cmd>,
    pub on: BTreeSet<Cmd>,
}

thread_local! {
    static QUEUE: RefCell<Vec<Picked>> = const { RefCell::new(Vec::new()) };
    static SHOWN: RefCell<Shown> = const {
        RefCell::new(Shown {
            off: BTreeSet::new(),
            on: BTreeSet::new(),
        })
    };
}

static NATIVE: AtomicBool = AtomicBool::new(false);

pub const RECENT: usize = 10;
pub const OUTSIDE_A_DRAWING: [Cmd; 4] = [Cmd::NewDocument, Cmd::OpenDocument, Cmd::OpenRecent, Cmd::Quit];

pub fn native() -> bool {
    NATIVE.load(Ordering::Relaxed)
}

#[cfg(any(target_os = "macos", test))]
pub fn push(picked: Picked) {
    QUEUE.with(|q| q.borrow_mut().push(picked));
}

pub fn take() -> Vec<Picked> {
    QUEUE.with(|q| std::mem::take(&mut *q.borrow_mut()))
}

pub fn show(shown: Shown) {
    SHOWN.with(|s| *s.borrow_mut() = shown);
}

#[cfg(any(target_os = "macos", test))]
fn shown() -> Shown {
    SHOWN.with(|s| s.borrow().clone())
}

pub fn outside_a_drawing() -> Shown {
    Shown {
        off: Cmd::ALL
            .into_iter()
            .filter(|c| !OUTSIDE_A_DRAWING.contains(c))
            .collect(),
        on: BTreeSet::new(),
    }
}

pub fn recent_label(path: &Path) -> String {
    let name = path
        .file_stem()
        .map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().to_string());
    match path.parent().and_then(Path::file_name) {
        Some(folder) => format!("{name}  ({})", folder.to_string_lossy()),
        None => name,
    }
}

#[cfg(any(target_os = "macos", test))]
pub fn tidy(items: &[Item], keep: impl Fn(&Item) -> bool) -> Vec<Item> {
    let kept: Vec<Item> = items.iter().copied().filter(|i| keep(i)).collect();
    kept.iter()
        .enumerate()
        .filter(|(i, item)| **item != Item::Line || (*i > 0 && *i + 1 < kept.len() && kept[i - 1] != Item::Line))
        .map(|(_, item)| *item)
        .collect()
}

pub fn signature(keys: &std::collections::BTreeMap<String, Vec<String>>, recent: &[String]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    keys.hash(&mut hasher);
    recent.iter().take(RECENT).for_each(|r| r.hash(&mut hasher));
    hasher.finish()
}

#[cfg(target_os = "macos")]
pub use mac::install;

#[cfg(not(target_os = "macos"))]
pub fn install(_ctx: &egui::Context, _bindings: &crate::commands::Bindings, _recent: &[String]) {}

#[cfg(target_os = "macos")]
mod mac {
    use super::{Picked, RECENT, push, recent_label, shown, tidy};
    use crate::commands::{Bindings, Cmd, IN_APP_MENU, Item, Keys, MENUS, menu_label};
    use egui::Key;
    use objc2::rc::Retained;
    use objc2::runtime::{ProtocolObject, Sel};
    use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
    use objc2_app_kit::{
        NSApplication, NSControlStateValueOff, NSControlStateValueOn, NSEvent, NSEventModifierFlags, NSMenu,
        NSMenuDelegate, NSMenuItem,
    };
    use objc2_foundation::{NSObject, NSObjectProtocol, NSString};
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::path::PathBuf;
    use std::sync::OnceLock;
    use std::sync::atomic::Ordering;

    static CTX: OnceLock<egui::Context> = OnceLock::new();
    const RECENT_TAG: isize = 100_000;

    thread_local! {
        static TARGET: RefCell<Option<Retained<Target>>> = const { RefCell::new(None) };
        static PATHS: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "KladMenuTarget"]
        struct Target;

        impl Target {
            #[unsafe(method(kladRun:))]
            fn run(&self, item: &NSMenuItem) {
                let tag = item.tag();
                let picked = match tag {
                    t if t >= RECENT_TAG => PATHS.with(|p| p.borrow().get((t - RECENT_TAG) as usize).cloned().map(Picked::Open)),
                    t => Cmd::ALL.get(t as usize).copied().map(Picked::Run),
                };
                if let Some(picked) = picked {
                    push(picked);
                }
                if let Some(ctx) = CTX.get() {
                    ctx.request_repaint();
                }
            }

            #[unsafe(method(validateMenuItem:))]
            fn validate(&self, item: &NSMenuItem) -> bool {
                let cmd = usize::try_from(item.tag()).ok().and_then(|t| Cmd::ALL.get(t));
                let now = shown();
                if let Some(cmd) = cmd {
                    item.setState(match now.on.contains(cmd) {
                        true => NSControlStateValueOn,
                        false => NSControlStateValueOff,
                    });
                }
                cmd.is_none_or(|c| !now.off.contains(c))
            }

            #[unsafe(method(menuHasKeyEquivalent:forEvent:target:action:))]
            fn has_key(&self, _menu: &NSMenu, _event: &NSEvent, _target: *mut c_void, _action: *mut c_void) -> bool {
                false
            }
        }

        unsafe impl NSObjectProtocol for Target {}

        unsafe impl NSMenuDelegate for Target {}
    );

    impl Target {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(());
            unsafe { msg_send![super(this), init] }
        }
    }

    fn key_text(key: Key) -> Option<String> {
        let special = match key {
            Key::Period => ".",
            Key::Comma => ",",
            Key::OpenBracket => "[",
            Key::CloseBracket => "]",
            Key::Equals => "=",
            Key::Plus => "+",
            Key::Minus => "-",
            Key::Questionmark => "?",
            Key::Num1 => "1",
            Key::Enter => "\r",
            Key::Escape => "\u{1b}",
            Key::Tab => "\t",
            Key::Backspace => "\u{8}",
            Key::Delete => "\u{F728}",
            Key::ArrowUp => "\u{F700}",
            Key::ArrowDown => "\u{F701}",
            Key::ArrowLeft => "\u{F702}",
            Key::ArrowRight => "\u{F703}",
            Key::Home => "\u{F729}",
            Key::End => "\u{F72B}",
            Key::F5 => "\u{F708}",
            other => {
                let name = other.name();
                return (name.chars().count() == 1).then(|| name.to_lowercase());
            }
        };
        Some(special.to_string())
    }

    fn equivalent(keys: &[Keys]) -> (String, NSEventModifierFlags) {
        keys.iter()
            .find(|k| k.key == Key::Backspace)
            .or(keys.first())
            .and_then(|k| {
                let text = key_text(k.key)?;
                let mut mask = NSEventModifierFlags::empty();
                [
                    (k.command, NSEventModifierFlags::Command),
                    (k.shift, NSEventModifierFlags::Shift),
                    (k.alt, NSEventModifierFlags::Option),
                ]
                .into_iter()
                .filter(|(on, _)| *on)
                .for_each(|(_, flag)| mask |= flag);
                Some((text, mask))
            })
            .unwrap_or_else(|| (String::new(), NSEventModifierFlags::empty()))
    }

    fn item(
        mtm: MainThreadMarker,
        title: &str,
        action: Option<Sel>,
        key: &str,
        mask: NSEventModifierFlags,
    ) -> Retained<NSMenuItem> {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                action,
                &NSString::from_str(key),
            )
        };
        item.setKeyEquivalentModifierMask(mask);
        item
    }

    fn command(mtm: MainThreadMarker, target: &Target, cmd: Cmd, bindings: &Bindings) -> Retained<NSMenuItem> {
        let (key, mask) = equivalent(&bindings.keys(cmd));
        let entry = item(mtm, &menu_label(cmd), Some(sel!(kladRun:)), &key, mask);
        unsafe { entry.setTarget(Some(target)) };
        entry.setTag(Cmd::ALL.iter().position(|c| *c == cmd).unwrap_or_default() as isize);
        entry
    }

    fn menu(mtm: MainThreadMarker, title: &str, target: Option<&Target>) -> Retained<NSMenu> {
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
        if let Some(target) = target {
            menu.setDelegate(Some(ProtocolObject::from_ref(target)));
        }
        menu
    }

    fn holding(mtm: MainThreadMarker, sub: &NSMenu) -> Retained<NSMenuItem> {
        let holder = item(mtm, &sub.title().to_string(), None, "", NSEventModifierFlags::empty());
        holder.setSubmenu(Some(sub));
        holder
    }

    fn fill(
        mtm: MainThreadMarker,
        into: &NSMenu,
        items: &[Item],
        target: &Target,
        bindings: &Bindings,
        recent: &[String],
    ) {
        tidy(items, |i| !matches!(i, Item::Run(c) if IN_APP_MENU.contains(c)))
            .into_iter()
            .for_each(|entry| match entry {
                Item::Line => into.addItem(&NSMenuItem::separatorItem(mtm)),
                Item::Run(cmd) => into.addItem(&command(mtm, target, cmd, bindings)),
                Item::Sub(title, inner) => {
                    let sub = menu(mtm, title, Some(target));
                    fill(mtm, &sub, inner, target, bindings, recent);
                    into.addItem(&holding(mtm, &sub));
                }
                Item::Recent => {
                    let sub = menu(mtm, "Open recent", Some(target));
                    let paths: Vec<PathBuf> = recent.iter().take(RECENT).map(PathBuf::from).collect();
                    paths.iter().enumerate().for_each(|(i, path)| {
                        let entry = item(
                            mtm,
                            &recent_label(path),
                            Some(sel!(kladRun:)),
                            "",
                            NSEventModifierFlags::empty(),
                        );
                        unsafe { entry.setTarget(Some(target)) };
                        entry.setTag(RECENT_TAG + i as isize);
                        sub.addItem(&entry);
                    });
                    if !paths.is_empty() {
                        sub.addItem(&NSMenuItem::separatorItem(mtm));
                    }
                    sub.addItem(&command(mtm, target, Cmd::OpenRecent, bindings));
                    PATHS.with(|p| *p.borrow_mut() = paths);
                    into.addItem(&holding(mtm, &sub));
                }
            });
    }

    fn system(
        mtm: MainThreadMarker,
        title: &str,
        action: Sel,
        key: &str,
        mask: NSEventModifierFlags,
    ) -> Retained<NSMenuItem> {
        item(mtm, title, Some(action), key, mask)
    }

    pub fn install(ctx: &egui::Context, bindings: &Bindings, recent: &[String]) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let _ = CTX.set(ctx.clone());
        let target = TARGET.with(|t| t.borrow_mut().get_or_insert_with(|| Target::new(mtm)).clone());
        let app = NSApplication::sharedApplication(mtm);
        let bar = menu(mtm, "", None);
        let klad = menu(mtm, "klad", None);
        let services = menu(mtm, "Services", None);
        let command = NSEventModifierFlags::Command;
        klad.addItem(&self::command(mtm, &target, Cmd::About, bindings));
        klad.addItem(&NSMenuItem::separatorItem(mtm));
        klad.addItem(&self::command(mtm, &target, Cmd::Settings, bindings));
        klad.addItem(&NSMenuItem::separatorItem(mtm));
        klad.addItem(&holding(mtm, &services));
        klad.addItem(&NSMenuItem::separatorItem(mtm));
        klad.addItem(&system(mtm, "Hide klad", sel!(hide:), "h", command));
        klad.addItem(&system(
            mtm,
            "Hide others",
            sel!(hideOtherApplications:),
            "h",
            command | NSEventModifierFlags::Option,
        ));
        klad.addItem(&system(
            mtm,
            "Show all",
            sel!(unhideAllApplications:),
            "",
            NSEventModifierFlags::empty(),
        ));
        klad.addItem(&NSMenuItem::separatorItem(mtm));
        klad.addItem(&self::command(mtm, &target, Cmd::Quit, bindings));
        bar.addItem(&holding(mtm, &klad));
        let window = menu(mtm, "Window", None);
        window.addItem(&system(mtm, "Minimize", sel!(performMiniaturize:), "m", command));
        window.addItem(&system(
            mtm,
            "Zoom",
            sel!(performZoom:),
            "",
            NSEventModifierFlags::empty(),
        ));
        window.addItem(&NSMenuItem::separatorItem(mtm));
        window.addItem(&system(
            mtm,
            "Bring all to front",
            sel!(arrangeInFront:),
            "",
            NSEventModifierFlags::empty(),
        ));
        MENUS.iter().for_each(|m| {
            if m.title == "Help" {
                bar.addItem(&holding(mtm, &window));
            }
            let built = menu(mtm, m.title, Some(&target));
            fill(mtm, &built, m.items, &target, bindings, recent);
            bar.addItem(&holding(mtm, &built));
            if m.title == "Help" {
                app.setHelpMenu(Some(&built));
            }
        });
        app.setMainMenu(Some(&bar));
        app.setServicesMenu(Some(&services));
        app.setWindowsMenu(Some(&window));
        super::NATIVE.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{IN_APP_MENU, MENUS, in_menus};

    #[test]
    fn tidy_drops_separators_at_the_ends_and_twice_in_a_row() {
        let file = MENUS[0].items;
        let mac = tidy(file, |i| !matches!(i, Item::Run(c) if IN_APP_MENU.contains(c)));
        assert_ne!(
            mac.last(),
            Some(&Item::Line),
            "quit moved out leaves no line at the end"
        );
        assert!(mac.windows(2).all(|w| w != [Item::Line, Item::Line]));
        assert_eq!(tidy(file, |_| true).len(), file.len());
        assert!(!in_menus(&mac).contains(&Cmd::Quit));
    }

    #[test]
    fn picks_queue_up_until_taken_and_outside_a_drawing_only_files_run() {
        push(Picked::Run(Cmd::Export));
        push(Picked::Open(PathBuf::from("/w/a.klad")));
        assert_eq!(
            take(),
            [Picked::Run(Cmd::Export), Picked::Open(PathBuf::from("/w/a.klad"))]
        );
        assert!(take().is_empty());
        let outside = outside_a_drawing();
        assert!(outside.off.contains(&Cmd::Undo) && !outside.off.contains(&Cmd::NewDocument));
        show(outside.clone());
        assert_eq!(shown(), outside);
        assert_eq!(recent_label(Path::new("/w/rocks/ledge.klad")), "ledge  (rocks)");
    }
}
