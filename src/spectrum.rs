use flume::{Receiver, Sender};
use image::{ImageBuffer, Pixel, Rgb};
use nalgebra::{Dynamic, OMatrix, U4};
use rayon::prelude::*;
use std::time::Duration;

pub type Spectrum = OMatrix<f32, U4, Dynamic>;

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
                let rows = window.height();
                let max_value = rows * u8::MAX as u32 * 3;

                let spectrum: Spectrum = window
                    .rows()
                    .par_bridge()
                    .map(|r| {
                        Spectrum::from_vec(
                            r.flat_map(|p| {
                                let pv =
                                    p.channels().iter().map(|&v| v as f32).collect::<Vec<f32>>();
                                [pv.as_slice(), &[pv.iter().sum()]].concat()
                            })
                            .collect::<Vec<f32>>(),
                        )
                    })
                    .reduce(
                        || Spectrum::from_element(columns as usize, 0.),
                        |a, b| a + b,
                    )
                    / max_value as f32;

                self.spectrum_tx.send(spectrum).unwrap();
            }
            std::thread::sleep(Duration::from_millis(1))
        }
    }
}
