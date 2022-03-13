use log::{set_max_level, LevelFilter};
use simple_logger::SimpleLogger;

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
    SimpleLogger::new().init().unwrap();
    set_max_level(LevelFilter::Info);
}
