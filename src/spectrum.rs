use flume::{Receiver, Sender};
use image::{ImageBuffer, Rgb};
use nalgebra::{Dynamic, OMatrix, RowDVector, U1};
use rayon::prelude::*;
use std::time::Duration;

pub type Spectrum = RowDVector<f32>;

pub struct SpectrumCalculator {
    window_rx: Receiver<ImageBuffer<Rgb<u8>, Vec<u8>>>,
    spectrum_tx: Sender<Spectrum>,
}

impl SpectrumCalculator {
    pub fn new(
        window_rx: Receiver<ImageBuffer<Rgb<u8>, Vec<u8>>>,
        spectrum_tx: Sender<Spectrum>,
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
                let max_value = columns * u8::MAX as u32;
                let spectrum: OMatrix<f32, U1, Dynamic> = window
                    .rows()
                    .par_bridge()
                    .map(|r| {
                        OMatrix::<f32, U1, Dynamic>::from_vec(
                            {
                                r.par_bridge()
                                    .map(|p| p.0.into_iter().map(|sp| sp as f32).sum())
                            }
                            .collect::<Vec<f32>>(),
                        )
                    })
                    .reduce(
                        || OMatrix::<f32, U1, Dynamic>::from_element(columns as usize, 0.),
                        |a, b| a + b,
                    )
                    / max_value as f32;
                self.spectrum_tx.send(spectrum).unwrap();
            }
            std::thread::sleep(Duration::from_millis(1))
        }
    }
}
