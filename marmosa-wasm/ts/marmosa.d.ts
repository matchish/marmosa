export interface Tag {
  key: string;
  value: string;
}

export interface DomainEvent {
  event_type: string;
  data: string;
  tags: Tag[];
}

export interface EventData {
  event_id: string;
  event: DomainEvent;
  metadata?: string | null;
}

export interface EventRecord {
  position: number;
  event_id: string;
  event: DomainEvent;
  metadata?: string | null;
  timestamp: number;
}

export interface QueryItem {
  event_types: string[];
  tags: Tag[];
}

export interface Query {
  items: QueryItem[];
}

export interface AppendCondition {
  fail_if_events_match: Query;
  after_sequence_position?: number | null;
}

export interface DecisionProjection<T> {
  initialState: T;
  query: Query;
  apply(state: T, event: EventRecord): T;
}

export interface DecisionModel<T> {
  state: T;
  appendCondition: AppendCondition;
}

export interface ProjectionDefinition<T> {
  projectionName: string;
  eventTypes: Query;
  keySelector(event: EventRecord): string | null;
  apply(state: T | null, event: EventRecord): T | null;
}

export class WasmProjectionStore {
  get(key: string): Promise<any | null>;
  getAll(): Promise<any[]>;
  save(key: string, state: any): Promise<void>;
  delete(key: string): Promise<void>;
  clear(): Promise<void>;
  queryByTag(tag: Tag): Promise<any[]>;
  queryByTags(tags: Tag[]): Promise<any[]>;
}

export class WasmProjectionRunner {
  rebuild(store: MarmosaEventStore): Promise<number>;
  processEvents(events: EventRecord[]): Promise<number>;
  getCheckpoint(): Promise<{
    projectionName: string;
    lastPosition: number;
    totalEventsProcessed: number;
  } | null>;
}

export class MarmosaEventStore {
  constructor();
  static withFileSystem(basePath: string): MarmosaEventStore;

  append(
    events: EventData[],
    condition?: AppendCondition | null
  ): Promise<void>;
  read(
    query: Query,
    startPosition?: number | null,
    maxCount?: number | null
  ): Promise<EventRecord[]>;
  readAll(query: Query): Promise<EventRecord[]>;
  readLast(query: Query): Promise<EventRecord | null>;

  buildDecisionModel<T>(
    projection: DecisionProjection<T>
  ): Promise<DecisionModel<T>>;
  executeDecision<R>(
    maxRetries: number,
    operation: (store: MarmosaEventStore) => Promise<R>
  ): Promise<R>;

  createProjectionStore(name: string): WasmProjectionStore;
  createProjectionRunner<T>(
    definition: ProjectionDefinition<T>,
    store: WasmProjectionStore
  ): WasmProjectionRunner;
}
