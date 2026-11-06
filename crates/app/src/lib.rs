mod agent;
mod color;
mod commands;
mod composite;
mod editor;
mod frames;
mod gpu;
mod home;
mod jobs;
mod layout;
mod lift;
mod messages;
mod opened;
mod pen;
mod picker;
mod places;
mod planes;
mod settings;
mod start;
mod stats;
mod sync;
mod theme;
pub mod updates;
mod view;

pub use home::run;

pub const VERSION: &str = match env!("KLAD_BUILD").is_empty() {
    true => env!("CARGO_PKG_VERSION"),
    false => concat!(env!("CARGO_PKG_VERSION"), " (", env!("KLAD_BUILD"), ")"),
};

fn background() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
        rayon::ThreadPoolBuilder::new()
            .num_threads(cores.saturating_sub(1).max(1))
            .thread_name(|i| format!("draw background {i}"))
            .build()
            .expect("a background thread pool")
    })
}
