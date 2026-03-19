# Configuration Guide (Marmosa)

Marmosa configuration is code-first via Rust types/builders.

## Recommended baseline

- Set stable root/storage paths per environment.
- Keep production durability stricter than test/dev.
- Define projection names and tag conventions centrally.

## Environment separation

- Local development: fast iteration, relaxed durability acceptable.
- CI: deterministic paths and strict validation.
- Production: durable writes, explicit backup strategy.
