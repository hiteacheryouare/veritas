//! Balance projections, what-if scenarios, budget suggestions, goal ETAs and Wrapped aggregates (docs/finance/forecasting.md).
//!
//! Everything here is a pure function of a borrowed [`BudgetData`], a
//! "today" and a list of [`KnownFlow`]s. It reads `veritas_reports::classify`
//! for what every row means and never writes anything.
//!
//! [`BudgetData`]: veritas_domain::BudgetData

mod band;
mod baseline;
mod flows;
mod goal;
mod history;
mod math;
mod projection;
mod scenario;
mod suggest;
mod wrapped;

pub use band::{BAND_MIN_RESIDUALS, Band, BandMethod};
pub use baseline::{Baseline, INDEX_ACTIVE_MIN, INDEX_MAX, INDEX_MIN, SEASON_MIN, WINDOW};
pub use flows::{Certainty, FlowKind, KnownFlow, LoanPayment, ScheduleAmount};
pub use goal::{EtaStatus, GoalEta, goal_eta, month_name};
pub use history::Side;
pub use projection::{
	Comparison, Forecaster, HORIZONS, HorizonForecast, MonthForecast, Projection,
};
pub use scenario::{Cadence, IncomeDelta, MAX_COMPARED, Scenario, ScenarioEvent};
pub use suggest::{Basis, Envelope, Suggestion, Suggestions};
pub use wrapped::{
	CategoryRank, Change, GroupRank, NoSpend, PayeeRank, PriorYear, Redacted, Streak, Wrapped,
	wrapped,
};
