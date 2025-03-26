# spectro-cam-rs

A cross-platform GUI for webcam-based spectrometers.

It should be a replacement for the Windows-only [Theremino Spectrometer][theremino] GUI since I only use Linux.

I use it with my [i-PhosHD][iphos] low-budget spectrometer.

![Screenshot](res/screenshot.png)

# Features

  - Adjustable webcam picture window size
  - Wavelength calibration
  - Per channel gain with presets
  - Linearization
  - Camera controls
  - Postprocessing (averaging buffer, low-pass filter, extraction of peaks and dips)
  - Absorption spectrography via zero reference
  - Calibration with imported reference or generated tungsten spectrum
  - Spectrum export
  - Multi-core support
  - Dark theme

## Calibration with imported reference or generated tungsten spectrum

This feature allows using a known spectrum to calibrate the output of your spectrometer.
This could correct for errors in your spectrometer, and make it output more accurate measurements.

First, a reference spectrum is loaded. The reference can be loaded from a CSV file, but spectro-cam
can also generate a tungsten filament spectrum directly in the program. The reference represents
what the spectrometer *should* output when pointed at a light source known to have this spectrum.

With the reference loaded, point the spectrometer at a light source with the known reference
spectrum. Let the readings stabilize and then click "Set Reference as Calibration".

This computes the error between the measured spectrum and the reference spectrum.
This yields an error correcting scaling factor for each wavelength that is applied to future
readings until "Delete Calibration" is clicked.

# Limitations

  - Camera controls not tested on Windows and Mac
  - Not tested on Mac
  - Missing documentation

# License

This program is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)

- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

## Code of Conduct

Contribution to this crate is organized under the terms of the [Rust Code of
Conduct][CoC], the maintainer of this crate, [DerFetzer][team], promises
to intervene to uphold that code of conduct.

[CoC]: https://www.rust-lang.org/policies/code-of-conduct
[team]: https://github.com/DerFetzer
[theremino]: https://physicsopenlab.org/2015/11/26/webcam-diffraction-grating-spectrometer/
[iphos]: https://chriswesley.org/spectrometer.htm
