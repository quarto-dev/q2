//! Terminal prompting seam for `q2 create` (bd-hh1erpfx).
//!
//! The [`Prompter`] trait keeps the prompt *flow* unit-testable — the
//! tests in `mod.rs` drive it with a scripted fake, no PTY needed.
//! [`InquirePrompter`] is the real terminal implementation, rendering
//! via `inquire` (whose crossterm backend is the same crossterm
//! already in the tree via pampa). Prompt UI renders on stderr,
//! keeping stdout reserved for command output.

use super::artifact::CreateFailure;

/// One selectable row: the label shown, plus a help/description line.
#[derive(Clone)]
pub struct PromptItem {
    pub label: String,
    pub help: String,
}

pub trait Prompter {
    /// Present a selection list; returns the chosen index.
    fn select(&mut self, prompt: &str, items: &[PromptItem]) -> Result<usize, CreateFailure>;

    /// Ask for a line of text. When `default` is given, an empty
    /// submission returns the default.
    fn input(&mut self, prompt: &str, default: Option<&str>) -> Result<String, CreateFailure>;
}

/// Real terminal prompter backed by `inquire`.
pub struct InquirePrompter;

fn map_inquire_err(e: inquire::InquireError) -> CreateFailure {
    match e {
        inquire::InquireError::OperationCanceled | inquire::InquireError::OperationInterrupted => {
            CreateFailure::cancelled()
        }
        other => CreateFailure::new("Prompt failed", other.to_string()),
    }
}

impl Prompter for InquirePrompter {
    fn select(&mut self, prompt: &str, items: &[PromptItem]) -> Result<usize, CreateFailure> {
        let options: Vec<String> = items
            .iter()
            .map(|i| {
                if i.help.is_empty() {
                    i.label.clone()
                } else {
                    format!("{} — {}", i.label, i.help)
                }
            })
            .collect();
        let chosen = inquire::Select::new(prompt, options.clone())
            .prompt()
            .map_err(map_inquire_err)?;
        Ok(options
            .iter()
            .position(|o| *o == chosen)
            .expect("selected option came from the offered list"))
    }

    fn input(&mut self, prompt: &str, default: Option<&str>) -> Result<String, CreateFailure> {
        let mut text = inquire::Text::new(prompt);
        if let Some(d) = default {
            text = text.with_default(d);
        }
        text.prompt().map_err(map_inquire_err)
    }
}
