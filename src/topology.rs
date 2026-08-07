//! The NIC priority matrix, in Rust.
//!
//! Pure data: nothing here calls the engine. The engine takes the matrix as a
//! JSON string, and this is what builds that string.

use std::collections::BTreeMap;
use std::fmt;

/// Which HCA serves memory registered under which location.
///
/// The engine takes this as a JSON string whose shape is documented nowhere but
/// Mooncake's own `TopologyEntry::toJson`: every location maps to a two-element
/// array, preferred HCAs first and fallbacks second. Built here rather than
/// spelled out by each caller, because a matrix that parses but names the wrong
/// key is indistinguishable from a working one until throughput is measured.
///
/// Ordering is deterministic so the rendered matrix can be logged and compared.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NicPriorityMatrix {
    entries: BTreeMap<String, (Vec<String>, Vec<String>)>,
}

impl NicPriorityMatrix {
    pub fn new() -> Self {
        Self::default()
    }

    /// Serve `location` from `hca` and nothing else.
    ///
    /// No fallback is the useful default on a multi-HCA host: falling back to a
    /// NIC under a different PCIe switch than the memory still moves the bytes,
    /// at a fraction of the rate, and nothing reports it. A missing NIC is a
    /// loud failure; a slow one is not.
    pub fn pin(mut self, location: impl Into<String>, hca: impl Into<String>) -> Self {
        self.entries
            .insert(location.into(), (vec![hca.into()], Vec::new()));
        self
    }

    /// Serve `location` from `preferred`, falling back to `fallback`.
    ///
    /// Prefer [`Self::pin`] unless the fallback is genuinely equivalent; see
    /// there for why.
    pub fn entry(
        mut self,
        location: impl Into<String>,
        preferred: Vec<String>,
        fallback: Vec<String>,
    ) -> Self {
        self.entries
            .insert(location.into(), (preferred, fallback));
        self
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The locations this matrix covers, in sorted order.
    ///
    /// The matrix replaces the discovered topology rather than extending it, so
    /// a location absent from here has no NIC at all. Callers that register
    /// memory under a computed location can check it against this first and
    /// fail at startup instead of at the first transfer.
    pub fn locations(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    pub fn covers(&self, location: &str) -> bool {
        self.entries.contains_key(location)
    }

    /// Render as the JSON the engine expects.
    pub fn to_json(&self) -> String {
        let entries: serde_json::Map<String, serde_json::Value> = self
            .entries
            .iter()
            .map(|(location, (preferred, fallback))| {
                (
                    location.clone(),
                    serde_json::json!([preferred, fallback]),
                )
            })
            .collect();

        serde_json::Value::Object(entries).to_string()
    }
}

impl fmt::Display for NicPriorityMatrix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_json())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn pinned_location_has_no_fallback() {
        let matrix = NicPriorityMatrix::new()
            .pin("cuda:0", "ibp0")
            .pin("cuda:3", "ibp3");

        let parsed: serde_json::Value = serde_json::from_str(&matrix.to_json()).unwrap();

        assert_eq!(parsed["cuda:0"], serde_json::json!([["ibp0"], []]));
        assert_eq!(parsed["cuda:3"], serde_json::json!([["ibp3"], []]));
        assert_eq!(parsed.as_object().unwrap().len(), 2);
    }

    #[test]
    fn entry_keeps_both_lists() {
        let matrix = NicPriorityMatrix::new().entry(
            "cpu:0",
            vec!["ibp0".to_string()],
            vec!["ibp1".to_string()],
        );

        let parsed: serde_json::Value = serde_json::from_str(&matrix.to_json()).unwrap();

        assert_eq!(parsed["cpu:0"], serde_json::json!([["ibp0"], ["ibp1"]]));
    }

    #[test]
    fn covers_reports_the_keys_registration_will_use() {
        let matrix = NicPriorityMatrix::new().pin("cuda:0", "ibp0");

        assert!(matrix.covers("cuda:0"));
        assert!(!matrix.covers("cuda:1"));
        assert_eq!(matrix.locations().collect::<Vec<_>>(), ["cuda:0"]);
    }

    #[test]
    fn rendering_is_deterministic() {
        let a = NicPriorityMatrix::new()
            .pin("cuda:1", "ibp1")
            .pin("cuda:0", "ibp0");
        let b = NicPriorityMatrix::new()
            .pin("cuda:0", "ibp0")
            .pin("cuda:1", "ibp1");

        assert_eq!(a.to_json(), b.to_json());
    }

    #[test]
    fn empty_matrix_is_empty() {
        assert!(NicPriorityMatrix::new().is_empty());
        assert!(!NicPriorityMatrix::new().pin("cuda:0", "ibp0").is_empty());
    }
}
