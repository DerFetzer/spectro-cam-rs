use criterion::*;
use image::RgbImage;
use spectro_cam_rs::config::{Linearize, ReferenceConfig, SpectrometerConfig};
use spectro_cam_rs::spectrum::{SpectrumCalculator, SpectrumContainer, SpectrumRgb};
use spectro_cam_rs::tungsten_halogen::reference_from_filament_temp;

fn spectrum_calculator_bench(c: &mut Criterion) {
    let window = RgbImage::new(1000, 20);

    c.bench_with_input(
        BenchmarkId::new("process_window", "window_1000_20"),
        &window,
        |b, w| b.iter(|| SpectrumCalculator::process_window(w)),
    );

    let window = RgbImage::new(1000, 1);
    c.bench_with_input(
        BenchmarkId::new("process_window", "window_1000_1"),
        &window,
        |b, w| b.iter(|| SpectrumCalculator::process_window(w)),
    );
}

fn spectrum_buffer_bench(c: &mut Criterion) {
    let (_tx, rx) = flume::unbounded();
    let (json_tx, _rx) = flume::unbounded();
    let mut sc = SpectrumContainer::new(rx, json_tx);

    c.bench_function("update_spectrum_default", |b| {
        let config = SpectrometerConfig::default();
        b.iter(|| {
            let s = timed(SpectrumRgb::from_element(1000, 0.5));
            sc.update_spectrum(black_box(s), &config);
        });
    });

    c.bench_function("update_spectrum_filter", |b| {
        let mut config = SpectrometerConfig::default();
        config.postprocessing_config.spectrum_filter_active = true;
        b.iter(|| {
            let s = timed(SpectrumRgb::from_element(1000, 0.5));
            sc.update_spectrum(black_box(s), &config);
        });
    });

    c.bench_function("update_spectrum_linearize", |b| {
        let mut config = SpectrometerConfig::default();
        config.spectrum_calibration.linearize = Linearize::Rec601;
        b.iter(|| {
            let s = timed(SpectrumRgb::from_element(1000, 0.5));
            sc.update_spectrum(black_box(s), &config);
        });
    });

    sc.clear_buffer();
    sc.update_spectrum(
        timed(SpectrumRgb::from_fn(1000, |_, j| (j % 20) as f32)),
        &SpectrometerConfig::default(),
    );

    c.bench_function("spectrum_to_peaks", |b| {
        let config = SpectrometerConfig::default();
        b.iter(|| {
            sc.spectrum_to_peaks_and_dips(black_box(true), &config);
        });
    });

    c.bench_function("spectrum_to_dips", |b| {
        let config = SpectrometerConfig::default();
        b.iter(|| {
            sc.spectrum_to_peaks_and_dips(black_box(false), &config);
        });
    });
}

fn config_bench(c: &mut Criterion) {
    let rc = ReferenceConfig {
        reference: Some(reference_from_filament_temp(2500)),
        scale: 1.,
    };

    c.bench_function("get_value_at_wavelength", |b| {
        b.iter(|| {
            rc.get_value_at_wavelength(black_box(851.75));
        });
    });
}

fn timed<T>(data: T) -> spectro_cam_rs::Timestamped<T> {
    let now = jiff::Zoned::now();
    spectro_cam_rs::Timestamped {
        start: now.clone(),
        end: now.clone(),
        data,
    }
}

criterion_group!(
    benches,
    spectrum_calculator_bench,
    spectrum_buffer_bench,
    config_bench
);
criterion_main!(benches);
