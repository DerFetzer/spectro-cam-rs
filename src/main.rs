use std::sync::{Arc, Mutex};

use spectro_cam_rs::camera::CameraThread;
use spectro_cam_rs::gui::SpectrometerGui;
use spectro_cam_rs::init_logging;
use spectro_cam_rs::spectrum::SpectrumCalculator;

fn main() -> eframe::Result {
    init_logging();

    let frame = Arc::new(Mutex::new(None));

    // Image buffer holding the last read frame from the camera. Consumed by the GUI to display the camera feed.
    let (frame_tx, frame_rx) = (frame.clone(), frame.clone());
    // Channel transporting image buffers from the camera thread to the spectrum calculator thread.
    // This is bounded to provide backpressure handling. If the spectrum calculator is slower than the
    // framerate of the camera, frames will be dropped instead of filling up the memory.
    let (window_tx, window_rx) = flume::bounded(5);
    let (spectrum_tx, spectrum_rx) = flume::bounded(1000);
    let (config_tx, config_rx) = flume::unbounded();
    let (result_tx, result_rx) = flume::unbounded();

    std::thread::spawn(move || CameraThread::new(frame_tx, window_tx, config_rx, result_tx).run());
    std::thread::spawn(move || SpectrumCalculator::new(window_rx, spectrum_tx).run());

    let native_options = eframe::NativeOptions::default();

    eframe::run_native(
        "spectro-cam-rs",
        native_options,
        Box::new(|cc| {
            let gui = SpectrometerGui::new(cc, config_tx, spectrum_rx, result_rx, frame_rx);
            Ok(Box::new(gui))
        }),
    )
}
