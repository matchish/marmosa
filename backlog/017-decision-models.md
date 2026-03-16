# CQRS Decision Models

## Status: Not Started

## Description
Based on `DecisionModel/BuildDecisionModelIntegrationTests.cs`, `DecisionProjectionTests.cs`, and `ComposeProjectionsTests.cs`.
Opossum exposes sophisticated CQRS structures allowing domain commands to build ephemeral state models to make complex decisions before generating new events.

## Acceptance Criteria
- [ ] Implement `DecisionModel<TState>` framework that applies local rules against a local state builder before calling `EventStore`.
- [ ] Implement command execution hooks (`execute_decision_async`).
- [ ] Implement combinatorial or composed projections (a projection built dynamically from multiple streams).

## API Design
```rust
pub trait CommandHandler {
    type Command;
    type Error;
    async fn handle(&self, command: Self::Command, store: &dyn EventStore) -> Result<(), Self::Error>;
}
// Using generic states to model decision constraints inline.
```