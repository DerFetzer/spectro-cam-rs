use flume::{Receiver, Sender};
use image::{ImageBuffer, Pixel, Rgb};
use nalgebra::{Dynamic, OMatrix, U3, U4};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;

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
            if let Ok(window) = self.window_rx.try_recv() {
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
            std::thread::sleep(Duration::from_millis(1))
        }
    }
}
