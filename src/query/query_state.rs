//! Query state — tracks the latest issued `request_id` so stale responses are discarded.
//!
//! The correctness backbone of the type-as-you-go loop (`dev/PLAN.md` §3.1, §0/D4): every
//! `QueryRequest` carries a monotonic `request_id`; the dispatcher records the latest issued
//! id; any `QueryResponse` whose id isn't the latest is dropped before it touches result
//! state. This is what makes fast typing correct — a late result from a superseded keystroke
//! never overwrites the current one. Cancellation (interrupt) is a latency optimization on
//! top; *correctness* rests entirely on this stale-discard decision, which is a pure function.

/// Whether an incoming response id is stale relative to the latest issued id.
///
/// Pure and total over all `u64` pairs — unit-tested exhaustively at the boundaries, so only
/// delivery *timing* is ever left to a race, never the correctness of the decision.
pub fn is_stale(incoming_id: u64, latest_id: u64) -> bool {
    incoming_id < latest_id
}

/// Dispatcher-side bookkeeping: allocates monotonic request ids and decides staleness.
#[derive(Debug, Default)]
pub struct QueryState {
    latest_id: u64,
    in_flight: bool,
}

impl QueryState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocate the next request id and mark a query in-flight. Returns the new id to stamp
    /// on the `QueryRequest`.
    pub fn issue(&mut self) -> u64 {
        self.latest_id += 1;
        self.in_flight = true;
        self.latest_id
    }

    /// The most recently issued id.
    pub fn latest_id(&self) -> u64 {
        self.latest_id
    }

    /// Whether a query is currently in-flight (used to gate interrupts — the dispatcher only
    /// interrupts while it believes a specific request is running, §0/D4).
    pub fn in_flight(&self) -> bool {
        self.in_flight
    }

    /// Decide whether an arriving response should be accepted. A response is accepted only if
    /// its id is the latest issued; anything older is stale and dropped. Accepting clears the
    /// in-flight flag.
    pub fn accept(&mut self, incoming_id: u64) -> bool {
        if is_stale(incoming_id, self.latest_id) {
            return false;
        }
        // incoming_id == latest_id (a response can't have an id greater than what we issued)
        self.in_flight = false;
        true
    }
}

#[cfg(test)]
#[path = "query_state_tests.rs"]
mod query_state_tests;
