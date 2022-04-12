use crate::config::{Linearize, ReferenceConfig, SpectrumCalibration, SpectrumPoint};
use crate::SpectrometerConfig;
use biquad::{
    Biquad, Coefficients, DirectForm2Transposed, Hertz, ToHertz, Type, Q_BUTTERWORTH_F32,
};
use egui::plot::{Line, MarkerShape, Points, Text, Value, Values};
use egui::Color32;
use flume::{Receiver, Sender};
use image::{ImageBuffer, Pixel, Rgb};
use nalgebra::{Dynamic, OMatrix, U3, U4};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub type SpectrumRgb = OMatrix<f32, U3, Dynamic>;
pub type Spectrum = OMatrix<f32, U4, Dynamic>;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy, Default)]
pub struct SpectrumExportPoint {
    pub wavelength: f32,
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub sum: f32,
}

pub struct SpectrumCalculator {
    window_rx: Receiver<ImageBuffer<Rgb<u8>, Vec<u8>>>,
    spectrum_tx: Sender<SpectrumRgb>,
}

impl SpectrumCalculator {
    pub fn new(
        window_rx: Receiver<ImageBuffer<Rgb<u8>, Vec<u8>>>,
        spectrum_tx: Sender<SpectrumRgb>,
    ) -> Self {
        SpectrumCalculator {
            window_rx,
            spectrum_tx,
        }
    }

    pub fn run(&mut self) -> ! {
        loop {
            if let Ok(window) = self.window_rx.recv() {
                let columns = window.width();
                let rows = window.height();
                let max_value = rows * u8::MAX as u32 * 3;

                let spectrum: SpectrumRgb = window
                    .rows()
                    .par_bridge()
                    .map(|r| {
                        SpectrumRgb::from_vec(
                            r.flat_map(|p| p.channels().iter().map(|&v| v as f32))
                                .collect::<Vec<f32>>(),
                        )
                    })
                    .reduce(
                        || SpectrumRgb::from_element(columns as usize, 0.),
                        |a, b| a + b,
                    )
                    / max_value as f32;

                self.spectrum_tx.send(spectrum).unwrap();
            }
        }
    }
}

pub struct SpectrumContainer {
    spectrum: Spectrum,
    spectrum_buffer: VecDeque<SpectrumRgb>,
    zero_reference: Option<Spectrum>,
    spectrum_rx: Receiver<SpectrumRgb>,
}

impl SpectrumContainer {
    pub fn new(spectrum_rx: Receiver<SpectrumRgb>) -> Self {
        SpectrumContainer {
            spectrum: Spectrum::zeros(0),
            spectrum_buffer: VecDeque::with_capacity(100),
            zero_reference: None,
            spectrum_rx,
        }
    }

    pub fn clear_buffer(&mut self) {
        self.spectrum_buffer.clear();
    }

    pub fn update(&mut self, config: &SpectrometerConfig) {
        if let Ok(spectrum) = self.spectrum_rx.try_recv() {
            self.update_spectrum(spectrum, config);
        }
    }

    fn update_spectrum(&mut self, mut spectrum: SpectrumRgb, config: &SpectrometerConfig) {
        let ncols = spectrum.ncols();

        // Clear buffer and zero reference on dimension change
        if let Some(s) = self.spectrum_buffer.get(0) {
            if s.ncols() != ncols {
                self.spectrum_buffer.clear();
                self.zero_reference = None;
            }
        }

        if config.spectrum_calibration.linearize != Linearize::Off {
            spectrum
                .iter_mut()
                .for_each(|v| *v = config.spectrum_calibration.linearize.linearize(*v));
        }

        self.spectrum_buffer.push_front(spectrum);
        self.spectrum_buffer
            .truncate(config.postprocessing_config.spectrum_buffer_size);

        let mut combined_buffer = self
            .spectrum_buffer
            .par_iter()
            .cloned()
            .reduce(|| SpectrumRgb::from_element(ncols, 0.), |a, b| a + b)
            / self.spectrum_buffer.len() as f32;

        combined_buffer.set_row(
            0,
            &(combined_buffer.row(0) * config.spectrum_calibration.gain_r),
        );
        combined_buffer.set_row(
            1,
            &(combined_buffer.row(1) * config.spectrum_calibration.gain_g),
        );
        combined_buffer.set_row(
            2,
            &(combined_buffer.row(2) * config.spectrum_calibration.gain_b),
        );

        let mut current_spectrum = Spectrum::from_rows(&[
            combined_buffer.row(0).clone_owned(),
            combined_buffer.row(1).clone_owned(),
            combined_buffer.row(2).clone_owned(),
            if config.spectrum_calibration.scaling.is_some() {
                let mut sum = combined_buffer.row_sum();
                sum.iter_mut().enumerate().for_each(|(i, v)| {
                    *v *= config.spectrum_calibration.get_scaling_factor_from_index(i);
                });
                sum
            } else {
                combined_buffer.row_sum()
            },
        ]);

        if config.postprocessing_config.spectrum_filter_active {
            let cutoff = config
                .postprocessing_config
                .spectrum_filter_cutoff
                .clamp(0.001, 1.);
            let fs: Hertz<f32> = 2.0.hz();
            let f0: Hertz<f32> = cutoff.hz();

            let coeffs =
                Coefficients::<f32>::from_params(Type::LowPass, fs, f0, Q_BUTTERWORTH_F32).unwrap();
            for mut channel in current_spectrum.row_iter_mut() {
                let mut biquad = DirectForm2Transposed::<f32>::new(coeffs);
                for sample in channel.iter_mut() {
                    *sample = biquad.run(*sample);
                }
                // Apply filter in reverse to compensate phase error
                for sample in channel.iter_mut().rev() {
                    *sample = biquad.run(*sample);
                }
            }
        }

        if let Some(zero_reference) = self.zero_reference.as_ref() {
            current_spectrum -= zero_reference;
        }

        self.spectrum = current_spectrum;
    }

    pub fn spectrum_to_peaks_and_dips(
        &self,
        peaks: bool,
        config: &SpectrometerConfig,
    ) -> (Points, Vec<Text>) {
        let mut peaks_dips = Vec::new();

        let spectrum: Vec<_> = self.spectrum.row(3).iter().cloned().collect();

        let windows_size = config.view_config.peaks_dips_find_window * 2 + 1;
        let mid_index = (windows_size - 1) / 2;

        let max_spectrum_value = spectrum
            .iter()
            .cloned()
            .reduce(f32::max)
            .unwrap_or_default();

        for (i, win) in spectrum.as_slice().windows(windows_size).enumerate() {
            let (lower, upper) = win.split_at(mid_index);

            if lower.iter().chain(upper[1..].iter()).all(|&v| {
                if peaks {
                    v < win[mid_index]
                } else {
                    v > win[mid_index]
                }
            }) {
                peaks_dips.push(SpectrumPoint {
                    wavelength: config
                        .spectrum_calibration
                        .get_wavelength_from_index(i + mid_index),
                    value: win[mid_index],
                })
            }
        }

        let mut filtered_peaks_dips = Vec::new();
        let mut peak_dip_labels = Vec::new();

        let window = config.view_config.peaks_dips_unique_window;

        for peak_dip in &peaks_dips {
            if peak_dip.value
                == peaks_dips
                    .iter()
                    .filter(|sp| {
                        sp.wavelength > peak_dip.wavelength - window / 2.
                            && sp.wavelength < peak_dip.wavelength + window / 2.
                    })
                    .map(|sp| sp.value)
                    .reduce(if peaks { f32::max } else { f32::min })
                    .unwrap()
            {
                filtered_peaks_dips.push(peak_dip);
                peak_dip_labels.push(
                    Text::new(
                        Value::new(
                            peak_dip.wavelength,
                            if peaks {
                                peak_dip.value + (max_spectrum_value * 0.01)
                            } else {
                                peak_dip.value - (max_spectrum_value * 0.01)
                            },
                        ),
                        format!("{}", peak_dip.wavelength as u32),
                    )
                    .color(if peaks {
                        Color32::LIGHT_RED
                    } else {
                        Color32::LIGHT_BLUE
                    }),
                );
            }
        }

        (
            Points::new(Values::from_values_iter(
                filtered_peaks_dips
                    .into_iter()
                    .map(|sp| Value::new(sp.wavelength, sp.value)),
            ))
            .name("Peaks")
            .shape(if peaks {
                MarkerShape::Up
            } else {
                MarkerShape::Down
            })
            .color(if peaks {
                Color32::LIGHT_RED
            } else {
                Color32::LIGHT_BLUE
            })
            .filled(true)
            .radius(5.),
            peak_dip_labels,
        )
    }

    pub fn spectrum_channel_to_line(
        &self,
        channel_index: usize,
        config: &SpectrometerConfig,
    ) -> Line {
        Line::new({
            let calibration = &config.spectrum_calibration;
            Values::from_values_iter(self.spectrum.row(channel_index).iter().enumerate().map(
                |(i, p)| {
                    let x = calibration.get_wavelength_from_index(i);
                    let y = *p;
                    Value::new(x, y)
                },
            ))
        })
    }

    pub fn set_calibration(
        &mut self,
        calibration: &mut SpectrumCalibration,
        reference_config: &ReferenceConfig,
    ) {
        calibration.scaling = Some(
            self.spectrum
                .row(3)
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let wavelength = calibration.get_wavelength_from_index(i);
                    let ref_value = reference_config
                        .get_value_at_wavelength(wavelength)
                        .unwrap();
                    ref_value / v
                })
                .collect(),
        );
    }

    pub fn has_zero_reference(&self) -> bool {
        self.zero_reference.is_some()
    }

    pub fn set_zero_reference(&mut self) {
        self.zero_reference = Some(self.spectrum.clone());
    }

    pub fn clear_zero_reference(&mut self) {
        self.zero_reference = None;
    }

    pub fn write_to_csv(
        &self,
        path: &String,
        calibration: &SpectrumCalibration,
    ) -> Result<(), String> {
        let writer = csv::Writer::from_path(path);
        match writer {
            Ok(mut writer) => {
                for p in self.spectrum_to_point_vec(calibration) {
                    writer.serialize(p).unwrap();
                }
                writer.flush().unwrap();
                Ok(())
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn spectrum_to_point_vec(&self, calibration: &SpectrumCalibration) -> Vec<SpectrumExportPoint> {
        self.spectrum
            .column_iter()
            .enumerate()
            .map(|(i, p)| {
                let x = calibration.get_wavelength_from_index(i);
                SpectrumExportPoint {
                    wavelength: x,
                    r: p[0],
                    g: p[1],
                    b: p[2],
                    sum: p[3],
                }
            })
            .collect()
    }
}
