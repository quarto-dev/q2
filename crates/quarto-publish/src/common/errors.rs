//! Error-construction helpers shared across providers.

use crate::types::PublishError;

/// Format an "unable to publish" error with a human reason.
///
/// Producers should use this for user-fixable preconditions
/// (no git installed, no origin remote, etc.) — *not* for
/// transient I/O failures, which belong in `PublishError::Other`.
pub fn unable_to_publish(provider: &'static str, message: impl Into<String>) -> PublishError {
    PublishError::UnableToPublish {
        provider,
        message: message.into(),
    }
}
