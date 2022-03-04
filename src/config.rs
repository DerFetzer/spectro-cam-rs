use crate::serde::CameraFormatDef;
use egui::Vec2;
use nokhwa::{CameraFormat, FrameFormat, Resolution};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone, Copy, Default)]
pub struct SpectrumWindow {
    pub offset: Vec2,
    pub size: Vec2,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CameraControl {
    pub name: String,
    pub value: i32,
    pub automatic: bool,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub struct ViewConfig {
    pub image_scale: f32,
}

impl Default for ViewConfig {
    fn default() -> Self {
        Self { image_scale: 0.25 }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ImageConfig {
    pub controls: HashMap<String, CameraControl>,
    pub window: SpectrumWindow,
    pub flip: bool,
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            controls: Default::default(),
            window: SpectrumWindow {
                offset: Vec2::new(0., 400.),
                size: Vec2::new(1100., 120.),
            },
            flip: true,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone)]
pub struct SpectrumCalibrationPoint {
    pub wavelength: u32,
    pub index: usize,
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone)]
pub struct SpectrumCalibration {
    pub low: SpectrumCalibrationPoint,
    pub high: SpectrumCalibrationPoint,
}

impl SpectrumCalibration {
    fn get_wavelength_delta(&self) -> f32 {
        (self.high.wavelength - self.low.wavelength) as f32
            / (self.high.index - self.low.index) as f32
    }

    pub fn get_wavelength_from_index(&self, index: usize) -> f32 {
        self.low.wavelength as f32
            + (index as f32 - self.low.index as f32) * self.get_wavelength_delta()
    }
}

impl Default for SpectrumCalibration {
    fn default() -> Self {
        Self {
            low: SpectrumCalibrationPoint {
                wavelength: 436,
                index: 50,
            },
            high: SpectrumCalibrationPoint {
                wavelength: 546,
                index: 100,
            },
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SpectrometerConfig {
    pub camera_id: usize,
    #[serde(with = "CameraFormatDef")]
    pub camera_format: CameraFormat,
    pub image_config: ImageConfig,
    pub spectrum_calibration: SpectrumCalibration,
    pub spectrum_filter_size: usize,
    pub view_config: ViewConfig,
}

impl Default for SpectrometerConfig {
    fn default() -> Self {
        let camera_format = CameraFormat::new(Resolution::new(1920, 1080), FrameFormat::MJPEG, 30);
        Self {
            camera_id: 0,
            camera_format,
            image_config: Default::default(),
            spectrum_calibration: Default::default(),
            spectrum_filter_size: 10,
            view_config: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn spectrum_calibration() {
        let low = SpectrumCalibrationPoint {
            wavelength: 436,
            index: 50,
        };
        let high = SpectrumCalibrationPoint {
            wavelength: 546,
            index: 100,
        };
        let s = SpectrumCalibration { low, high };

        assert_relative_eq!(s.get_wavelength_delta(), 2.2);

        assert_relative_eq!(s.get_wavelength_from_index(49), 433.8);
        assert_relative_eq!(s.get_wavelength_from_index(50), 436.);
        assert_relative_eq!(s.get_wavelength_from_index(51), 438.2);
        assert_relative_eq!(s.get_wavelength_from_index(100), 546.);
        assert_relative_eq!(s.get_wavelength_from_index(101), 548.2);
    }
}
