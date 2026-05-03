//! Top-level publish driver.
//!
//! Orchestrates `prepare → (dry-run? report : commit) → verify`,
//! emits human or JSON output, and decides exit semantics. Called
//! from `crates/quarto/src/commands/publish.rs`.

use std::sync::Arc;

use crate::host::PublishHost;
use crate::provider::{ProviderRegistry, PublishProvider};
use crate::renderer::PublishRenderer;
use crate::types::{
    AccountToken, PublishError, PublishEvent, PublishInput, PublishOutcome, PublishUx,
};

/// Inputs to a single `quarto publish` invocation.
pub struct ExecuteArgs<'a> {
    pub provider_name: String,
    pub input: PublishInput,
    pub ux: PublishUx,
    pub registry: &'a ProviderRegistry,
    pub renderer: &'a dyn PublishRenderer,
    pub host: &'a dyn PublishHost,
}

/// Run a publish.
///
/// Returns the final `PublishOutcome` on success. Side effects
/// (writing to stdout, opening the browser) are the caller's
/// responsibility — this function only emits events through the
/// host and produces the structured outcome.
///
/// Flow:
///
/// 1. Look up the provider in the registry.
/// 2. Resolve an account token (anonymous for gh-pages).
/// 3. Optionally look up an existing publish record (target).
/// 4. Call `provider.prepare`.
/// 5. If `ux.dry_run` → emit the plan, return synthesized
///    dry-run outcome.
/// 6. Else call `provider.commit`, then `provider.verify`.
pub async fn execute(args: ExecuteArgs<'_>) -> Result<PublishOutcome, PublishError> {
    let ExecuteArgs {
        provider_name,
        input,
        ux,
        registry,
        renderer,
        host,
    } = args;

    let provider = registry.find(&provider_name).ok_or_else(|| {
        let known = registry.known_names().join(", ");
        PublishError::UnableToPublish {
            provider: "publish",
            message: format!("unknown provider '{provider_name}'. Available providers: {known}"),
        }
    })?;

    if !ux.render && provider.requires_render() {
        return Err(PublishError::UnableToPublish {
            provider: provider.name(),
            message: format!(
                "{} requires rendering before publish; --no-render is not supported \
                 with this provider.",
                provider.description()
            ),
        });
    }

    let account = resolve_account(provider.as_ref(), &input, host).await?;
    let target = provider.publish_record(&input, host).await?;

    host.emit(PublishEvent::PrepareStart {
        provider: provider.name().to_string(),
    })
    .await;

    let prepared = provider
        .prepare(&account, &input, renderer, &ux, host, target.as_ref())
        .await?;

    // Surface the plan so dry-run consumers (and JSON callers) see it.
    host.emit(PublishEvent::Plan {
        provider: provider.name().to_string(),
        actions: prepared.plan.clone(),
    })
    .await;

    if ux.dry_run {
        let summary = crate::types::PublishSummary {
            commit: None,
            deploy_id: None,
            file_count: prepared.files.files.len(),
            bytes: prepared
                .files
                .files
                .iter()
                .filter_map(|f| std::fs::metadata(prepared.files.base_dir.join(f)).ok())
                .map(|m| m.len())
                .sum(),
        };
        let url = prepared
            .destination
            .url
            .as_deref()
            .and_then(|u| url::Url::parse(u).ok());
        return Ok(PublishOutcome::dry_run(provider.name(), summary, url));
    }

    host.emit(PublishEvent::CommitStart {
        provider: provider.name().to_string(),
    })
    .await;

    let mut outcome = provider.commit(prepared, host).await?;

    host.emit(PublishEvent::CommitComplete {
        provider: provider.name().to_string(),
    })
    .await;

    provider.verify(&mut outcome, &ux, host).await?;

    Ok(outcome)
}

/// Resolve an account token. Phase 1 only handles the anonymous
/// path; future providers with credential storage will pick from
/// `provider.account_tokens()` and prompt as needed.
async fn resolve_account(
    provider: &dyn PublishProvider,
    input: &PublishInput,
    host: &dyn PublishHost,
) -> Result<AccountToken, PublishError> {
    if let Some(token) = provider.authorize_token(input, host).await? {
        return Ok(token);
    }
    let tokens = provider.account_tokens(host).await?;
    tokens
        .into_iter()
        .next()
        .ok_or_else(|| PublishError::Unauthorized {
            provider: provider.name(),
            source: anyhow::anyhow!("no account tokens available"),
        })
}

/// Convenience: build a `ProviderRegistry` and call `execute`.
pub async fn execute_with_builtins(
    provider_name: String,
    input: PublishInput,
    ux: PublishUx,
    renderer: &dyn PublishRenderer,
    host: &dyn PublishHost,
) -> Result<PublishOutcome, PublishError> {
    let registry = Arc::new(ProviderRegistry::with_builtins());
    execute(ExecuteArgs {
        provider_name,
        input,
        ux,
        registry: &registry,
        renderer,
        host,
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::NativeHost;
    use crate::provider::{PreparedPublish, PublishProvider};
    use crate::renderer::{PublishRenderFlags, PublishRenderer};
    use crate::types::*;
    use async_trait::async_trait;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// Fake renderer that always succeeds with a fixed file list.
    struct FakeRenderer {
        base_dir: PathBuf,
    }

    #[async_trait]
    impl PublishRenderer for FakeRenderer {
        async fn render(&self, _flags: &PublishRenderFlags) -> Result<PublishFiles, PublishError> {
            Ok(PublishFiles {
                base_dir: self.base_dir.clone(),
                root_file: "index.html".to_string(),
                files: vec!["index.html".to_string()],
            })
        }
    }

    /// Fake provider that records what it was called with and
    /// returns a synthetic outcome. Used to verify the driver wires
    /// the trait surface together correctly.
    struct FakeProvider {
        name: &'static str,
    }

    #[async_trait]
    impl PublishProvider for FakeProvider {
        fn name(&self) -> &'static str {
            self.name
        }
        fn description(&self) -> &'static str {
            "Fake provider for tests"
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
            input: &PublishInput,
            renderer: &dyn PublishRenderer,
            _ux: &PublishUx,
            _host: &dyn PublishHost,
            _target: Option<&PublishRecord>,
        ) -> Result<PreparedPublish, PublishError> {
            let files = renderer.render(&PublishRenderFlags::default()).await?;
            Ok(PreparedPublish {
                provider: self.name,
                staging_dir: input.project_dir.clone(),
                files,
                destination: PublishDestination {
                    provider: self.name.to_string(),
                    description: "fake destination".to_string(),
                    url: Some("https://fake.example/".to_string()),
                },
                plan: vec![PublishAction::Note {
                    message: "fake plan".to_string(),
                }],
                provider_state: Box::new(()),
            })
        }
        async fn commit(
            &self,
            prepared: PreparedPublish,
            _host: &dyn PublishHost,
        ) -> Result<PublishOutcome, PublishError> {
            Ok(PublishOutcome {
                provider: prepared.provider.to_string(),
                record: None,
                url: prepared
                    .destination
                    .url
                    .as_deref()
                    .and_then(|u| url::Url::parse(u).ok()),
                admin_url: None,
                summary: PublishSummary {
                    commit: Some("fakecommit".to_string()),
                    deploy_id: None,
                    file_count: prepared.files.files.len(),
                    bytes: 0,
                },
                verified: false,
                dry_run: false,
            })
        }
    }

    fn fake_input() -> PublishInput {
        PublishInput {
            project_dir: PathBuf::from("/tmp/fake-project"),
            kind: PublishKind::Site,
            title: "Test".into(),
            slug: "test".into(),
            site_url: None,
        }
    }

    #[test]
    fn unknown_provider_yields_clear_error() {
        let registry = ProviderRegistry::with_builtins();
        let renderer = FakeRenderer {
            base_dir: PathBuf::from("/tmp"),
        };
        let (host, _captured) = NativeHost::recording();
        let err = pollster::block_on(execute(ExecuteArgs {
            provider_name: "no-such-provider".to_string(),
            input: fake_input(),
            ux: PublishUx::default(),
            registry: &registry,
            renderer: &renderer,
            host: &host,
        }))
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown provider"), "got: {msg}");
        assert!(msg.contains("no-such-provider"), "got: {msg}");
        // Should also list known providers (gh-pages built in).
        assert!(msg.contains("gh-pages"), "got: {msg}");
    }

    #[test]
    fn no_render_with_render_required_provider_errors() {
        let mut registry = ProviderRegistry::empty();
        registry.register(Arc::new(FakeProvider { name: "fake" }));
        let renderer = FakeRenderer {
            base_dir: PathBuf::from("/tmp"),
        };
        let (host, _captured) = NativeHost::recording();
        let mut ux = PublishUx::default();
        ux.render = false;
        let err = pollster::block_on(execute(ExecuteArgs {
            provider_name: "fake".to_string(),
            input: fake_input(),
            ux,
            registry: &registry,
            renderer: &renderer,
            host: &host,
        }))
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("requires rendering"), "got: {msg}");
    }

    #[test]
    fn dry_run_does_not_call_commit() {
        let mut registry = ProviderRegistry::empty();
        registry.register(Arc::new(FakeProvider { name: "fake" }));
        let renderer = FakeRenderer {
            base_dir: PathBuf::from("/tmp"),
        };
        let (host, captured) = NativeHost::recording();
        let mut ux = PublishUx::default();
        ux.dry_run = true;
        ux.browser = false; // dry-run already forces this, but be explicit
        let outcome = pollster::block_on(execute(ExecuteArgs {
            provider_name: "fake".to_string(),
            input: fake_input(),
            ux,
            registry: &registry,
            renderer: &renderer,
            host: &host,
        }))
        .unwrap();
        assert!(outcome.dry_run, "outcome should be marked dry-run");
        assert_eq!(outcome.provider, "fake");
        // Confirm commit was not invoked: outcome.summary.commit
        // would be Some("fakecommit") if commit ran.
        assert!(outcome.summary.commit.is_none());

        // Confirm the plan event was emitted (so a JSON consumer
        // sees it under --json --dry-run).
        let events = captured.lock().unwrap();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, PublishEvent::Plan { .. })),
            "expected a Plan event among {events:?}"
        );
        // Confirm CommitStart was NOT emitted under dry-run.
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, PublishEvent::CommitStart { .. })),
            "did not expect a CommitStart event under dry-run"
        );
    }

    #[test]
    fn happy_path_emits_prepare_plan_commit_events() {
        let mut registry = ProviderRegistry::empty();
        registry.register(Arc::new(FakeProvider { name: "fake" }));
        let renderer = FakeRenderer {
            base_dir: PathBuf::from("/tmp"),
        };
        let (host, captured) = NativeHost::recording();
        let outcome = pollster::block_on(execute(ExecuteArgs {
            provider_name: "fake".to_string(),
            input: fake_input(),
            ux: PublishUx {
                browser: false,
                wait: false, // skip verify; OK because browser is off
                ..PublishUx::default()
            },
            registry: &registry,
            renderer: &renderer,
            host: &host,
        }))
        .unwrap();
        assert!(!outcome.dry_run);
        assert_eq!(outcome.summary.commit.as_deref(), Some("fakecommit"));

        let events = captured.lock().unwrap();
        let kinds: Vec<_> = events
            .iter()
            .map(|e| match e {
                PublishEvent::PrepareStart { .. } => "prepare-start",
                PublishEvent::Plan { .. } => "plan",
                PublishEvent::CommitStart { .. } => "commit-start",
                PublishEvent::CommitComplete { .. } => "commit-complete",
                _ => "other",
            })
            .collect();
        assert_eq!(
            kinds,
            vec!["prepare-start", "plan", "commit-start", "commit-complete"]
        );
    }
}
