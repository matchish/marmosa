/**
 * Pure TypeScript test harness for Decider-based event sourcing.
 *
 * No event store, no async, no WASM dependency — just fold events into state
 * via `evolve`, call `decide`, and assert on the result.
 *
 * Inspired by Emmett's DeciderSpecification.
 *
 * @example
 * ```ts
 * import { DeciderSpecification } from "marmosa-wasm/testing";
 *
 * const given = DeciderSpecification.for(myDecider);
 *
 * test("enroll student", () => {
 *   given([{ type: "CourseCreated", data: { maxCapacity: 30 } }])
 *     .when({ type: "EnrollStudent", data: { studentId: "s1" } })
 *     .then([{ type: "StudentEnrolled", data: { studentId: "s1" } }]);
 * });
 * ```
 */

/** A pure decider: initialState + evolve + decide. Same object works in tests and production. */
export interface Decider<Command, Event, State> {
  /** Returns the initial state before any events. */
  initialState: () => State;
  /** Folds a single event into the current state. */
  evolve: (state: State, event: Event) => State;
  /** Given a command and current state, returns new events (or throws on error). */
  decide: (command: Command, state: State) => Event[];
}

export interface DeciderSpecificationType {
  for: <Command, Event, State>(
    decider: Decider<Command, Event, State>,
  ) => (
    givenEvents: Event | Event[],
  ) => {
    when: (command: Command) => {
      then: (expectedEvents: Event | Event[]) => void;
      thenNothingHappened: () => void;
      thenThrows: (check?: (error: Error) => boolean) => void;
    };
  };
}

function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (a == null || b == null) return false;
  if (typeof a !== typeof b) return false;

  if (Array.isArray(a)) {
    if (!Array.isArray(b) || a.length !== b.length) return false;
    return a.every((val, i) => deepEqual(val, b[i]));
  }

  if (typeof a === "object") {
    const keysA = Object.keys(a as Record<string, unknown>);
    const keysB = Object.keys(b as Record<string, unknown>);
    if (keysA.length !== keysB.length) return false;
    return keysA.every((key) =>
      deepEqual(
        (a as Record<string, unknown>)[key],
        (b as Record<string, unknown>)[key],
      ),
    );
  }

  return false;
}

export const DeciderSpecification: DeciderSpecificationType = {
  for: <Command, Event, State>(
    decider: Decider<Command, Event, State>,
  ) => {
    return (givenEvents: Event | Event[]) => {
      return {
        when: (command: Command) => {
          const handle = () => {
            const events = Array.isArray(givenEvents)
              ? givenEvents
              : [givenEvents];

            const currentState = events.reduce<State>(
              decider.evolve,
              decider.initialState(),
            );

            return decider.decide(command, currentState);
          };

          return {
            then: (expectedEvents: Event | Event[]) => {
              const result = handle();
              const expected = Array.isArray(expectedEvents)
                ? expectedEvents
                : [expectedEvents];

              if (!deepEqual(result, expected)) {
                throw new Error(
                  `Expected events:\n${JSON.stringify(expected, null, 2)}\n\nActual events:\n${JSON.stringify(result, null, 2)}`,
                );
              }
            },

            thenNothingHappened: () => {
              const result = handle();
              if (result.length !== 0) {
                throw new Error(
                  `Expected no events, but got ${result.length}:\n${JSON.stringify(result, null, 2)}`,
                );
              }
            },

            thenThrows: (check?: (error: Error) => boolean) => {
              let threw = false;
              try {
                handle();
              } catch (error) {
                threw = true;
                if (check && !check(error as Error)) {
                  throw new Error(
                    `Error did not match the check: ${(error as Error).message}`,
                  );
                }
              }
              if (!threw) {
                throw new Error("Expected an error to be thrown, but none was");
              }
            },
          };
        },
      };
    };
  },
};
