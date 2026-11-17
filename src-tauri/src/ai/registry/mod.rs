//! The model registry the app runs on (P05): every model, price, limit,
//! capability, feature default and provider data term comes from here and
//! nowhere else. `ai::models` keeps the functions callers already used, as a
//! thin layer over [`current`].
//!
//! - `load`: the bundled copy, the cached copy, the checks a copy must pass
//!   and the data-terms floor.
//! - `fetch`: the signed remote copy (`GET /v1/registry`), on launch, every
//!   six hours and after a wake; never on the startup path.
//!
//! BYOK calls still go from the Mac to the provider directly; the registry
//! carries no keys and never decides where a key is sent.

pub mod doc;
pub mod fetch;
pub mod load;
mod lookup;

pub use doc::{DataTerms, Model, Provider, Registry, Status};
pub use load::Source;
pub use lookup::ModelState;

use std::sync::{Arc, LazyLock, RwLock};

/// The registry compiled into this build. Always valid: the Rust test
/// `the_bundled_registry_is_usable` parses it, and so does every launch.
/// Under test, a test may put its own copy in its place on its thread
/// ([`test_override::bundled_with`]); the copy in use at launch
/// ([`holder`]) is always the compiled one.
pub fn bundled() -> Arc<Registry> {
    #[cfg(test)]
    if let Some(registry) = test_override::bundled() {
        return registry;
    }
    compiled()
}

fn compiled() -> Arc<Registry> {
    static BUNDLED: LazyLock<Arc<Registry>> =
        LazyLock::new(|| Arc::new(Registry::parse(load::BUNDLED_JSON.as_bytes()).expect("registry/registry.json is a usable registry")));
    Arc::clone(&BUNDLED)
}

/// The copy in use and where it came from.
#[derive(Debug, Clone)]
pub struct Active {
    pub registry: Arc<Registry>,
    pub source: Source,
}

/// Holds the copy in use. The app has one ([`holder`]); tests make their own.
pub struct Holder {
    active: RwLock<Active>,
}

impl Holder {
    pub fn new(active: Active) -> Self {
        Holder { active: RwLock::new(active) }
    }

    pub fn get(&self) -> Active {
        self.active.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
    }

    pub fn set(&self, active: Active) {
        *self.active.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = active;
    }
}

/// The app's copy in use: the bundled copy until [`start`] has read the
/// cache.
pub fn holder() -> &'static Holder {
    static HOLDER: LazyLock<Holder> = LazyLock::new(|| Holder::new(Active { registry: compiled(), source: Source::Bundled }));
    &HOLDER
}

/// The registry every lookup reads. Cheap: one read lock and an `Arc` clone.
/// Under test: the frozen fixture, or a test's own copy ([`test_override`]).
pub fn current() -> Arc<Registry> {
    #[cfg(test)]
    return test_override::get().unwrap_or_else(fixture);
    #[cfg(not(test))]
    holder().get().registry
}

/// registry/registry.json as seeded (version 1), frozen for the tests:
/// the app's tests run on it, so editing registry/registry.json (a price, a
/// default) never needs a test change. The bundled copy itself is checked
/// by `the_bundled_registry_is_usable` and by registry/validate.ts.
#[cfg(test)]
pub(crate) fn fixture() -> Arc<Registry> {
    static FIXTURE: LazyLock<Arc<Registry>> = LazyLock::new(|| Arc::new(Registry::parse(include_bytes!("fixture.json")).expect("the test fixture is a usable registry")));
    Arc::clone(&FIXTURE)
}

/// What Settings > About shows (`ai_models`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Info {
    pub version: u64,
    pub issued_at: String,
    pub source: Source,
}

/// At launch, before anything asks for a model: use the cached copy when it
/// passes every check, else the bundled copy. Reads one small file and
/// verifies one signature; never touches the network.
pub fn start() {
    let bundled = bundled();
    let cache = load::cache_dir().and_then(|dir| load::read_cache(&dir));
    let (registry, source) = load::at_launch(&bundled, cache.as_ref(), &load::compiled_key(), load::app_version());
    log::info!("registry: version {} ({:?})", registry.version, source);
    holder().set(Active { registry: Arc::new(registry), source });
}

/// For tests that need a registry other than the bundled one: every lookup
/// on this thread reads it until the guard is dropped. `#[tokio::test]` runs
/// on one thread, so this covers async tests too.
#[cfg(test)]
pub(crate) mod test_override {
    use super::Registry;
    use std::cell::RefCell;
    use std::sync::Arc;

    thread_local! {
        static OVERRIDE: RefCell<Option<Arc<Registry>>> = const { RefCell::new(None) };
        static BUNDLED: RefCell<Option<Arc<Registry>>> = const { RefCell::new(None) };
    }

    pub(crate) fn get() -> Option<Arc<Registry>> {
        OVERRIDE.with(|o| o.borrow().clone())
    }

    pub(crate) struct Guard(Option<Arc<Registry>>);

    impl Drop for Guard {
        fn drop(&mut self) {
            let previous = self.0.take();
            OVERRIDE.with(|o| *o.borrow_mut() = previous);
        }
    }

    pub(crate) fn set(registry: Registry) -> Guard {
        let previous = OVERRIDE.with(|o| o.borrow_mut().replace(Arc::new(registry)));
        Guard(previous)
    }

    /// The test fixture with `change` applied, in place for this thread.
    pub(crate) fn with(change: impl FnOnce(&mut Registry)) -> Guard {
        let mut registry = (*super::fixture()).clone();
        change(&mut registry);
        set(registry)
    }

    pub(crate) fn bundled() -> Option<Arc<Registry>> {
        BUNDLED.with(|b| b.borrow().clone())
    }

    pub(crate) struct BundledGuard(Option<Arc<Registry>>);

    impl Drop for BundledGuard {
        fn drop(&mut self) {
            let previous = self.0.take();
            BUNDLED.with(|b| *b.borrow_mut() = previous);
        }
    }

    /// The bundled copy with `change` applied, returned by `bundled()` on
    /// this thread until the guard is dropped: for a test that shows some
    /// code does not depend on it.
    pub(crate) fn bundled_with(change: impl FnOnce(&mut Registry)) -> BundledGuard {
        let mut registry = (*super::bundled()).clone();
        change(&mut registry);
        BundledGuard(BUNDLED.with(|b| b.borrow_mut().replace(Arc::new(registry))))
    }
}

#[cfg(test)]
mod tests;
