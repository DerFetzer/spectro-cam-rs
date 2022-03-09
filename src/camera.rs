use crate::config::{CameraControl, ImageConfig};
use flume::{Receiver, Sender};
use image::{DynamicImage, GenericImageView, ImageBuffer, Rgb};
use nokhwa::{CameraFormat, FrameFormat, Resolution, ThreadedCamera};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use v4l::Control;

#[derive(Debug, Clone)]
pub struct CameraInfo {
    pub info: nokhwa::CameraInfo,
    pub formats: Vec<CameraFormat>,
}

impl CameraInfo {
    pub fn get_default_camera_formats() -> Vec<CameraFormat> {
        vec![
            CameraFormat::default(),
            CameraFormat::new(Resolution::new(640, 480), FrameFormat::YUYV, 30),
        ]
    }
}

#[derive(Debug, Clone)]
pub enum CameraEvent {
    StartStream { id: usize, format: CameraFormat },
    StopStream,
    Config(ImageConfig),
    Controls(Vec<CameraControl>),
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
        let controls: Arc<Mutex<Option<Vec<CameraControl>>>> = Arc::new(Mutex::new(None));
        let mut join_handle = None;
        loop {
            let config = Arc::clone(&config);
            let controls = Arc::clone(&controls);
            if let Ok(event) = self.config_rx.try_recv() {
                match event {
                    CameraEvent::StartStream { id, format } => {
                        let frame_tx = self.frame_tx.clone();
                        let window_tx = self.window_tx.clone();
                        let exit_rx = exit_rx.clone();
                        let hdl = std::thread::spawn(move || {
                            let mut camera = ThreadedCamera::new(id, Some(format)).unwrap();

                            camera.open_stream(|_| {}).unwrap();

                            let mut inner_config = None;

                            loop {
                                // Check exit request
                                if exit_rx.try_recv().is_ok() {
                                    return;
                                }
                                // Check for new config
                                if let Some(cfg) = config.lock().unwrap().take() {
                                    for control in &cfg.controls {
                                        Self::set_control(&mut camera, control);
                                    }
                                    inner_config = Some(cfg);
                                }
                                // Check for new controls
                                if let Some(controls) = controls.lock().unwrap().take() {
                                    for control in controls.iter() {
                                        Self::set_control(&mut camera, control);
                                    }
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
                    CameraEvent::Controls(ctrls) => {
                        *controls.lock().unwrap() = Some(ctrls);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn set_control(camera: &mut ThreadedCamera, control: &CameraControl) {
        camera
            .set_raw_camera_control(&control.id, &Control::Value(control.value))
            .map_err(|e| log::warn!("Could not write camera control: {:?}", e))
            .ok();
    }
}
