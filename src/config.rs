use crate::serde::CameraFormatDef;
use egui::plot::{Line, Value, Values};
use egui::Vec2;
use glium::glutin::dpi::PhysicalSize;
use nokhwa::{CameraFormat, FrameFormat, Resolution};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone, Copy)]
pub enum Linearize {
    Off,
    Rec601,
    Rec709,
    SRgb,
}

impl Display for Linearize {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Linearize::Off => write!(f, "Off"),
            Linearize::Rec601 => write!(f, "Rec. 601"),
            Linearize::Rec709 => write!(f, "Rec. 709"),
            Linearize::SRgb => write!(f, "sRGB"),
        }
    }
}

impl Linearize {
    pub fn linearize(&self, value: f32) -> f32 {
        match self {
            Linearize::Off => value,
            Linearize::Rec709 | Linearize::Rec601 => {
                if value < 0.081 {
                    value / 4.5
                } else {
                    ((value + 0.099) / 1.099).powf(1. / 0.45)
                }
            }
            Linearize::SRgb => {
                if value < 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
pub struct ImportExportConfig {
    pub path: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy, Default)]
pub struct ReferencePoint {
    pub wavelength: f32,
    pub value: f32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ReferenceConfig {
    pub reference: Option<Vec<ReferencePoint>>,
    pub scale: f32,
}

impl Default for ReferenceConfig {
    fn default() -> Self {
        Self {
            reference: None,
            scale: 1.0,
        }
    }
}

impl ReferenceConfig {
    pub fn to_line(&self) -> Option<Line> {
        self.reference.as_ref().map(|reference| {
            Line::new(Values::from_values_iter(
                reference
                    .iter()
                    .map(|rp| Value::new(rp.wavelength, rp.value * self.scale)),
            ))
        })
    }

    pub fn get_value_at_wavelength(&self, wavelength: f32) -> Option<f32> {
        self.reference.as_ref().map(|r| {
            let mut sorted = r.clone();
            sorted.sort_by(|a, b| a.wavelength.partial_cmp(&b.wavelength).unwrap());
            let mut value = None;
            for (rp1, rp2) in sorted.iter().zip(sorted[1..].iter()) {
                if wavelength >= rp1.wavelength && wavelength < rp2.wavelength {
                    let a = (rp1.value - rp2.value) / (rp1.wavelength - rp2.wavelength);
                    value = Some((a * wavelength + rp1.value - a * rp1.wavelength) * self.scale);
                    break;
                }
            }
            value.unwrap_or(0.)
        })
    }
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone, Copy, Default)]
pub struct SpectrumWindow {
    pub offset: Vec2,
    pub size: Vec2,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CameraControl {
    pub id: u32,
    pub name: String,
    pub value: i32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub struct ViewConfig {
    pub window_size: PhysicalSize<u32>,
    pub image_scale: f32,
    pub draw_spectrum_r: bool,
    pub draw_spectrum_g: bool,
    pub draw_spectrum_b: bool,
    pub draw_spectrum_combined: bool,
    pub show_camera_window: bool,
    pub show_calibration_window: bool,
    pub show_postprocessing_window: bool,
    pub show_camera_control_window: bool,
    pub show_import_export_window: bool,
}

impl Default for ViewConfig {
    fn default() -> Self {
        Self {
            window_size: PhysicalSize::new(800, 600),
            image_scale: 0.25,
            draw_spectrum_r: true,
            draw_spectrum_g: true,
            draw_spectrum_b: true,
            draw_spectrum_combined: true,
            show_camera_window: true,
            show_calibration_window: false,
            show_postprocessing_window: false,
            show_camera_control_window: false,
            show_import_export_window: false,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ImageConfig {
    pub window: SpectrumWindow,
    pub flip: bool,
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            window: SpectrumWindow {
                offset: Vec2::new(100., 500.),
                size: Vec2::new(1500., 1.),
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SpectrumCalibration {
    pub low: SpectrumCalibrationPoint,
    pub high: SpectrumCalibrationPoint,
    pub linearize: Linearize,
    pub gain_r: f32,
    pub gain_g: f32,
    pub gain_b: f32,
    pub scaling: Option<Vec<f32>>,
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

    pub fn get_scaling_factor_from_index(&self, index: usize) -> f32 {
        if let Some(scaling) = self.scaling.as_ref() {
            *scaling.get(index).unwrap_or(&1.)
        } else {
            1.
        }
    }
}

impl Default for SpectrumCalibration {
    fn default() -> Self {
        Self {
            low: SpectrumCalibrationPoint {
                wavelength: 436,
                index: 261,
            },
            high: SpectrumCalibrationPoint {
                wavelength: 546,
                index: 486,
            },
            linearize: Linearize::Off,
            gain_r: 1.0,
            gain_g: 1.0,
            gain_b: 1.0,
            scaling: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PostprocessingConfig {
    pub spectrum_buffer_size: usize,
    pub spectrum_filter_active: bool,
    pub spectrum_filter_cutoff: f32,
}

impl Default for PostprocessingConfig {
    fn default() -> Self {
        Self {
            spectrum_buffer_size: 10,
            spectrum_filter_active: false,
            spectrum_filter_cutoff: 0.5,
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
    pub postprocessing_config: PostprocessingConfig,
    pub view_config: ViewConfig,
    pub reference_config: ReferenceConfig,
    pub import_export_config: ImportExportConfig,
}

impl Default for SpectrometerConfig {
    fn default() -> Self {
        let camera_format = CameraFormat::new(Resolution::new(1920, 1080), FrameFormat::MJPEG, 30);
        Self {
            camera_id: 0,
            camera_format,
            image_config: Default::default(),
            spectrum_calibration: Default::default(),
            postprocessing_config: Default::default(),
            view_config: Default::default(),
            reference_config: Default::default(),
            import_export_config: Default::default(),
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
        let s = SpectrumCalibration {
            low,
            high,
            linearize: Linearize::Off,
            gain_r: 0.0,
            gain_g: 0.0,
            gain_b: 0.0,
            scaling: None,
        };

        assert_relative_eq!(s.get_wavelength_delta(), 2.2);

        assert_relative_eq!(s.get_wavelength_from_index(49), 433.8);
        assert_relative_eq!(s.get_wavelength_from_index(50), 436.);
        assert_relative_eq!(s.get_wavelength_from_index(51), 438.2);
        assert_relative_eq!(s.get_wavelength_from_index(100), 546.);
        assert_relative_eq!(s.get_wavelength_from_index(101), 548.2);
    }
}
