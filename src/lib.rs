use env_logger::Env;

pub mod camera;
pub mod color;
pub mod config;
pub mod gui;
pub mod spectrum;
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

pub fn init_logging() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
}
