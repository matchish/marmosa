# PostgreSQL Projection Store

## Status: Not Started

## Description
Implement `ProjectionStore` for PostgreSQL, allowing projections to be stored in a relational database instead of the file-based storage backend.

## Acceptance Criteria
- [ ] `PostgresProjectionStore<TState>` implementing `ProjectionStore<TState>`
- [ ] Uses `sqlx` or `tokio-postgres` for async queries
- [ ] Automatic table creation/migration
- [ ] JSON serialization of state in a `jsonb` column
- [ ] Efficient `get_all` with pagination support
- [ ] Checkpoint storage in same database
- [ ] Connection pooling support
- [ ] Gated behind `#[cfg(feature = "postgres")]`

## Schema Design
```sql
CREATE TABLE IF NOT EXISTS {projection_name} (
    key VARCHAR(255) PRIMARY KEY,
    state JSONB NOT NULL,
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS projection_checkpoints (
    projection_name VARCHAR(255) PRIMARY KEY,
    last_position BIGINT NOT NULL,
    updated_at TIMESTAMPTZ DEFAULT NOW()
);
```

## API Design
```rust
#[cfg(feature = "postgres")]
pub struct PostgresProjectionStore<TState> {
    pool: sqlx::PgPool,
    table_name: String,
    _marker: PhantomData<TState>,
}

#[cfg(feature = "postgres")]
impl<TState> PostgresProjectionStore<TState> {
    pub async fn new(pool: sqlx::PgPool, projection_name: &str) -> Result<Self, Error>;
    
    /// Run migrations to create tables
    pub async fn migrate(&self) -> Result<(), Error>;
}

#[cfg(feature = "postgres")]
impl<TState: Serialize + DeserializeOwned + Send + Sync> ProjectionStore<TState> 
    for PostgresProjectionStore<TState> 
{
    async fn get(&self, key: &str) -> Result<Option<TState>, Error>;
    async fn get_all(&self) -> Result<Vec<TState>, Error>;
    async fn save(&self, key: &str, state: &TState) -> Result<(), Error>;
    async fn delete(&self, key: &str) -> Result<(), Error>;
}
```

## Cargo.toml
```toml
[features]
postgres = ["sqlx/postgres", "sqlx/runtime-tokio-rustls"]

[dependencies.sqlx]
version = "0.7"
optional = true
default-features = false
features = ["postgres", "json"]
```

## Usage Example
```rust
let pool = PgPool::connect("postgres://localhost/mydb").await?;
let store = PostgresProjectionStore::<CourseInfo>::new(pool, "course_info").await?;
store.migrate().await?;

let runner = ProjectionRunner::new(event_storage, CourseProjection, store);
runner.process_events(&events).await?;

// Query directly with SQL if needed
let courses: Vec<CourseInfo> = sqlx::query_as("SELECT state FROM course_info WHERE state->>'status' = 'active'")
    .fetch_all(&pool)
    .await?;
```

## Benefits over File-Based
- SQL queries on projection state (WHERE, ORDER BY, etc.)
- ACID transactions
- Better concurrency handling
- Easier backup/restore
- Integration with existing Postgres tooling

## Considerations
- Requires `std` (not `no_std` compatible)
- Network latency vs local file I/O
- Connection pool sizing
- Migration strategy for schema changes

## Dependencies
- ProjectionStore trait (done)
- `sqlx` crate with postgres feature
