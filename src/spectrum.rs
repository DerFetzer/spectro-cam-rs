use crate::config::{
    Linearize, ReferenceConfig, SpectrometerConfig, SpectrumCalibration, SpectrumPoint,
};
use biquad::{
    Biquad, Coefficients, DirectForm2Transposed, Hertz, ToHertz, Type, Q_BUTTERWORTH_F32,
};
use flume::{Receiver, Sender, TrySendError};
use image::{ImageBuffer, Pixel, Rgb};
use log::trace;
use nalgebra::{Dyn, OMatrix, U3, U4};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub type SpectrumRgb = OMatrix<f32, U3, Dyn>;
pub type Spectrum = OMatrix<f32, U4, Dyn>;

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

    pub fn run(&mut self) {
        while let Ok(window) = self.window_rx.recv() {
            trace!("Got window {}x{}", window.width(), window.height());
            let spectrum = Self::process_window(&window);

            if let Err(TrySendError::Disconnected(_)) = self.spectrum_tx.try_send(spectrum) {
                break;
            }
        }
        log::debug!("SpectrumCalculator thread exiting");
    }

    pub fn process_window(window: &ImageBuffer<Rgb<u8>, Vec<u8>>) -> SpectrumRgb {
        let columns = window.width();
        let rows = window.height();
        let max_value = rows * u8::MAX as u32 * 3;

        window
            .rows()
            //.par_bridge()
            .map(|r| {
                SpectrumRgb::from_vec(
                    r.flat_map(|p| p.channels().iter().map(|&v| v as f32))
                        .collect::<Vec<f32>>(),
                )
            })
            .reduce(|a, b| a + b)
            .map(|s| s / max_value as f32)
            .unwrap_or(SpectrumRgb::from_element(columns as usize, 0.))
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
        while let Ok(spectrum) = self.spectrum_rx.try_recv() {
            trace!(
                "Got spectrum with {} columns and {} rows",
                spectrum.ncols(),
                spectrum.nrows()
            );
            self.update_spectrum(spectrum, config);
        }
    }

    pub fn update_spectrum(&mut self, mut spectrum: SpectrumRgb, config: &SpectrometerConfig) {
        let ncols = spectrum.ncols();

        // Clear buffer and zero reference on dimension change
        if let Some(s) = self.spectrum_buffer.front() {
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
            .iter()
            .cloned()
            .reduce(|a, b| a + b)
            .map(|s| s / self.spectrum_buffer.len() as f32)
            .unwrap_or(SpectrumRgb::from_element(ncols, 0.));

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
                sum / 3.
            } else {
                combined_buffer.row_sum() / 3.
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
                // Filter can make value negative so clamp to zero.
                for sample in channel.iter_mut() {
                    *sample = biquad.run(*sample).max(0.0);
                }
                // Apply filter in reverse to compensate phase error
                for sample in channel.iter_mut().rev() {
                    *sample = biquad.run(*sample).max(0.0);
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
    ) -> Vec<SpectrumPoint> {
        let mut peaks_dips = Vec::new();

        let spectrum: Vec<_> = self.spectrum.row(3).iter().cloned().collect();

        let windows_size = config.view_config.peaks_dips_find_window * 2 + 1;
        let mid_index = (windows_size - 1) / 2;

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
                filtered_peaks_dips.push(*peak_dip);
            }
        }
        filtered_peaks_dips
    }

    pub fn get_spectrum_channel(
        &self,
        channel_index: usize,
        config: &SpectrometerConfig,
    ) -> Vec<SpectrumPoint> {
        let calibration = &config.spectrum_calibration;
        self.spectrum
            .row(channel_index)
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let wavelength = calibration.get_wavelength_from_index(i);
                let value = *p;
                SpectrumPoint { wavelength, value }
            })
            .collect()
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

    pub fn get_spectrum_max_value(&self) -> Option<f32> {
        self.spectrum.iter().cloned().reduce(f32::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::*;

    #[fixture]
    fn spectrum_container() -> SpectrumContainer {
        let (_tx, rx) = flume::unbounded();
        SpectrumContainer::new(rx)
    }

    #[fixture]
    fn config() -> SpectrometerConfig {
        SpectrometerConfig::default()
    }

    #[rstest]
    fn buffer_size(mut spectrum_container: SpectrumContainer, config: SpectrometerConfig) {
        spectrum_container.update_spectrum(SpectrumRgb::from_element(1000, 0.5), &config);
        spectrum_container.update_spectrum(SpectrumRgb::from_element(1000, 0.75), &config);

        assert_eq!(spectrum_container.spectrum_buffer.len(), 2);

        for _ in 0..100 {
            spectrum_container.update_spectrum(SpectrumRgb::from_element(1000, 0.5), &config);
            assert!(
                spectrum_container.spectrum_buffer.len()
                    <= config.postprocessing_config.spectrum_buffer_size
            );
        }

        assert_eq!(
            spectrum_container.spectrum_buffer.len(),
            config.postprocessing_config.spectrum_buffer_size
        );
    }

    #[rstest]
    fn get_spectrum_max_value(
        mut spectrum_container: SpectrumContainer,
        config: SpectrometerConfig,
    ) {
        spectrum_container.update_spectrum(SpectrumRgb::from_element(1000, 0.5), &config);

        assert_eq!(spectrum_container.get_spectrum_max_value(), Some(0.5));
    }
}
