//! CesiumJS `EarthOrientationParameters` and `EarthOrientationParametersSample`:
//! polar motion, celestial pole offsets, and UT1 − UTC, interpolated from
//! tabulated IERS data.
//!
//! Cesium downloads the table with `fromUrl`; here the caller supplies the
//! JSON text ([`EarthOrientationParameters::from_json`]) or parsed columns.

use std::cell::Cell;

use super::binary_search::binary_search;
use super::julian_date::JulianDate;
use super::leap_second::LeapSecond;
use super::runtime_error::RuntimeError;
use super::time_constants::MODIFIED_JULIAN_DATE_DIFFERENCE;
use super::time_standard::TimeStandard;

/// One evaluation of the Earth orientation parameters, in radians and seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EarthOrientationParametersSample {
    pub x_pole_wander: f64,
    pub y_pole_wander: f64,
    pub x_pole_offset: f64,
    pub y_pole_offset: f64,
    pub ut1_minus_utc: f64,
}

/// The table: column names and the samples, row by row. Cells that are not
/// numbers (such as the `dateIso8601` column) are stored as NaN.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EopData {
    pub column_names: Vec<String>,
    pub samples: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct EarthOrientationParameters {
    dates: Vec<JulianDate>,
    samples: Vec<f64>,
    x_pole_wander_radians_column: usize,
    y_pole_wander_radians_column: usize,
    ut1_minus_utc_seconds_column: usize,
    x_celestial_pole_offset_radians_column: usize,
    y_celestial_pole_offset_radians_column: usize,
    tai_minus_utc_seconds_column: usize,
    column_count: usize,
    last_index: Cell<Option<usize>>,
}

const COLUMN_ERROR: &str = "Error in loaded EOP data: The columnNames property must include modifiedJulianDateUtc, xPoleWanderRadians, yPoleWanderRadians, ut1MinusUtcSeconds, xCelestialPoleOffsetRadians, yCelestialPoleOffsetRadians, and taiMinusUtcSeconds columns";

impl EarthOrientationParameters {
    /// Cesium's `EarthOrientationParameters.NONE`: every parameter is zero.
    pub fn none() -> Self {
        let columns = [
            "dateIso8601",
            "modifiedJulianDateUtc",
            "xPoleWanderRadians",
            "yPoleWanderRadians",
            "ut1MinusUtcSeconds",
            "lengthOfDayCorrectionSeconds",
            "xCelestialPoleOffsetRadians",
            "yCelestialPoleOffsetRadians",
            "taiMinusUtcSeconds",
        ];
        let data = EopData { column_names: columns.iter().map(|c| c.to_string()).collect(), samples: Vec::new() };
        Self::new(&data, false).expect("the built-in columns are complete")
    }

    /// With `add_new_leap_seconds`, a change in TAI − UTC between rows adds
    /// that leap second to [`JulianDate`]'s table if it is not there yet.
    pub fn new(data: &EopData, add_new_leap_seconds: bool) -> Result<Self, RuntimeError> {
        let column = |name: &str| data.column_names.iter().position(|c| c == name);
        let (
            Some(date_column),
            Some(x_pole_wander_radians_column),
            Some(y_pole_wander_radians_column),
            Some(ut1_minus_utc_seconds_column),
            Some(x_celestial_pole_offset_radians_column),
            Some(y_celestial_pole_offset_radians_column),
            Some(tai_minus_utc_seconds_column),
        ) = (
            column("modifiedJulianDateUtc"),
            column("xPoleWanderRadians"),
            column("yPoleWanderRadians"),
            column("ut1MinusUtcSeconds"),
            column("xCelestialPoleOffsetRadians"),
            column("yCelestialPoleOffsetRadians"),
            column("taiMinusUtcSeconds"),
        )
        else {
            return Err(RuntimeError::new(COLUMN_ERROR));
        };

        let column_count = data.column_names.len();
        let samples = data.samples.clone();
        let mut dates = Vec::new();
        let mut last_tai_minus_utc: Option<f64> = None;
        let mut i = 0;
        while i < samples.len() {
            let mjd = samples.get(i + date_column).copied().unwrap_or(f64::NAN);
            let tai_minus_utc = samples.get(i + tai_minus_utc_seconds_column).copied().unwrap_or(f64::NAN);
            let day = mjd + MODIFIED_JULIAN_DATE_DIFFERENCE;
            let date = JulianDate::new(day, tai_minus_utc, TimeStandard::Tai);
            dates.push(date);
            if add_new_leap_seconds {
                if let Some(last) = last_tai_minus_utc {
                    if tai_minus_utc != last {
                        JulianDate::add_leap_second(LeapSecond { julian_date: date, offset: tai_minus_utc });
                    }
                }
                last_tai_minus_utc = Some(tai_minus_utc);
            }
            i += column_count;
        }

        Ok(Self {
            dates,
            samples,
            x_pole_wander_radians_column,
            y_pole_wander_radians_column,
            ut1_minus_utc_seconds_column,
            x_celestial_pole_offset_radians_column,
            y_celestial_pole_offset_radians_column,
            tai_minus_utc_seconds_column,
            column_count,
            last_index: Cell::new(None),
        })
    }

    /// Parses IERS data in Cesium's JSON layout (`columnNames`, `samples`).
    pub fn from_json(json: &str, add_new_leap_seconds: bool) -> Result<Self, RuntimeError> {
        let value: serde_json::Value = serde_json::from_str(json)
            .map_err(|e| RuntimeError::new(format!("An error occurred while parsing the EOP data: {e}")))?;
        let Some(column_names) = value.get("columnNames").and_then(|c| c.as_array()) else {
            return Err(RuntimeError::new("Error in loaded EOP data: The columnNames property is required."));
        };
        let Some(samples) = value.get("samples").and_then(|s| s.as_array()) else {
            return Err(RuntimeError::new("Error in loaded EOP data: The samples property is required."));
        };
        let data = EopData {
            column_names: column_names.iter().map(|c| c.as_str().unwrap_or_default().to_string()).collect(),
            samples: samples.iter().map(|s| s.as_f64().unwrap_or(f64::NAN)).collect(),
        };
        Self::new(&data, add_new_leap_seconds)
    }

    /// The parameters at `date` (TAI), linearly interpolated. Dates past the
    /// end of the table give zeros; dates before it use the first row.
    pub fn compute(&self, date: &JulianDate) -> EarthOrientationParametersSample {
        let dates = &self.dates;
        if self.samples.is_empty() {
            return EarthOrientationParametersSample::default();
        }

        // Most calls step forward a little from the previous one.
        if let Some(last_index) = self.last_index.get() {
            let previous_index_date = &dates[last_index];
            let next_index_date = dates.get(last_index + 1);
            let is_after_previous = JulianDate::less_than_or_equals(previous_index_date, date);
            let is_after_last_sample = next_index_date.is_none();
            let is_before_next = next_index_date.is_none_or(|n| JulianDate::greater_than_or_equals(n, date));
            if is_after_previous && is_before_next {
                let mut before = last_index;
                if !is_after_last_sample && next_index_date.is_some_and(|n| n.equals(date)) {
                    before += 1;
                }
                return self.interpolate(date, before, before + 1);
            }
        }

        let index = binary_search(dates, date, JulianDate::compare);
        let (before, after);
        if index >= 0 {
            // Of two rows at the same moment (either side of a leap second), use the later.
            let mut index = index as usize;
            if index < dates.len() - 1 && dates[index + 1].equals(date) {
                index += 1;
            }
            before = index;
            after = index;
        } else {
            after = (!index) as usize;
            before = after.saturating_sub(1);
        }
        self.last_index.set(Some(before));
        self.interpolate(date, before, after)
    }

    fn fill_from_index(&self, index: usize) -> EarthOrientationParametersSample {
        let start = index * self.column_count;
        let s = |column: usize| self.samples[start + column];
        EarthOrientationParametersSample {
            x_pole_wander: s(self.x_pole_wander_radians_column),
            y_pole_wander: s(self.y_pole_wander_radians_column),
            x_pole_offset: s(self.x_celestial_pole_offset_radians_column),
            y_pole_offset: s(self.y_celestial_pole_offset_radians_column),
            ut1_minus_utc: s(self.ut1_minus_utc_seconds_column),
        }
    }

    fn interpolate(&self, date: &JulianDate, before: usize, after: usize) -> EarthOrientationParametersSample {
        let dates = &self.dates;
        if after > dates.len() - 1 {
            return EarthOrientationParametersSample::default();
        }
        let before_date = &dates[before];
        let after_date = &dates[after];
        if before_date.equals(after_date) || date.equals(before_date) {
            return self.fill_from_index(before);
        } else if date.equals(after_date) {
            return self.fill_from_index(after);
        }

        let factor =
            JulianDate::seconds_difference(date, before_date) / JulianDate::seconds_difference(after_date, before_date);
        let start_before = before * self.column_count;
        let start_after = after * self.column_count;
        let samples = &self.samples;

        // UT1 − UTC jumps by a second at a leap second; take it out before interpolating.
        let mut before_ut1_minus_utc = samples[start_before + self.ut1_minus_utc_seconds_column];
        let mut after_ut1_minus_utc = samples[start_after + self.ut1_minus_utc_seconds_column];
        let offset_difference = after_ut1_minus_utc - before_ut1_minus_utc;
        // Not a range check: NaN must skip this, as in Cesium.
        #[allow(clippy::manual_range_contains)]
        if offset_difference > 0.5 || offset_difference < -0.5 {
            let before_tai_minus_utc = samples[start_before + self.tai_minus_utc_seconds_column];
            let after_tai_minus_utc = samples[start_after + self.tai_minus_utc_seconds_column];
            if before_tai_minus_utc != after_tai_minus_utc {
                if after_date.equals(date) {
                    before_ut1_minus_utc = after_ut1_minus_utc;
                } else {
                    after_ut1_minus_utc -= after_tai_minus_utc - before_tai_minus_utc;
                }
            }
        }

        let lerp = |column: usize| {
            let y1 = samples[start_before + column];
            let y2 = samples[start_after + column];
            y1 + factor * (y2 - y1)
        };
        EarthOrientationParametersSample {
            x_pole_wander: lerp(self.x_pole_wander_radians_column),
            y_pole_wander: lerp(self.y_pole_wander_radians_column),
            x_pole_offset: lerp(self.x_celestial_pole_offset_radians_column),
            y_pole_offset: lerp(self.y_celestial_pole_offset_radians_column),
            ut1_minus_utc: before_ut1_minus_utc + factor * (after_ut1_minus_utc - before_ut1_minus_utc),
        }
    }
}

impl Default for EarthOrientationParameters {
    fn default() -> Self {
        Self::none()
    }
}
