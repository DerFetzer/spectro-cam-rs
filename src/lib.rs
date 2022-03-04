use log::{set_max_level, LevelFilter};
use simple_logger::SimpleLogger;

pub fn init_logging() {
    SimpleLogger::new().init().unwrap();
    set_max_level(LevelFilter::Info);
}
