use egui::Vec2;
use egui_plot::{Line, PlotPoints};
use nokhwa::utils::CameraFormat;
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

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ImportExportConfig {
    pub path: String,
}

impl Default for ImportExportConfig {
    fn default() -> Self {
        Self {
            path: "spectrum.csv".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy, Default)]
pub struct SpectrumPoint {
    pub wavelength: f32,
    pub value: f32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ReferenceConfig {
    pub reference: Option<Vec<SpectrumPoint>>,
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
            Line::new(
                "Reference line",
                PlotPoints::from_iter(
                    reference
                        .iter()
                        .map(|rp| [rp.wavelength as f64, (rp.value * self.scale) as f64]),
                ),
            )
        })
    }

    pub fn get_value_at_wavelength(&self, wavelength: f32) -> Option<f32> {
        self.reference.as_ref().map(|r| {
            let mut sorted = r.clone();
            sorted.sort_by(|a, b| a.wavelength.partial_cmp(&b.wavelength).unwrap());
            let mut value = None;
            for (rp1, rp2) in sorted.iter().zip(sorted[1..].iter()) {
                if wavelength >= rp1.wavelength && wavelength <= rp2.wavelength {
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

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub struct ViewConfig {
    pub image_scale: f32,
    pub draw_spectrum_r: bool,
    pub draw_spectrum_g: bool,
    pub draw_spectrum_b: bool,
    pub draw_spectrum_combined: bool,
    pub draw_color_polygons: bool,
    pub draw_peaks: bool,
    pub draw_dips: bool,
    pub peaks_dips_unique_window: f32,
    pub peaks_dips_find_window: usize,
    pub show_camera_window: bool,
    pub show_calibration_window: bool,
    pub show_postprocessing_window: bool,
    pub show_camera_control_window: bool,
    pub show_import_export_window: bool,
}

impl Default for ViewConfig {
    fn default() -> Self {
        Self {
            image_scale: 0.25,
            draw_spectrum_r: true,
            draw_spectrum_g: true,
            draw_spectrum_b: true,
            draw_spectrum_combined: true,
            draw_color_polygons: true,
            draw_peaks: true,
            draw_dips: true,
            peaks_dips_unique_window: 50.,
            peaks_dips_find_window: 5,
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

impl ImageConfig {
    pub fn clamp(&mut self, width: f32, height: f32) {
        self.window.offset = self.window.offset.min(Vec2::new(width, height));
        self.window.size = self
            .window
            .size
            .min(Vec2::new(width, height) - self.window.offset);
    }
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone)]
pub struct SpectrumCalibrationPoint {
    pub wavelength: u32,
    pub index: usize,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone, Copy)]
pub enum GainPresets {
    Unity,
    Rec601,
    Rec709,
    SRgb,
}

impl GainPresets {
    pub fn get_gain(&self) -> (f32, f32, f32) {
        match self {
            GainPresets::Unity => (1., 1., 1.),
            GainPresets::Rec601 => (0.299, 0.587, 0.114),
            GainPresets::Rec709 | GainPresets::SRgb => (0.2126, 0.7152, 0.0722),
        }
    }
}

impl Display for GainPresets {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            GainPresets::Unity => write!(f, "Unity"),
            GainPresets::Rec601 => write!(f, "Rec. 601"),
            GainPresets::Rec709 => write!(f, "Rec. 709"),
            GainPresets::SRgb => write!(f, "sRGB"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SpectrumCalibration {
    pub low: SpectrumCalibrationPoint,
    pub high: SpectrumCalibrationPoint,
    pub linearize: Linearize,
    pub gain_r: f32,
    pub gain_g: f32,
    pub gain_b: f32,
    /// If reference calibration is applied, this contains the error correcting scaling factor
    /// for each index. Index corresponding to pixel column in the source image.
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

    /// Returns the reference calibration scaling factor for the given index.
    ///
    /// Index means the index in the spectrum from low to high wavelength.
    /// These indexes correspond to the pixel columns in the source image.
    pub fn get_scaling_factor_from_index(&self, index: usize) -> f32 {
        if let Some(scaling) = self.scaling.as_ref() {
            *scaling.get(index).unwrap_or(&1.)
        } else {
            1.
        }
    }

    pub fn set_gain_preset(&mut self, preset: GainPresets) {
        let factors = preset.get_gain();
        self.gain_r = factors.0;
        self.gain_g = factors.1;
        self.gain_b = factors.2;
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

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SpectrometerConfig {
    pub camera_id: usize,
    pub camera_format: Option<CameraFormat>,
    pub image_config: ImageConfig,
    pub spectrum_calibration: SpectrumCalibration,
    pub postprocessing_config: PostprocessingConfig,
    pub view_config: ViewConfig,
    pub reference_config: ReferenceConfig,
    pub import_export_config: ImportExportConfig,
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

    #[test]
    fn linearize() {
        for l in [
            Linearize::Off,
            Linearize::Rec709,
            Linearize::Rec601,
            Linearize::SRgb,
        ] {
            assert_eq!(l.linearize(0.), 0.);
            if l == Linearize::Off {
                assert_eq!(l.linearize(0.5), 0.5);
            } else {
                assert!(l.linearize(0.5) < 0.5);
            }
            assert_eq!(l.linearize(1.), 1.);
        }
    }

    #[test]
    fn reference_config() {
        let rc = ReferenceConfig {
            reference: Some(vec![
                SpectrumPoint {
                    wavelength: 100.,
                    value: 1.,
                },
                SpectrumPoint {
                    wavelength: 200.,
                    value: 2.,
                },
            ]),
            scale: 1.0,
        };

        assert_eq!(rc.get_value_at_wavelength(100.), Some(1.0));
        assert_eq!(rc.get_value_at_wavelength(150.), Some(1.5));
        assert_eq!(rc.get_value_at_wavelength(200.), Some(2.0));
    }

    #[test]
    fn image_config() {
        let mut ic = ImageConfig {
            window: SpectrumWindow {
                offset: Vec2::new(100., 50.),
                size: Vec2::new(1000., 500.),
            },
            flip: false,
        };

        ic.clamp(500., 400.);

        assert_eq!(ic.window.offset, Vec2::new(100., 50.));
        assert_eq!(ic.window.size, Vec2::new(400., 350.));
    }
}
