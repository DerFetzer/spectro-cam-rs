use crate::Timestamped;
use crate::config::ImageConfig;
use crate::{ThreadId, ThreadResult};
use flume::{Receiver, Sender};
use image::{DynamicImage, GenericImageView, ImageBuffer, Rgb};
use jiff::Unit;
use log::{error, trace};
use nokhwa::CallbackCamera;
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{
    CameraFormat, CameraIndex, ControlValueSetter, FrameFormat, KnownCameraControl,
    RequestedFormat, RequestedFormatType, Resolution,
};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct CameraInfo {
    pub info: nokhwa::utils::CameraInfo,
    pub formats: Vec<CameraFormat>,
}

impl CameraInfo {
    pub fn get_default_camera_format_types() -> Vec<RequestedFormatType> {
        vec![
            RequestedFormatType::None,
            RequestedFormatType::AbsoluteHighestResolution,
            RequestedFormatType::Exact(CameraFormat::default()),
            RequestedFormatType::Exact(CameraFormat::new(
                Resolution::new(640, 480),
                FrameFormat::YUYV,
                30,
            )),
        ]
    }
}

#[derive(Debug, Clone)]
pub enum CameraEvent {
    StartStream {
        id: CameraIndex,
        format: CameraFormat,
    },
    StopStream,
    Config(ImageConfig),
    Controls(Vec<(KnownCameraControl, ControlValueSetter)>),
}

struct Exit {}

pub type SharedFrameBuffer = Arc<Mutex<Option<ImageBuffer<Rgb<u8>, Vec<u8>>>>>;

pub struct CameraThread {
    frame_tx: SharedFrameBuffer,
    window_tx: Sender<Timestamped<ImageBuffer<Rgb<u8>, Vec<u8>>>>,
    config_rx: Receiver<CameraEvent>,
    result_tx: Sender<ThreadResult>,
}

impl CameraThread {
    pub fn new(
        frame_tx: SharedFrameBuffer,
        window_tx: Sender<Timestamped<ImageBuffer<Rgb<u8>, Vec<u8>>>>,
        config_rx: Receiver<CameraEvent>,
        result_tx: Sender<ThreadResult>,
    ) -> Self {
        Self {
            frame_tx,
            window_tx,
            config_rx,
            result_tx,
        }
    }

    pub fn run(&mut self) {
        let (exit_tx, exit_rx) = flume::bounded(0);
        let config: Arc<Mutex<Option<ImageConfig>>> = Arc::new(Mutex::new(None));
        #[allow(clippy::type_complexity)]
        let controls: Arc<Mutex<Option<Vec<(KnownCameraControl, ControlValueSetter)>>>> =
            Arc::new(Mutex::new(None));
        let mut join_handle = None;
        while let Ok(event) = self.config_rx.recv() {
            match event {
                CameraEvent::StartStream { id, format } => {
                    let config = Arc::clone(&config);
                    let controls = Arc::clone(&controls);

                    let frame_tx = self.frame_tx.clone();
                    let window_tx = self.window_tx.clone();
                    let result_tx = self.result_tx.clone();
                    let exit_rx = exit_rx.clone();
                    let hdl = std::thread::spawn(move || {
                        let mut camera = match CallbackCamera::new(
                            id,
                            RequestedFormat::new::<RgbFormat>(
                                nokhwa::utils::RequestedFormatType::Exact(format),
                            ),
                            |_| {},
                        ) {
                            Ok(camera) => camera,
                            Err(e) => {
                                log::error!("{:?}", e);
                                result_tx
                                    .send(ThreadResult {
                                        id: ThreadId::Camera,
                                        result: Err("Could not initialize camera".into()),
                                    })
                                    .unwrap();
                                return;
                            }
                        };

                        if let Err(e) = camera.open_stream() {
                            log::error!("{:?}", e);
                            result_tx
                                .send(ThreadResult {
                                    id: ThreadId::Camera,
                                    result: Err("Could not open stream".into()),
                                })
                                .unwrap();
                            return;
                        };

                        result_tx
                            .send(ThreadResult {
                                id: ThreadId::Camera,
                                result: Ok(()),
                            })
                            .unwrap();

                        let mut inner_config = None;

                        let mut previous_frame_timestamp =
                            jiff::Zoned::now().round(Unit::Millisecond).unwrap();
                        loop {
                            // Check exit request
                            if exit_rx.try_recv().is_ok() {
                                return;
                            }
                            // Check for new config
                            if let Some(cfg) = config.lock().unwrap().take() {
                                inner_config = Some(cfg);
                            }
                            // Check for new controls
                            if let Some(controls) = controls.lock().unwrap().take() {
                                for (control, setter) in &controls {
                                    let control: &KnownCameraControl = control;
                                    if let Err(e) =
                                        camera.set_camera_control(*control, setter.clone())
                                    {
                                        log::error!("{:?}", e);
                                    }
                                }
                            }
                            // Get frame
                            let mut frame = match camera
                                .poll_frame()
                                .and_then(|frame| frame.decode_image::<RgbFormat>())
                            {
                                Ok(frame) => frame,
                                Err(e) => {
                                    log::error!("{:?}", e);
                                    result_tx
                                        .send(ThreadResult {
                                            id: ThreadId::Camera,
                                            result: Err("Could not poll for frame".into()),
                                        })
                                        .unwrap();
                                    return;
                                }
                            };
                            // A timestamp guaranteed to be after the last photon included in the frame
                            // hit the camera sensor
                            let frame_end_timestamp =
                                jiff::Zoned::now().round(Unit::Millisecond).unwrap();
                            // A timestamp most likely before the first photon included in the frame
                            // hit the camera sensor.
                            let frame_start_timestamp = previous_frame_timestamp.clone();
                            previous_frame_timestamp = frame_end_timestamp.clone();
                            trace!("Got frame from camera");

                            if let Some(cfg) = &inner_config {
                                // Flip
                                if cfg.flip {
                                    frame = DynamicImage::ImageRgb8(frame).fliph().into_rgb8();
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
                                let timed_window = Timestamped {
                                    start: frame_start_timestamp,
                                    end: frame_end_timestamp,
                                    data: window,
                                };
                                if let Err(e) = window_tx.send(timed_window) {
                                    error!("Could not send window: {e}");
                                    return;
                                };
                            }
                            *frame_tx.lock().expect("Mutex poisoned") = Some(frame);
                        }
                    });
                    join_handle = Some(hdl);
                }
                CameraEvent::StopStream => {
                    if let Some(hdl) = join_handle.take() {
                        exit_tx.send(Exit {}).ok();
                        hdl.join().ok();
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
        log::debug!("Camera thread exiting");
    }
}
