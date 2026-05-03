//! `PublishProvider` trait and the `ProviderRegistry` lookup.
//!
//! The trait is **dyn-compatible** (no method generics, no
//! associated types tied to the impl, no return-position `impl
//! Trait`). This is load-bearing: the registry holds
//! `Arc<dyn PublishProvider>`, and a future WASM-bridged provider
//! (registered at startup from the JS side) plugs into the same
//! registry.
//!
//! The publish flow is split into three steps so that `--dry-run`
//! has a clean cut point:
//!
//! 1. **`prepare`** — read state, render, stage, plan. May make
//!    read-only network calls but **must not** mutate the
//!    destination.
//! 2. **`commit`** — push or upload. Once Ok, the deploy is
//!    irrevocable from the CLI's perspective.
//! 3. **`verify`** — optional post-commit poll for "is it live yet?".

use std::any::Any;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use crate::host::PublishHost;
use crate::renderer::PublishRenderer;
use crate::types::{
    AccountToken, PublishAction, PublishDestination, PublishError, PublishFiles, PublishInput,
    PublishOutcome, PublishRecord, PublishUx,
};

/// Output of `prepare()`. Carries everything `commit()` needs and
/// nothing more.
///
/// `provider_state` is the keep-it-dyn-friendly escape hatch:
/// providers stash their per-publish private state (worktree path,
/// signed URLs, deploy id, ...) in a `Box<dyn Any>` and downcast
/// inside `commit`. A typed associated state would break dyn-
/// compatibility, which we need for the registry.
pub struct PreparedPublish {
    pub provider: &'static str,
    pub staging_dir: PathBuf,
    pub files: PublishFiles,
    pub destination: PublishDestination,
    pub plan: Vec<PublishAction>,
    pub provider_state: Box<dyn Any + Send + Sync>,
}

/// Source of publishing capabilities.
#[async_trait]
pub trait PublishProvider: Send + Sync {
    /// Provider name (`"gh-pages"`, etc.). Stable identifier used
    /// in `_publish.yml`, in CLI args, and in machine-readable
    /// output.
    fn name(&self) -> &'static str;

    /// Human description ("GitHub Pages", "Netlify", ...).
    fn description(&self) -> &'static str;

    /// True if the provider needs an explicit `--server` argument.
    fn requires_server(&self) -> bool {
        false
    }

    /// True if the publish flow requires re-rendering (most do).
    /// `quarto publish --no-render` is rejected when this is true.
    fn requires_render(&self) -> bool {
        true
    }

    /// True for providers that exist for testing or are not yet
    /// publicly listed.
    fn hidden(&self) -> bool {
        false
    }

    /// Look up an existing publish target for this input, *without*
    /// any prompts. For gh-pages this means "is there already a
    /// gh-pages branch on origin?".
    async fn publish_record(
        &self,
        input: &PublishInput,
        host: &dyn PublishHost,
    ) -> Result<Option<PublishRecord>, PublishError>;

    /// Return account tokens this provider can use. Default impl
    /// returns the anonymous account — providers that need real
    /// auth override.
    async fn account_tokens(
        &self,
        _host: &dyn PublishHost,
    ) -> Result<Vec<AccountToken>, PublishError> {
        Ok(vec![AccountToken::anonymous()])
    }

    /// Authorize a token. For gh-pages this just verifies the
    /// repo has an origin and returns the anonymous account.
    async fn authorize_token(
        &self,
        input: &PublishInput,
        host: &dyn PublishHost,
    ) -> Result<Option<AccountToken>, PublishError>;

    /// Side-effect-free planning + render + staging. Returns a
    /// `PreparedPublish` describing what would be committed. May
    /// read disk and the local git state, may render via
    /// `renderer`, may make read-only network calls — **must not**
    /// push, upload, or otherwise mutate the destination.
    async fn prepare(
        &self,
        account: &AccountToken,
        input: &PublishInput,
        renderer: &dyn PublishRenderer,
        ux: &PublishUx,
        host: &dyn PublishHost,
        target: Option<&PublishRecord>,
    ) -> Result<PreparedPublish, PublishError>;

    /// Push or upload the prepared publish. After Ok, the deploy
    /// is irrevocable.
    async fn commit(
        &self,
        prepared: PreparedPublish,
        host: &dyn PublishHost,
    ) -> Result<PublishOutcome, PublishError>;

    /// Optional post-commit verification (e.g. polling
    /// `.nojekyll`). Default impl is a no-op for providers that
    /// don't support post-deploy verification.
    async fn verify(
        &self,
        _outcome: &mut PublishOutcome,
        _ux: &PublishUx,
        _host: &dyn PublishHost,
    ) -> Result<(), PublishError> {
        Ok(())
    }
}

/// Lookup table of registered providers.
///
/// Open by design — built-ins register at construction
/// (`ProviderRegistry::with_builtins()`), and additional providers
/// can be registered at runtime (`register()`). This is the seam
/// for future extension-loaded providers.
#[derive(Default, Clone)]
pub struct ProviderRegistry {
    providers: HashMap<&'static str, Arc<dyn PublishProvider>>,
}

impl ProviderRegistry {
    /// Empty registry. Useful in tests; production use should
    /// prefer `with_builtins`.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Registry pre-populated with all built-in providers.
    pub fn with_builtins() -> Self {
        let mut registry = Self::empty();
        registry.register(Arc::new(crate::gh_pages::GhPagesProvider::new()));
        registry
    }

    /// Register a provider. Last-write-wins on name collisions.
    pub fn register(&mut self, provider: Arc<dyn PublishProvider>) {
        self.providers.insert(provider.name(), provider);
    }

    /// Look up a provider by name.
    pub fn find(&self, name: &str) -> Option<Arc<dyn PublishProvider>> {
        self.providers.get(name).cloned()
    }

    /// All registered providers, sorted by name (stable order for
    /// listings and error messages).
    pub fn all(&self) -> Vec<Arc<dyn PublishProvider>> {
        let mut providers: Vec<_> = self.providers.values().cloned().collect();
        providers.sort_by_key(|p| p.name());
        providers
    }

    /// Names of all registered providers, sorted, suitable for use
    /// in user-facing error messages.
    pub fn known_names(&self) -> Vec<&'static str> {
        let mut names: Vec<_> = self.providers.keys().copied().collect();
        names.sort();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trivial provider used to prove the registry is open for
    /// runtime registration.
    struct NoopProvider;

    #[async_trait]
    impl PublishProvider for NoopProvider {
        fn name(&self) -> &'static str {
            "noop"
        }
        fn description(&self) -> &'static str {
            "No-op (test only)"
        }
        async fn publish_record(
            &self,
            _input: &PublishInput,
            _host: &dyn PublishHost,
        ) -> Result<Option<PublishRecord>, PublishError> {
            Ok(None)
        }
        async fn authorize_token(
            &self,
            _input: &PublishInput,
            _host: &dyn PublishHost,
        ) -> Result<Option<AccountToken>, PublishError> {
            Ok(Some(AccountToken::anonymous()))
        }
        async fn prepare(
            &self,
            _account: &AccountToken,
            _input: &PublishInput,
            _renderer: &dyn PublishRenderer,
            _ux: &PublishUx,
            _host: &dyn PublishHost,
            _target: Option<&PublishRecord>,
        ) -> Result<PreparedPublish, PublishError> {
            unreachable!("noop provider not exercised in this test")
        }
        async fn commit(
            &self,
            _prepared: PreparedPublish,
            _host: &dyn PublishHost,
        ) -> Result<PublishOutcome, PublishError> {
            unreachable!("noop provider not exercised in this test")
        }
    }

    #[test]
    fn empty_registry_finds_nothing() {
        let r = ProviderRegistry::empty();
        assert!(r.find("gh-pages").is_none());
        assert!(r.find("anything").is_none());
        assert!(r.known_names().is_empty());
    }

    #[test]
    fn builtins_registry_finds_gh_pages() {
        let r = ProviderRegistry::with_builtins();
        let provider = r.find("gh-pages").expect("gh-pages should be registered");
        assert_eq!(provider.name(), "gh-pages");
    }

    #[test]
    fn builtins_registry_does_not_find_unknown() {
        let r = ProviderRegistry::with_builtins();
        assert!(r.find("does-not-exist").is_none());
    }

    #[test]
    fn registry_is_open_for_runtime_registration() {
        let mut r = ProviderRegistry::empty();
        assert!(r.find("noop").is_none());
        r.register(Arc::new(NoopProvider));
        assert_eq!(
            r.find("noop").expect("registered").name(),
            "noop",
            "noop provider should be findable after register()"
        );
    }

    #[test]
    fn runtime_registration_overrides_existing() {
        // Last-write-wins is the documented contract — used by
        // hypothetical extensions that override built-ins.
        struct OverrideProvider;
        #[async_trait]
        impl PublishProvider for OverrideProvider {
            fn name(&self) -> &'static str {
                "gh-pages" // collide with the builtin
            }
            fn description(&self) -> &'static str {
                "Overridden GitHub Pages"
            }
            async fn publish_record(
                &self,
                _input: &PublishInput,
                _host: &dyn PublishHost,
            ) -> Result<Option<PublishRecord>, PublishError> {
                Ok(None)
            }
            async fn authorize_token(
                &self,
                _input: &PublishInput,
                _host: &dyn PublishHost,
            ) -> Result<Option<AccountToken>, PublishError> {
                Ok(Some(AccountToken::anonymous()))
            }
            async fn prepare(
                &self,
                _account: &AccountToken,
                _input: &PublishInput,
                _renderer: &dyn PublishRenderer,
                _ux: &PublishUx,
                _host: &dyn PublishHost,
                _target: Option<&PublishRecord>,
            ) -> Result<PreparedPublish, PublishError> {
                unreachable!()
            }
            async fn commit(
                &self,
                _prepared: PreparedPublish,
                _host: &dyn PublishHost,
            ) -> Result<PublishOutcome, PublishError> {
                unreachable!()
            }
        }

        let mut r = ProviderRegistry::with_builtins();
        r.register(Arc::new(OverrideProvider));
        let p = r.find("gh-pages").unwrap();
        assert_eq!(p.description(), "Overridden GitHub Pages");
    }

    #[test]
    fn known_names_returns_sorted_list() {
        let mut r = ProviderRegistry::empty();
        r.register(Arc::new(NoopProvider));
        // Build a second provider so we can verify ordering.
        struct Z;
        #[async_trait]
        impl PublishProvider for Z {
            fn name(&self) -> &'static str {
                "zzz"
            }
            fn description(&self) -> &'static str {
                "Z"
            }
            async fn publish_record(
                &self,
                _input: &PublishInput,
                _host: &dyn PublishHost,
            ) -> Result<Option<PublishRecord>, PublishError> {
                Ok(None)
            }
            async fn authorize_token(
                &self,
                _input: &PublishInput,
                _host: &dyn PublishHost,
            ) -> Result<Option<AccountToken>, PublishError> {
                Ok(Some(AccountToken::anonymous()))
            }
            async fn prepare(
                &self,
                _account: &AccountToken,
                _input: &PublishInput,
                _renderer: &dyn PublishRenderer,
                _ux: &PublishUx,
                _host: &dyn PublishHost,
                _target: Option<&PublishRecord>,
            ) -> Result<PreparedPublish, PublishError> {
                unreachable!()
            }
            async fn commit(
                &self,
                _prepared: PreparedPublish,
                _host: &dyn PublishHost,
            ) -> Result<PublishOutcome, PublishError> {
                unreachable!()
            }
        }
        r.register(Arc::new(Z));
        assert_eq!(r.known_names(), vec!["noop", "zzz"]);
    }
}
