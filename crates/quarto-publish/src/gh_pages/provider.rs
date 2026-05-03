//! `GhPagesProvider` — Phase 0 stub.
//!
//! Real `prepare`/`commit`/`verify` impls land in Phase 1.

use async_trait::async_trait;

use crate::host::PublishHost;
use crate::provider::{PreparedPublish, PublishProvider};
use crate::renderer::PublishRenderer;
use crate::types::{
    AccountToken, PublishError, PublishInput, PublishOutcome, PublishRecord, PublishUx,
};

pub const PROVIDER_NAME: &str = "gh-pages";
pub const PROVIDER_DESCRIPTION: &str = "GitHub Pages";

pub struct GhPagesProvider;

impl GhPagesProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GhPagesProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PublishProvider for GhPagesProvider {
    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    fn description(&self) -> &'static str {
        PROVIDER_DESCRIPTION
    }

    async fn publish_record(
        &self,
        _input: &PublishInput,
        _host: &dyn PublishHost,
    ) -> Result<Option<PublishRecord>, PublishError> {
        // Phase 0: no detection. Phase 1 reads git state to detect
        // an existing gh-pages branch on origin.
        Ok(None)
    }

    async fn authorize_token(
        &self,
        _input: &PublishInput,
        _host: &dyn PublishHost,
    ) -> Result<Option<AccountToken>, PublishError> {
        // gh-pages is anonymous (no token storage). Phase 1 also
        // verifies git context here.
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
        unimplemented!("gh-pages prepare lands in bd-t3ny Phase 1")
    }

    async fn commit(
        &self,
        _prepared: PreparedPublish,
        _host: &dyn PublishHost,
    ) -> Result<PublishOutcome, PublishError> {
        unimplemented!("gh-pages commit lands in bd-t3ny Phase 1")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_and_description() {
        let p = GhPagesProvider::new();
        assert_eq!(p.name(), "gh-pages");
        assert_eq!(p.description(), "GitHub Pages");
    }

    #[test]
    fn does_not_require_server() {
        let p = GhPagesProvider::new();
        assert!(!p.requires_server());
    }

    #[test]
    fn requires_render_by_default() {
        // gh-pages needs files, so requires render.
        let p = GhPagesProvider::new();
        assert!(p.requires_render());
    }

    #[test]
    fn publish_record_in_phase_0_is_none() {
        // Phase 0 placeholder; Phase 1 detects existing branches.
        let p = GhPagesProvider::new();
        let input = PublishInput {
            project_dir: std::path::PathBuf::from("/tmp"),
            kind: crate::types::PublishKind::Site,
            title: "Test".into(),
            slug: "test".into(),
            site_url: None,
        };
        let (host, _captured) = crate::host::NativeHost::recording();
        let result = pollster::block_on(p.publish_record(&input, &host)).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn authorize_token_returns_anonymous() {
        let p = GhPagesProvider::new();
        let input = PublishInput {
            project_dir: std::path::PathBuf::from("/tmp"),
            kind: crate::types::PublishKind::Site,
            title: "Test".into(),
            slug: "test".into(),
            site_url: None,
        };
        let (host, _captured) = crate::host::NativeHost::recording();
        let token = pollster::block_on(p.authorize_token(&input, &host))
            .unwrap()
            .expect("anonymous account should be returned");
        assert!(matches!(token, AccountToken::Anonymous));
    }
}
