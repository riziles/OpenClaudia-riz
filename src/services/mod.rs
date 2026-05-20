//! Service registry — organized analytics, feature flags, and other
//! cross-cutting concerns that would otherwise scatter across the
//! codebase.
//!
//! Port of Claude Code's `services/` layer (analytics / `GrowthBook` /
//! LSP manager / remote settings). Rather than one giant registry
//! struct, each service is defined by its own trait with a default
//! no-op impl. [`ServiceRegistry`] holds one `Arc<dyn Trait>` per
//! service, so adding a new service is:
//!
//! 1. Define the trait + its `Noop` impl in a submodule.
//! 2. Add an `Arc<dyn NewTrait>` field to `ServiceRegistry`.
//! 3. Expose an accessor.
//!
//! The registry defaults to "no-op everywhere" — nothing changes on
//! the hot path until a user (CLI / SDK caller / test) installs a
//! real implementation.

pub mod analytics;
pub mod background;
pub mod feature_flags;

pub use analytics::{AnalyticsEvent, AnalyticsSink, NoopAnalytics, TracingAnalytics};
pub use background::{BackgroundJob, JobOutcome, JobScheduler, MemoryConsolidationJob};
pub use feature_flags::{FeatureFlagSource, StaticFlags};

use std::sync::Arc;

/// Central service registry. Clone-cheap (`Arc` fields) so it can be
/// passed down the call tree without worrying about lifetime plumbing.
#[derive(Clone)]
pub struct ServiceRegistry {
    analytics: Arc<dyn AnalyticsSink>,
    flags: Arc<dyn FeatureFlagSource>,
}

impl ServiceRegistry {
    /// All services wired to their no-op default. Safe to use in
    /// tests and headless invocations where analytics emission or
    /// feature-flag resolution isn't desired.
    #[must_use]
    pub fn noop() -> Self {
        Self {
            analytics: Arc::new(NoopAnalytics),
            flags: Arc::new(StaticFlags::default()),
        }
    }

    /// Swap the analytics sink. Consuming builder style so test code
    /// can chain `noop().with_analytics(recording)` fluently.
    #[must_use]
    pub fn with_analytics(mut self, sink: Arc<dyn AnalyticsSink>) -> Self {
        self.analytics = sink;
        self
    }

    /// Swap the feature-flag source.
    #[must_use]
    pub fn with_flags(mut self, flags: Arc<dyn FeatureFlagSource>) -> Self {
        self.flags = flags;
        self
    }

    /// Borrow the analytics sink as a trait reference.
    ///
    /// Returns `&dyn AnalyticsSink` rather than `&Arc<dyn AnalyticsSink>`
    /// so callers don't depend on the registry's smart-pointer choice
    /// (crosslink #952). Replacing `Arc<dyn _>` with `Box<dyn _>` or a
    /// `&'static dyn _` singleton would now leave every call site
    /// unchanged. Callers that genuinely need a shared, long-lived
    /// handle should use [`Self::analytics_arc`].
    #[must_use]
    pub fn analytics(&self) -> &dyn AnalyticsSink {
        &*self.analytics
    }

    /// Borrow the feature-flag source as a trait reference.
    ///
    /// See [`Self::analytics`] — same reasoning. The Arc-flavoured
    /// accessor is [`Self::flags_arc`].
    #[must_use]
    pub fn flags(&self) -> &dyn FeatureFlagSource {
        &*self.flags
    }

    /// Explicit shared-ownership accessor for the analytics sink.
    ///
    /// Returns the underlying `Arc<dyn AnalyticsSink>` so a caller can
    /// keep the sink alive past the registry's borrow lifetime
    /// (e.g. to install it into a background task). This is the
    /// "I really do need the Arc" escape hatch — preferring
    /// [`Self::analytics`] keeps every other call site decoupled from
    /// the smart-pointer choice.
    #[must_use]
    pub fn analytics_arc(&self) -> Arc<dyn AnalyticsSink> {
        Arc::clone(&self.analytics)
    }

    /// Explicit shared-ownership accessor for the feature-flag source.
    /// See [`Self::analytics_arc`] for the motivation.
    #[must_use]
    pub fn flags_arc(&self) -> Arc<dyn FeatureFlagSource> {
        Arc::clone(&self.flags)
    }
}

impl Default for ServiceRegistry {
    fn default() -> Self {
        Self::noop()
    }
}

impl std::fmt::Debug for ServiceRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `Arc<dyn Trait>` isn't Debug; print type metadata without
        // trying to traverse the sinks. Keeps the struct usable in
        // `#[derive(Debug)]` contexts that transitively need it.
        f.debug_struct("ServiceRegistry")
            .field("analytics", &"<sink>")
            .field("flags", &"<source>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Test sink that records every event so assertions can inspect
    /// the order and contents. Mutex is fine — tests aren't hot.
    struct RecordingAnalytics {
        events: Mutex<Vec<AnalyticsEvent>>,
    }

    impl AnalyticsSink for RecordingAnalytics {
        fn record(&self, event: AnalyticsEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    #[test]
    fn default_registry_is_noop_everywhere() {
        let reg = ServiceRegistry::default();
        // No-op: calling record doesn't panic and has no observable
        // side effect. The assertion here is that we can call every
        // accessor without downcast or unwrap.
        reg.analytics().record(AnalyticsEvent::SessionStart {
            session_id: "s".to_string(),
        });
        assert!(!reg.flags().is_enabled("any_flag"));
    }

    #[test]
    fn with_analytics_swaps_sink() {
        let recording = Arc::new(RecordingAnalytics {
            events: Mutex::new(Vec::new()),
        });
        let reg = ServiceRegistry::noop().with_analytics(recording.clone());
        reg.analytics().record(AnalyticsEvent::ToolUsed {
            tool: "bash".to_string(),
            success: true,
        });
        let events = recording.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        matches!(events[0], AnalyticsEvent::ToolUsed { .. });
    }

    #[test]
    fn with_flags_swaps_source() {
        let mut flags = StaticFlags::default();
        flags.set("fast_path", true);
        let reg = ServiceRegistry::noop().with_flags(Arc::new(flags));
        assert!(reg.flags().is_enabled("fast_path"));
        assert!(!reg.flags().is_enabled("slow_path"));
    }

    #[test]
    fn registry_is_clone() {
        // Clone-cheap Arc semantics: the two handles point at the
        // same sinks. A test sink receiving events through either
        // handle sees them in the same vector.
        let recording = Arc::new(RecordingAnalytics {
            events: Mutex::new(Vec::new()),
        });
        let reg = ServiceRegistry::noop().with_analytics(recording.clone());
        let clone = reg.clone();

        reg.analytics().record(AnalyticsEvent::SessionStart {
            session_id: "a".to_string(),
        });
        clone.analytics().record(AnalyticsEvent::SessionEnd {
            session_id: "a".to_string(),
            messages: 10,
        });

        assert_eq!(recording.events.lock().unwrap().len(), 2);
    }
}
