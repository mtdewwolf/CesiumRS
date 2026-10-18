//! CesiumJS `Iau2006XysData` and `Iau2006XysSample`: the IAU 2006 X, Y and s
//! values (CIP coordinates and CIO locator), interpolated from daily samples.
//!
//! Cesium downloads the samples in chunks of 1,000 as they are needed. Here
//! the data starts empty; [`Iau2006XysData::compute_xys_radians`] and
//! [`Iau2006XysData::preload`] record the chunks they need, the caller fetches
//! them ([`Iau2006XysData::chunk_url`]) and hands them over with
//! [`Iau2006XysData::add_chunk_json`]. With the `bundled-xys` feature,
//! [`Iau2006XysData::bundled`] has every chunk built in.

use std::cell::RefCell;
use std::collections::BTreeSet;

use super::julian_date::JulianDate;
use super::math::to_int32;
use super::runtime_error::RuntimeError;
use super::time_standard::TimeStandard;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Iau2006XysSample {
    pub x: f64,
    pub y: f64,
    pub s: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Iau2006XysDataOptions {
    /// URL with `{0}` where the chunk index goes. Defaults to Cesium's
    /// `Assets/IAU2006_XYS/IAU2006_XYS_{0}.json`.
    pub xys_file_url_template: Option<String>,
    pub interpolation_order: usize,
    pub sample_zero_julian_ephemeris_date: f64,
    pub step_size_days: f64,
    pub samples_per_xys_file: usize,
    pub total_samples: usize,
}

impl Default for Iau2006XysDataOptions {
    fn default() -> Self {
        Self {
            xys_file_url_template: None,
            interpolation_order: 9,
            sample_zero_julian_ephemeris_date: 2442396.5,
            step_size_days: 1.0,
            samples_per_xys_file: 1000,
            total_samples: 27426,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Iau2006XysData {
    xys_file_url_template: Option<String>,
    interpolation_order: usize,
    sample_zero_date_tt: JulianDate,
    step_size_days: f64,
    samples_per_xys_file: usize,
    total_samples: usize,
    /// Three values (x, y, s) per sample; NaN until its chunk is loaded.
    samples: Vec<f64>,
    denominators: Vec<f64>,
    x_table: Vec<f64>,
    requested: RefCell<BTreeSet<usize>>,
}

impl Default for Iau2006XysData {
    fn default() -> Self {
        Self::new(Iau2006XysDataOptions::default())
    }
}

impl Iau2006XysData {
    pub fn new(options: Iau2006XysDataOptions) -> Self {
        let order = options.interpolation_order;
        let step = options.step_size_days;
        let step_n = step.powi(order as i32);
        let mut denominators = vec![0.0; order + 1];
        let mut x_table = vec![0.0; order + 1];
        for i in 0..=order {
            denominators[i] = step_n;
            x_table[i] = i as f64 * step;
            for j in 0..=order {
                if j != i {
                    denominators[i] *= i as f64 - j as f64;
                }
            }
            denominators[i] = 1.0 / denominators[i];
        }
        Self {
            xys_file_url_template: options.xys_file_url_template,
            interpolation_order: order,
            sample_zero_date_tt: JulianDate::new(options.sample_zero_julian_ephemeris_date, 0.0, TimeStandard::Tai),
            step_size_days: step,
            samples_per_xys_file: options.samples_per_xys_file,
            total_samples: options.total_samples,
            samples: vec![f64::NAN; options.total_samples * 3],
            denominators,
            x_table,
            requested: RefCell::new(BTreeSet::new()),
        }
    }

    /// Every chunk of Cesium's data, compiled in (about 1.8 MB of JSON).
    #[cfg(feature = "bundled-xys")]
    pub fn bundled() -> Self {
        let mut data = Self::default();
        for (index, json) in bundled::CHUNKS.iter().enumerate() {
            data.add_chunk_json(index, json).expect("bundled XYS data is valid");
        }
        data
    }

    fn days_since_epoch(&self, day_tt: f64, second_tt: f64) -> f64 {
        // Cesium writes the fields directly, so seconds past a day are fine.
        let date_tt = JulianDate { day_number: day_tt, seconds_of_day: second_tt };
        JulianDate::days_difference(&date_tt, &self.sample_zero_date_tt)
    }

    /// The chunks needed to evaluate from the start to the stop time (TT),
    /// which are also recorded as requested.
    pub fn preload(
        &self,
        start_day_tt: f64,
        start_second_tt: f64,
        stop_day_tt: f64,
        stop_second_tt: f64,
    ) -> Vec<usize> {
        let start_days_since_epoch = self.days_since_epoch(start_day_tt, start_second_tt);
        let stop_days_since_epoch = self.days_since_epoch(stop_day_tt, stop_second_tt);
        let half_order = self.interpolation_order as f64 / 2.0;

        let start_index = to_int32(start_days_since_epoch / self.step_size_days - half_order).max(0);
        // Cesium writes `(… - order / 2) | (0 + order)`: a bitwise OR, kept as is.
        let mut stop_index =
            to_int32(stop_days_since_epoch / self.step_size_days - half_order) | self.interpolation_order as i32;
        if stop_index >= self.total_samples as i32 {
            stop_index = self.total_samples as i32 - 1;
        }

        let per_file = self.samples_per_xys_file as i32;
        let start_chunk = start_index / per_file;
        let stop_chunk = stop_index / per_file;
        let chunks: Vec<usize> = (start_chunk..=stop_chunk).filter(|&c| c >= 0).map(|c| c as usize).collect();
        for &chunk in &chunks {
            self.request(chunk);
        }
        chunks
    }

    fn request(&self, chunk: usize) {
        if !self.is_chunk_loaded(chunk) {
            self.requested.borrow_mut().insert(chunk);
        }
    }

    /// Chunks requested but not loaded yet, oldest index first. The list is cleared.
    pub fn take_requested_chunks(&self) -> Vec<usize> {
        std::mem::take(&mut *self.requested.borrow_mut()).into_iter().collect()
    }

    /// Where Cesium would fetch a chunk from.
    pub fn chunk_url(&self, chunk_index: usize) -> String {
        match &self.xys_file_url_template {
            Some(template) => template.replace("{0}", &chunk_index.to_string()),
            None => format!("Assets/IAU2006_XYS/IAU2006_XYS_{chunk_index}.json"),
        }
    }

    pub fn is_chunk_loaded(&self, chunk_index: usize) -> bool {
        let start = chunk_index * self.samples_per_xys_file * 3;
        self.samples.get(start).is_some_and(|v| !v.is_nan())
    }

    /// Stores a chunk's `samples` (x, y, s per day).
    pub fn add_chunk(&mut self, chunk_index: usize, samples: &[f64]) {
        let start = chunk_index * self.samples_per_xys_file * 3;
        for (i, &v) in samples.iter().enumerate() {
            if let Some(slot) = self.samples.get_mut(start + i) {
                *slot = v;
            }
        }
        self.requested.borrow_mut().remove(&chunk_index);
    }

    /// Stores a chunk file in Cesium's JSON layout.
    pub fn add_chunk_json(&mut self, chunk_index: usize, json: &str) -> Result<(), RuntimeError> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| RuntimeError::new(format!("Invalid XYS chunk: {e}")))?;
        let samples = value
            .get("samples")
            .and_then(|s| s.as_array())
            .ok_or_else(|| RuntimeError::new("Invalid XYS chunk: samples are required."))?;
        let samples: Vec<f64> = samples.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect();
        self.add_chunk(chunk_index, &samples);
        Ok(())
    }

    /// X, Y and s in radians at a TT date, by Lagrange interpolation. `None`
    /// outside the data, or while a needed chunk is missing (it is then
    /// recorded as requested).
    pub fn compute_xys_radians(&self, day_tt: f64, second_tt: f64) -> Option<Iau2006XysSample> {
        let days_since_epoch = self.days_since_epoch(day_tt, second_tt);
        if days_since_epoch < 0.0 {
            return None;
        }
        let center_index = to_int32(days_since_epoch / self.step_size_days);
        if center_index >= self.total_samples as i32 {
            return None;
        }

        let degree = self.interpolation_order as i32;
        let total = self.total_samples as i32;
        let mut first_index = (center_index - degree / 2).max(0);
        let mut last_index = first_index + degree;
        if last_index >= total {
            last_index = total - 1;
            first_index = (last_index - degree).max(0);
        }

        let per_file = self.samples_per_xys_file;
        let samples = &self.samples;
        let (first, last) = (first_index as usize, last_index as usize);
        let mut is_data_missing = false;
        if samples[first * 3].is_nan() {
            self.request(first / per_file);
            is_data_missing = true;
        }
        if samples[last * 3].is_nan() {
            self.request(last / per_file);
            is_data_missing = true;
        }
        if is_data_missing {
            return None;
        }

        let x = days_since_epoch - first_index as f64 * self.step_size_days;
        let degree = self.interpolation_order;
        let work: Vec<f64> = self.x_table.iter().map(|t| x - t).collect();
        let mut result = Iau2006XysSample::default();
        for i in 0..=degree {
            let mut coef = 1.0;
            for (j, w) in work.iter().enumerate() {
                if j != i {
                    coef *= w;
                }
            }
            coef *= self.denominators[i];
            let sample_index = (first + i) * 3;
            result.x += coef * samples[sample_index];
            result.y += coef * samples[sample_index + 1];
            result.s += coef * samples[sample_index + 2];
        }
        Some(result)
    }
}

#[cfg(feature = "bundled-xys")]
mod bundled {
    macro_rules! chunks {
        ($($i:literal)*) => {
            [$(include_str!(concat!("../../assets/IAU2006_XYS/IAU2006_XYS_", $i, ".json"))),*]
        };
    }
    pub(super) const CHUNKS: [&str; 28] =
        chunks!(0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27);
}
