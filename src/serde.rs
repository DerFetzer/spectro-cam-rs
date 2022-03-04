use nokhwa::{CameraFormat, FrameFormat, Resolution};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(remote = "CameraFormat")]
pub struct CameraFormatDef {
    #[serde(with = "ResolutionDef")]
    #[serde(getter = "CameraFormat::resolution")]
    resolution: Resolution,
    #[serde(with = "FrameFormatDef")]
    #[serde(getter = "CameraFormat::format")]
    format: FrameFormat,
    #[serde(getter = "CameraFormat::frame_rate")]
    frame_rate: u32,
}

impl From<CameraFormatDef> for CameraFormat {
    fn from(cfd: CameraFormatDef) -> Self {
        CameraFormat::new(cfd.resolution, cfd.format, cfd.frame_rate)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(remote = "Resolution")]
pub struct ResolutionDef {
    width_x: u32,
    height_y: u32,
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Deserialize, Serialize)]
#[serde(remote = "FrameFormat")]
pub enum FrameFormatDef {
    MJPEG,
    YUYV,
}
