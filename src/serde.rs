use nokhwa::{CameraFormat, FrameFormat, Resolution};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_with::{DeserializeAs, SerializeAs};

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

impl SerializeAs<CameraFormat> for CameraFormatDef {
    fn serialize_as<S>(value: &CameraFormat, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        CameraFormatDef::serialize(value, serializer)
    }
}

impl<'de> DeserializeAs<'de, CameraFormat> for CameraFormatDef {
    fn deserialize_as<D>(deserializer: D) -> Result<CameraFormat, D::Error>
    where
        D: Deserializer<'de>,
    {
        CameraFormatDef::deserialize(deserializer)
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
