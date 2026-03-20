# marmosa-wasm

WASM bindings for the [Marmosa](https://github.com/matchish/marmosa) event sourcing library. Use Marmosa from Node.js, Deno, Cloudflare Workers, and other JS runtimes.

## Installation

Build from source:

```bash
wasm-pack build marmosa-wasm --target nodejs --release
```

Then reference the generated `marmosa-wasm/pkg` directory from your project:

```json
{
  "dependencies": {
    "marmosa-wasm": "file:../marmosa-wasm/pkg"
  }
}
```

## Storage Backends

Marmosa-wasm ships two storage backends, both implemented entirely in Rust:

| Backend | Constructor | Persistence | Best for |
|---------|-----------|-------------|----------|
| In-memory | `new MarmosaEventStore()` | None (process lifetime) | Tests, prototyping, serverless functions |
| File system | `MarmosaEventStore.withFileSystem(path)` | Disk via `node:fs/promises` | Node.js servers, CLI tools |

You choose the backend once when creating the store. Everything else — appending, reading, projections, decisions — works identically regardless of backend.

## Quick Start

### In-Memory Store

```javascript
const { MarmosaEventStore } = require("marmosa-wasm");

const store = new MarmosaEventStore();

// Append events
await store.append([
  {
    event_id: "evt-1",
    event: {
      event_type: "UserRegistered",
      data: JSON.stringify({ name: "Alice", email: "alice@example.com" }),
      tags: [{ key: "userId", value: "user-1" }],
    },
  },
]);

// Read all events
const events = await store.readAll({ items: [] });
console.log(events);
// [{ position: 0, event_id: "evt-1", event: { ... }, timestamp: 1711... }]
```

### File System Store (Node.js)

```javascript
const { MarmosaEventStore } = require("marmosa-wasm");

const store = MarmosaEventStore.withFileSystem("./data");

await store.append([
  {
    event_id: "evt-1",
    event: {
      event_type: "OrderPlaced",
      data: JSON.stringify({ product: "Widget", qty: 3 }),
      tags: [{ key: "orderId", value: "order-42" }],
    },
  },
]);

// Events survive process restarts — they're stored as JSON files under ./data/Events/
const events = await store.readAll({ items: [] });
```

## API Reference

### Appending Events

```javascript
await store.append(events, condition);
```

- **`events`** — Array of `EventData` objects:
  ```javascript
  {
    event_id: string,       // Unique event identifier
    event: {
      event_type: string,   // Event type name (e.g. "OrderPlaced")
      data: string,         // Stringified JSON payload
      tags: [{ key: string, value: string }]
    },
    metadata?: string | null // Optional stringified JSON metadata
  }
  ```
- **`condition`** — Optional `AppendCondition` for optimistic concurrency:
  ```javascript
  {
    fail_if_events_match: { items: [{ event_types: [...], tags: [...] }] },
    after_sequence_position: 5  // or null
  }
  ```
  The append fails if any event after `after_sequence_position` matches the query.

### Reading Events

```javascript
// Read with filtering, pagination, and start position
const events = await store.read(query, startPosition, maxCount);

// Read all matching events
const all = await store.readAll(query);

// Read the last matching event
const last = await store.readLast(query);
```

**Query examples:**

```javascript
// All events
const allQuery = { items: [] };

// Events of a specific type
const typeQuery = {
  items: [{ event_types: ["OrderPlaced"], tags: [] }],
};

// Events matching a tag
const tagQuery = {
  items: [{ event_types: [], tags: [{ key: "orderId", value: "order-42" }] }],
};

// Multiple criteria (OR across items)
const multiQuery = {
  items: [
    { event_types: ["OrderPlaced"], tags: [] },
    { event_types: ["OrderShipped"], tags: [] },
  ],
};
```

### Decision Model (CQRS Command Handling)

The decision model pattern reads relevant events, folds them into state, and produces an `AppendCondition` for safe writes with optimistic concurrency.

```javascript
const model = await store.buildDecisionModel({
  initialState: { exists: false, enrollmentCount: 0 },
  query: {
    items: [
      {
        event_types: ["CourseCreated", "StudentEnrolled"],
        tags: [{ key: "courseId", value: "course-1" }],
      },
    ],
  },
  apply: (state, event) => {
    switch (event.event.event_type) {
      case "CourseCreated":
        return { ...state, exists: true };
      case "StudentEnrolled":
        return { ...state, enrollmentCount: state.enrollmentCount + 1 };
      default:
        return state;
    }
  },
});

console.log(model.state);
// { exists: true, enrollmentCount: 3 }

// Use the condition when appending to prevent conflicts
await store.append(
  [
    {
      event_id: "evt-new",
      event: {
        event_type: "StudentEnrolled",
        data: "{}",
        tags: [{ key: "courseId", value: "course-1" }],
      },
    },
  ],
  model.appendCondition
);
```

### Execute with Retry

Automatically retries on optimistic concurrency conflicts:

```javascript
await store.executeDecision(3, async () => {
  const model = await store.buildDecisionModel({
    initialState: false,
    query: {
      items: [
        {
          event_types: ["CourseCreated"],
          tags: [{ key: "courseId", value: "course-1" }],
        },
      ],
    },
    apply: (state, event) => true,
  });

  if (model.state) {
    throw new Error("Course already exists");
  }

  await store.append(
    [
      {
        event_id: crypto.randomUUID(),
        event: {
          event_type: "CourseCreated",
          data: JSON.stringify({ name: "Intro to ES" }),
          tags: [{ key: "courseId", value: "course-1" }],
        },
      },
    ],
    model.appendCondition
  );
});
```

### Projections

Projections materialize event streams into queryable read models.

```javascript
const store = new MarmosaEventStore();

// 1. Create a projection store (persists projection state)
const projStore = store.createProjectionStore("OrderSummary");

// 2. Define the projection
const definition = {
  projectionName: "OrderSummary",
  eventTypes: {
    items: [
      { event_types: ["OrderPlaced", "ItemAdded", "OrderShipped"], tags: [] },
    ],
  },
  keySelector: (event) =>
    event.event.tags.find((t) => t.key === "orderId")?.value ?? null,
  apply: (state, event) => {
    switch (event.event.event_type) {
      case "OrderPlaced": {
        const data = JSON.parse(event.event.data);
        return { customer: data.customer, items: 0, shipped: false };
      }
      case "ItemAdded":
        return { ...state, items: (state?.items ?? 0) + 1 };
      case "OrderShipped":
        return { ...state, shipped: true };
      default:
        return state;
    }
  },
};

// 3. Create a runner and rebuild from all events
const runner = store.createProjectionRunner(definition, projStore);
await runner.rebuild(store);

// 4. Query projection state
const orderState = await projStore.get("order-42");
console.log(orderState);
// { customer: "Alice", items: 3, shipped: true }

// 5. Process new events incrementally
await runner.processEvents(newEvents);

// 6. Check progress
const checkpoint = await runner.getCheckpoint();
console.log(checkpoint);
// { projectionName: "OrderSummary", lastPosition: 42, totalEventsProcessed: 15 }
```

### Projection Store Queries

If your projection stores tagged state, you can query by tags:

```javascript
const results = await projStore.queryByTag({
  key: "status",
  value: "active",
});

const filtered = await projStore.queryByTags([
  { key: "status", value: "active" },
  { key: "tier", value: "premium" },
]);
```

## File System Locking

The file system backend uses cross-process file locks for safe concurrent access:

- Lock files are stored at `{basePath}/.locks/{streamId}.lock`
- Locking uses `fs.open(path, 'wx')` (exclusive create) — atomic across processes
- Stale locks from crashed processes are auto-recovered after 30 seconds

This means multiple Node.js workers or processes can safely share the same data directory.

## TypeScript

Copy `marmosa-wasm/ts/marmosa.d.ts` alongside the generated `pkg/` types for richer type definitions covering the `Query`, `EventData`, `ProjectionDefinition`, and other interfaces.

## Building for Different Targets

```bash
# Node.js (CommonJS)
wasm-pack build marmosa-wasm --target nodejs --release

# Bundlers (webpack, vite, etc.)
wasm-pack build marmosa-wasm --target bundler --release

# Web (ESM, no bundler)
wasm-pack build marmosa-wasm --target web --release

# Deno
wasm-pack build marmosa-wasm --target deno --release
```

Note: The file system backend (`withFileSystem`) requires `node:fs/promises` and only works in Node.js/Deno. Use the in-memory backend for browsers and edge runtimes without filesystem access.
