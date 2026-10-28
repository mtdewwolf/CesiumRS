//! CesiumJS `JobScheduler`: engine for time-slicing jobs during a frame to amortize work.
//!
//! Supports:
//! - Separate budgets for different job types ([`JobType::Texture`], [`JobType::Program`], [`JobType::Buffer`]).
//! - Stealing from other job type budgets if they were not exhausted in the previous frame.
//! - Guaranteed progress on all job types each frame, even if it exceeds the total budget.

use super::job_type::JobType;
use crate::core::clock::get_timestamp;

/// Budget and frame tracking for one job type.
#[derive(Clone, Debug, PartialEq)]
pub struct JobTypeBudget {
    total: f64,
    pub used_this_frame: f64,
    pub stolen_from_me_this_frame: f64,
    pub starved_this_frame: bool,
    pub starved_last_frame: bool,
}

impl JobTypeBudget {
    pub fn new(total: f64) -> Self {
        Self {
            total,
            used_this_frame: 0.0,
            stolen_from_me_this_frame: 0.0,
            starved_this_frame: false,
            starved_last_frame: false,
        }
    }

    /// Total budget in milliseconds allowed for one frame.
    pub fn total(&self) -> f64 {
        self.total
    }
}

/// Slices and amortizes expensive workloads across frames according to per-job budgets.
pub struct JobScheduler {
    budgets: [JobTypeBudget; 3],
    total_budget: f64,
    total_used_this_frame: f64,
    executed_this_frame: [bool; 3],
    timestamp_fn: Option<fn() -> f64>,
}

impl Default for JobScheduler {
    fn default() -> Self {
        Self::new(None)
    }
}

impl JobScheduler {
    pub const DEFAULT_TEXTURE_BUDGET: f64 = 10.0;
    pub const DEFAULT_PROGRAM_BUDGET: f64 = 10.0;
    pub const DEFAULT_BUFFER_BUDGET: f64 = 30.0;

    /// Creates a job scheduler with specified or default budgets for
    /// `[Texture, Program, Buffer]`.
    pub fn new(budgets: Option<[f64; 3]>) -> Self {
        let (tex, prog, buf) = match budgets {
            Some(b) => (b[0], b[1], b[2]),
            None => (
                Self::DEFAULT_TEXTURE_BUDGET,
                Self::DEFAULT_PROGRAM_BUDGET,
                Self::DEFAULT_BUFFER_BUDGET,
            ),
        };

        let budgets = [
            JobTypeBudget::new(tex),
            JobTypeBudget::new(prog),
            JobTypeBudget::new(buf),
        ];

        let total_budget = tex + prog + buf;

        Self {
            budgets,
            total_budget,
            total_used_this_frame: 0.0,
            executed_this_frame: [false; 3],
            timestamp_fn: None,
        }
    }

    /// Allows overriding the clock function (e.g. for testing).
    pub fn set_timestamp_provider(&mut self, provider: Option<fn() -> f64>) {
        self.timestamp_fn = provider;
    }

    fn now(&self) -> f64 {
        if let Some(f) = self.timestamp_fn {
            f()
        } else {
            get_timestamp()
        }
    }

    /// Total frame budget across all job types in milliseconds.
    pub fn total_budget(&self) -> f64 {
        self.total_budget
    }

    /// Total time spent on jobs during the current frame in milliseconds.
    pub fn total_used_this_frame(&self) -> f64 {
        self.total_used_this_frame
    }

    /// Access the per-job budgets.
    pub fn budgets(&self) -> &[JobTypeBudget; 3] {
        &self.budgets
    }

    /// Mutable access to the per-job budgets.
    pub fn budgets_mut(&mut self) -> &mut [JobTypeBudget; 3] {
        &mut self.budgets
    }

    /// Prevents jobs from running this frame.
    pub fn disable_this_frame(&mut self) {
        self.total_used_this_frame = self.total_budget;
    }

    /// Resets frame accounting at the start of a new frame.
    pub fn reset_budgets(&mut self) {
        for budget in &mut self.budgets {
            budget.starved_last_frame = budget.starved_this_frame;
            budget.starved_this_frame = false;
            budget.used_this_frame = 0.0;
            budget.stolen_from_me_this_frame = 0.0;
        }
        self.total_used_this_frame = 0.0;
    }

    /// Executes a job if budget allows, updating starvation and stolen time.
    pub fn execute<F: FnMut()>(&mut self, mut job: F, job_type: JobType) -> bool {
        let idx = job_type.value() as usize;
        if idx >= self.budgets.len() {
            return false;
        }

        let progress_this_frame = self.executed_this_frame[idx];

        if self.total_used_this_frame >= self.total_budget && progress_this_frame {
            self.budgets[idx].starved_this_frame = true;
            return false;
        }

        let mut stolen_idx = None;
        let budget = &self.budgets[idx];
        if budget.used_this_frame + budget.stolen_from_me_this_frame >= budget.total {
            let mut found = None;
            for i in 0..self.budgets.len() {
                let candidate = &self.budgets[i];
                if candidate.used_this_frame + candidate.stolen_from_me_this_frame < candidate.total
                    && !candidate.starved_last_frame
                {
                    found = Some(i);
                    break;
                }
            }

            match found {
                Some(i) => {
                    stolen_idx = Some(i);
                }
                None => {
                    if progress_this_frame {
                        return false;
                    }
                }
            }

            if progress_this_frame {
                self.budgets[idx].starved_this_frame = true;
            }
        }

        let start_time = self.now();
        job();
        let duration = (self.now() - start_time).max(0.0);

        self.total_used_this_frame += duration;

        if let Some(stolen) = stolen_idx {
            self.budgets[stolen].stolen_from_me_this_frame += duration;
        } else {
            self.budgets[idx].used_this_frame += duration;
        }

        self.executed_this_frame[idx] = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        static MOCK_TIME: Cell<f64> = const { Cell::new(0.0) };
    }

    fn mock_time() -> f64 {
        MOCK_TIME.with(|t| t.get())
    }

    fn advance_mock_time(ms: f64) {
        MOCK_TIME.with(|t| t.set(t.get() + ms));
    }

    fn reset_mock_time() {
        MOCK_TIME.with(|t| t.set(0.0));
    }

    #[test]
    fn default_budgets_match_upstream() {
        let scheduler = JobScheduler::default();
        assert_eq!(scheduler.total_budget(), 50.0);
        assert_eq!(scheduler.budgets()[0].total(), 10.0);
        assert_eq!(scheduler.budgets()[1].total(), 10.0);
        assert_eq!(scheduler.budgets()[2].total(), 30.0);
        assert_eq!(scheduler.total_used_this_frame(), 0.0);
    }

    #[test]
    fn custom_budgets_initialize_correctly() {
        let scheduler = JobScheduler::new(Some([5.0, 15.0, 20.0]));
        assert_eq!(scheduler.total_budget(), 40.0);
        assert_eq!(scheduler.budgets()[0].total(), 5.0);
        assert_eq!(scheduler.budgets()[1].total(), 15.0);
        assert_eq!(scheduler.budgets()[2].total(), 20.0);
    }

    #[test]
    fn disable_this_frame_sets_total_used() {
        let mut scheduler = JobScheduler::default();
        scheduler.disable_this_frame();
        assert_eq!(scheduler.total_used_this_frame(), scheduler.total_budget());
    }

    #[test]
    fn execute_tracks_time_and_starvation() {
        reset_mock_time();
        let mut scheduler = JobScheduler::new(Some([10.0, 10.0, 10.0]));
        scheduler.set_timestamp_provider(Some(mock_time));

        let mut ran = false;
        let executed = scheduler.execute(
            || {
                ran = true;
                advance_mock_time(5.0);
            },
            JobType::Texture,
        );
        assert!(executed);
        assert!(ran);
        assert_eq!(scheduler.budgets()[0].used_this_frame, 5.0);
        assert_eq!(scheduler.total_used_this_frame(), 5.0);

        // Run again with 6ms duration -> exceeds texture budget (5 + 6 = 11 > 10)
        // A subsequent job steals from Program (first available budget)
        let executed = scheduler.execute(
            || {
                advance_mock_time(6.0);
            },
            JobType::Texture,
        );
        assert!(executed);
        assert!(scheduler.execute(|| advance_mock_time(6.0), JobType::Texture));
        assert_eq!(scheduler.budgets()[1].stolen_from_me_this_frame, 6.0);
        assert_eq!(scheduler.budgets()[0].starved_this_frame, true);

        // Reset budgets for next frame
        scheduler.reset_budgets();
        assert_eq!(scheduler.budgets()[0].starved_last_frame, true);
        assert_eq!(scheduler.budgets()[0].starved_this_frame, false);
        assert_eq!(scheduler.budgets()[0].used_this_frame, 0.0);
        assert_eq!(scheduler.budgets()[1].stolen_from_me_this_frame, 0.0);
        assert_eq!(scheduler.total_used_this_frame(), 0.0);
    }
}
