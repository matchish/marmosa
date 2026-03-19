# Bind Order Fix (Porting Note)

Historical note from Opossum migration: startup/service registration ordering can affect runtime
behavior. In Marmosa, keep initialization order explicit in the calling application.

## Rust takeaway

- Initialize storage first.
- Initialize event store and projection services next.
- Start background workers only after stores are ready.
