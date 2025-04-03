use env_logger::Env;

pub mod camera;
pub mod color;
pub mod config;
pub mod gui;
pub mod spectrum;
pub mod spectrum_feed_server;
pub mod tungsten_halogen;

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum ThreadId {
    Camera,
    Main,
}

#[derive(Debug, PartialEq, Clone)]
pub struct ThreadResult {
    pub id: ThreadId,
    pub result: Result<(), String>,
}

/// Wrapper struct to hold arbitrary data with a start and end timestamp
/// denoting the time the data was captured during.
#[derive(Debug)]
pub struct Timestamped<T> {
    pub start: std::time::SystemTime,
    pub end: std::time::SystemTime,
    pub data: T,
}

pub fn init_logging() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
}
