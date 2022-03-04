use crate::config::ImageConfig;
use flume::{Receiver, Sender};
use image::{DynamicImage, GenericImageView, ImageBuffer, Rgb};
use nokhwa::{CameraFormat, ThreadedCamera};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum CameraEvent {
    StartStream { id: usize, format: CameraFormat },
    StopStream,
    Config(ImageConfig),
}

struct Exit {}

pub struct CameraThread {
    frame_tx: Sender<ImageBuffer<Rgb<u8>, Vec<u8>>>,
    window_tx: Sender<ImageBuffer<Rgb<u8>, Vec<u8>>>,
    config_rx: Receiver<CameraEvent>,
}

impl CameraThread {
    pub fn new(
        frame_tx: Sender<ImageBuffer<Rgb<u8>, Vec<u8>>>,
        window_tx: Sender<ImageBuffer<Rgb<u8>, Vec<u8>>>,
        config_rx: Receiver<CameraEvent>,
    ) -> Self {
        Self {
            frame_tx,
            window_tx,
            config_rx,
        }
    }

    pub fn run(&mut self) -> ! {
        let (exit_tx, exit_rx) = flume::bounded(0);
        let config: Arc<Mutex<Option<ImageConfig>>> = Arc::new(Mutex::new(None));
        let mut join_handle = None;
        loop {
            let config = Arc::clone(&config);
            if let Ok(event) = self.config_rx.try_recv() {
                match event {
                    CameraEvent::StartStream { id, format } => {
                        let frame_tx = self.frame_tx.clone();
                        let window_tx = self.window_tx.clone();
                        let exit_rx = exit_rx.clone();
                        let hdl = std::thread::spawn(move || {
                            let mut camera = ThreadedCamera::new(id, Some(format)).unwrap();
                            let controls = camera.camera_controls_string().unwrap();

                            camera.open_stream(|_| {}).unwrap();

                            let mut inner_config = None;

                            loop {
                                // Check exit request
                                if exit_rx.try_recv().is_ok() {
                                    return;
                                }
                                // Check for new config
                                if let Some(cfg) = config.lock().unwrap().take() {
                                    for (name, control) in &cfg.controls {
                                        let mut cc = *controls.get(name).unwrap();
                                        cc.set_value(control.value).unwrap();
                                        camera.set_camera_control(cc).unwrap();
                                    }
                                    inner_config = Some(cfg);
                                }
                                // Get frame
                                let frame = camera.poll_frame().unwrap();

                                // TODO: Remove repacking after nokhwa uses image = "0.24"
                                let (width, heigth) = frame.dimensions();
                                let mut frame =
                                    ImageBuffer::from_raw(width, heigth, frame.into_raw()).unwrap();

                                if let Some(cfg) = &inner_config {
                                    // Flip
                                    if cfg.flip {
                                        frame = DynamicImage::ImageRgb8(frame).fliph().into_rgb8()
                                    }
                                    // Extract window
                                    let window = frame
                                        .view(
                                            cfg.window.offset.x as u32,
                                            cfg.window.offset.y as u32,
                                            cfg.window.size.x as u32,
                                            cfg.window.size.y as u32,
                                        )
                                        .to_image();
                                    if window_tx.send(window).is_err() {
                                        return;
                                    };
                                }
                                if frame_tx.send(frame).is_err() {
                                    return;
                                };
                                std::thread::sleep(Duration::from_millis(1));
                            }
                        });
                        join_handle = Some(hdl);
                    }
                    CameraEvent::StopStream => {
                        if let Some(hdl) = join_handle.take() {
                            exit_tx.send(Exit {}).unwrap();
                            hdl.join().unwrap();
                        }
                    }
                    CameraEvent::Config(cfg) => {
                        *config.lock().unwrap() = Some(cfg);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
