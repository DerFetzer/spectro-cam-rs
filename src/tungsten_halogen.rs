use crate::config::SpectrumPoint;

const T0: f64 = 2.200;
const C: f64 = physical_constants::SPEED_OF_LIGHT_IN_VACUUM;
const H: f64 = physical_constants::PLANCK_CONSTANT;
const K: f64 = physical_constants::BOLTZMANN_CONSTANT;

pub fn reference_from_filament_temp(filament_temp: u16) -> Vec<SpectrumPoint> {
    let mut ref_points = (340..2000)
        .into_iter()
        .map(|wavelength| SpectrumPoint {
            wavelength: wavelength as f32,
            value: spectral_irradiance(wavelength as f64, filament_temp as f64).unwrap() as f32,
        })
        .collect::<Vec<_>>();
    let max = ref_points
        .iter()
        .map(|rp| rp.value)
        .reduce(f32::max)
        .unwrap();
    ref_points.iter_mut().for_each(|rp| rp.value /= max);
    ref_points
}

/// From: <https://doi.org/10.1364/AO.49.000880>
///
fn spectral_irradiance(wavelength: f64, filament_temp: f64) -> Option<f64> {
    let wavelength_m = wavelength * 10.0f64.powi(-9);
    emissivity(wavelength, filament_temp).map(|e| {
        e * 2. * H * C.powi(2)
            / (wavelength_m.powi(5) * (H * C / (wavelength_m * K * filament_temp)).exp_m1())
    })
}

/// From: <https://doi.org/10.1364/AO.23.000975>
///
fn emissivity(wavelength: f64, filament_temp: f64) -> Option<f64> {
    let filament_temp = filament_temp / 1000.;

    let (l0, a0, a1, b0, b1, b2, c0, c1) = match wavelength {
        w if w < 340. => return None,
        w if w < 420. => (0.380, 0.47245, -0.0155, -0.0086, -0.0229, 0., -2.86, 0.),
        w if w < 480. => (0.450, 0.46361, -0.0172, -0.1304, 0., 0., 0.52, 0.),
        w if w < 580. => (0.530, 0.45549, -0.0173, -0.1150, 0., 0., -0.5, 0.),
        w if w < 640. => (0.610, 0.44297, -0.0177, -0.1482, 0., 0., 0.723, 0.),
        w if w < 760. => (
            0.700, 0.43151, -0.0207, -0.1441, -0.0551, 0., -0.278, -0.190,
        ),
        w if w < 940. => (
            0.850, 0.40610, -0.0259, -0.1889, 0.0087, 0.0290, -0.126, 0.246,
        ),
        w if w < 1600. => (1.270, 0.32835, 0., -0.1686, 0.0737, 0., 0.046, 0.016),
        w if w <= 2600. => (2.100, 0.22631, 0.0431, -0.0829, 0.0241, 0., 0.04, -0.026),
        _ => return None,
    };
    Some(
        a0 + a1 * (filament_temp - T0)
            + (b0 + b1 * (filament_temp - T0) + b2 * (filament_temp - T0).powi(2))
                * (wavelength / 1000. - l0)
            + (c0 + c1 * (filament_temp - T0)) * (wavelength / 1000. - l0).powi(2),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tungsten() {
        let r = reference_from_filament_temp(2500);

        assert_eq!(r.iter().map(|rp| rp.value).reduce(f32::max), Some(1.));
        assert_eq!(r.len(), 2000 - 340);
        assert_eq!(r.first().unwrap().wavelength, 340.);
        assert_eq!(r.last().unwrap().wavelength, 2000. - 1.);
    }
}
