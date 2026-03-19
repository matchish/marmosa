# Marmosa Use Cases: When a File-System Event Store Fits Best

Marmosa is strongest where offline reliability, local ownership, and operational simplicity matter
more than cloud-scale throughput.

## Good-Fit Scenarios

### 1) On-Prem Transaction Tracking (Retail/Automotive)

- Local append-only history for auditability
- Rebuildable projections for reports and commissions
- Continued operation during network outages

### 2) Factory / OT Environments

- Works under strict “no database on OT network” policies
- Replayable robot/workstation history for diagnostics
- Durable local persistence on edge machines

### 3) Offline-First Desktop/Edge Apps

- Complete local history and deterministic rebuilds
- Eventual sync to central services when connectivity returns

## Where It Is Usually Not the Right Tool

- High-scale multi-region cloud systems
- Heavy multi-writer collaborative workloads
- Architectures requiring distributed ACID across many aggregates

## Decision Framework

Choose Marmosa if most are true:

- You need offline-first behavior.
- You prefer local data sovereignty.
- Operational simplicity is more important than horizontal cloud scale.
- Workload is moderate and bounded on a single node or small deployment.

## Architecture Patterns

### Edge + Cloud Hybrid

- Keep hot/operational data and decision logic locally.
- Export event batches to cloud analytics pipelines.

### Local First + Sync Later

- Accept writes locally at all times.
- Reconcile via append conditions and replay on sync.
