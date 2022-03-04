use nokhwa::{Camera, CameraFormat, FrameFormat, Resolution};
use spectro_cam_rs::init_logging;

fn main() {
    init_logging();
    log::info!("Start");

    let mut camera = Camera::new(0, None).unwrap();

    let known = camera.camera_controls_known_camera_controls().unwrap();
    log::info!("{known:?}");

    match camera.compatible_fourcc() {
        Ok(fcc) => {
            for ff in fcc {
                match camera.compatible_list_by_resolution(ff) {
                    Ok(compat) => {
                        log::info!("For FourCC {}", ff);
                        for (res, fps) in compat {
                            log::info!("{}x{}: {:?}", res.width(), res.height(), fps);
                        }
                    }
                    Err(why) => {
                        log::info!(
                            "Failed to get compatible resolution/FPS list for FrameFormat {}: {}",
                            ff,
                            why.to_string()
                        )
                    }
                }
            }
        }
        Err(why) => {
            log::info!("Failed to get compatible FourCC: {}", why.to_string())
        }
    }

    camera
        .set_camera_format(CameraFormat::new(
            Resolution::new(1280, 720),
            FrameFormat::MJPEG,
            30,
        ))
        .unwrap();
    camera.open_stream().unwrap();
    let frame = camera.frame().unwrap();
    frame.save("test.png").unwrap();
    camera.stop_stream().unwrap();
}
