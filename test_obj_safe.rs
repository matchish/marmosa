use marmosa::decision_model::DecisionProjection;
use marmosa::domain::{EventRecord, Query};

fn test_dyn<'a, TState>(p: &'a dyn DecisionProjection<State = TState>) {}
