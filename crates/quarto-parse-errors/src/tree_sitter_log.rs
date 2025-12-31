/*
 * tree_sitter_log.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Tree-sitter parser log observation for error reporting.
//!
//! This module provides utilities to capture and analyze tree-sitter's internal
//! parse state, enabling high-quality error message generation.

use std::collections::HashMap;

/// State of the tree-sitter log observer during parsing.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum TreeSitterLogState {
    Idle,
    InParse,
    JustReduced,
}

/// A message from the parser process containing parse state information.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ProcessMessage {
    pub version: usize,
    pub state: usize, // LR parser state
    pub row: usize,
    pub column: usize,
    pub sym: String, // Symbol at this position
    pub size: usize, // Size of the token in characters
}

/// A token that was consumed during parsing.
#[derive(Debug)]
pub struct ConsumedToken {
    pub row: usize,
    pub column: usize,
    pub size: usize,
    pub lr_state: usize,
    pub sym: String,
}

/// Complete log of a single parse attempt.
#[derive(Debug)]
pub struct TreeSitterParseLog {
    pub messages: Vec<String>,
    pub current_process: Option<usize>,
    pub current_lookahead: Option<(String, usize)>,
    pub processes: HashMap<usize, TreeSitterProcessLog>,
    pub all_tokens: Vec<ConsumedToken>,
    pub consumed_tokens: Vec<ConsumedToken>,
}

/// Log of a single parser process (GLR parsers may have multiple concurrent processes).
#[derive(Debug)]
pub struct TreeSitterProcessLog {
    pub found_accept: bool,
    pub found_bad_message: bool,
    pub error_states: Vec<ProcessMessage>,
    pub current_message: Option<ProcessMessage>,
}

impl TreeSitterProcessLog {
    pub fn is_good(&self) -> bool {
        self.found_accept && self.error_states.is_empty() && !self.found_bad_message
    }
}

impl TreeSitterParseLog {
    pub fn is_good(&self) -> bool {
        // For every process, there can't be any version that reached a state
        // with error states
        for process in self.processes.values() {
            if !process.is_good() {
                return false;
            }
        }
        true
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Trait for objects that can observe tree-sitter parser logs.
pub trait TreeSitterLogObserverTrait {
    fn had_errors(&self) -> bool;
    fn log(&mut self, log_type: tree_sitter::LogType, message: &str);
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Fast log observer that only tracks whether errors occurred (no detailed state).
#[derive(Default)]
pub struct TreeSitterLogObserverFast {
    pub saw_error: bool,
}


impl TreeSitterLogObserverTrait for TreeSitterLogObserverFast {
    fn had_errors(&self) -> bool {
        self.saw_error
    }
    fn log(&mut self, _log_type: tree_sitter::LogType, message: &str) {
        if message.starts_with("detect_error") {
            self.saw_error = true
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Full log observer that captures detailed parse state for error message generation.
pub struct TreeSitterLogObserver {
    pub parses: Vec<TreeSitterParseLog>,
    state: TreeSitterLogState,
}

impl TreeSitterLogObserver {
    pub fn had_errors(&self) -> bool {
        for parse in &self.parses {
            if !parse.is_good() {
                return true;
            }
        }
        false
    }
}

impl TreeSitterLogObserverTrait for TreeSitterLogObserver {
    fn had_errors(&self) -> bool {
        for parse in &self.parses {
            if !parse.is_good() {
                return true;
            }
        }
        false
    }

    fn log(&mut self, _log_type: tree_sitter::LogType, message: &str) {
        let words: Vec<&str> = message.split_whitespace().collect();
        if words.is_empty() {
            eprintln!("Empty log message from tree-sitter");
            return;
        }

        // Extract parameters directly into typed variables instead of creating a HashMap.
        // This eliminates expensive allocations: each HashMap creation costs ~608 bytes
        // (HashMap allocation + String allocations for keys and values + hash computations).
        // With only 6 possible parameters that are always parsed to the same types,
        // we can use simple Option variables on the stack (48 bytes total).
        let mut version: Option<usize> = None;
        let mut state: Option<usize> = None;
        let mut row: Option<usize> = None;
        let mut col: Option<usize> = None;
        let mut sym: Option<&str> = None;
        let mut size: Option<usize> = None;
        let mut child_count: Option<usize> = None;

        // Single pass through parameters
        for pair in &words[1..] {
            let pair = pair.trim_end_matches(',');
            if let Some((key, value)) = pair.split_once(':') {
                match key {
                    "version" => version = value.parse().ok(),
                    "state" => state = value.parse().ok(),
                    "row" => row = value.parse().ok(),
                    "col" => col = value.parse().ok(),
                    "sym" => sym = Some(value),
                    "size" => size = value.parse().ok(),
                    "child_count" => child_count = value.parse().ok(),
                    _ => {} // Ignore unknown parameters
                }
            }
        }
        match words[0] {
            "new_parse" => {
                if self.state != TreeSitterLogState::Idle {
                    panic!("Received 'new_parse' while not idle");
                }
                self.state = TreeSitterLogState::InParse;
                self.parses.push(TreeSitterParseLog {
                    messages: vec![],
                    processes: HashMap::new(),
                    current_process: None,
                    current_lookahead: None,
                    consumed_tokens: vec![],
                    all_tokens: vec![],
                });
            }
            "done" => {
                if self.state == TreeSitterLogState::Idle {
                    panic!("Received 'done' while idle");
                }
                self.state = TreeSitterLogState::Idle;
            }
            "reduce" => {
                let child_count = child_count.unwrap();
                if child_count > 0 {
                    let current_parse = self
                        .parses
                        .last_mut()
                        .expect("No current parse to log process to");
                    // after error correction we might have a completely messed up tree, but it should be good for the first error.
                    if current_parse.consumed_tokens.len() >= child_count {
                        let popped_tokens = current_parse
                            .consumed_tokens
                            .split_off(current_parse.consumed_tokens.len() - child_count);
                        let row = popped_tokens.first().unwrap().row;
                        let column = popped_tokens.first().unwrap().column;
                        let size = popped_tokens.iter().map(|x| x.size).sum();
                        let new_token = ConsumedToken {
                            row,
                            column,
                            size,
                            lr_state: 0,
                            sym: sym.unwrap().to_string().clone(),
                        };
                        current_parse.consumed_tokens.push(new_token);
                        current_parse.all_tokens.extend(popped_tokens);
                    }
                    self.state = TreeSitterLogState::JustReduced;
                }
            }
            "resume" => {
                let version = version.expect("Missing 'version' in process log");
                let current_parse = self
                    .parses
                    .last_mut()
                    .expect("No current parse to log process to");
                current_parse.current_process = Some(version);
            }
            "process" => {
                let version = version.expect("Missing 'version' in process log");
                let state = state.expect("Missing 'state' in process log");
                let row = row.expect("Missing 'row' in process log");
                let column = col.expect("Missing 'col' in process log");

                let current_parse = self
                    .parses
                    .last_mut()
                    .expect("No current parse to log process to");
                let no_lookahead = ("<no lookahead>".to_string(), 0);
                let (sym, size) = current_parse
                    .current_lookahead
                    .as_ref()
                    .unwrap_or(&no_lookahead);
                let current_process = current_parse.processes.entry(version).or_insert_with(|| {
                    TreeSitterProcessLog {
                        found_accept: false,
                        found_bad_message: false,
                        error_states: vec![],
                        current_message: None,
                    }
                });
                current_process.current_message = Some(ProcessMessage {
                    version,
                    state,
                    row,
                    column,
                    sym: sym.clone(),
                    size: *size,
                });
                current_parse.current_process = Some(version);
            }
            "detect_error" => {
                let current_parse = self
                    .parses
                    .last_mut()
                    .expect("No current parse to log process to");
                let current_process = current_parse
                    .processes
                    .get_mut(&current_parse.current_process.expect("No current process"))
                    .expect("No current process message");
                let current_process_message = current_process.current_message.as_ref().unwrap();
                current_process
                    .error_states
                    .push(current_process_message.clone());
            }
            "lexed_lookahead" => {
                let sym_str = sym.expect("Missing 'sym' in lexed_lookahead log");
                let size_val = size.expect("Missing 'size' in lexed_lookahead log");

                let current_parse = self
                    .parses
                    .last_mut()
                    .expect("No current parse to log process to");
                let current_process = current_parse
                    .processes
                    .get_mut(&current_parse.current_process.expect("No current process"))
                    .expect("No current process message");
                let current_process_message = current_process
                    .current_message
                    .as_mut()
                    .expect("No current process message");
                current_parse.current_lookahead = Some((sym_str.to_string(), size_val));
                current_process_message.sym = sym_str.to_string();
                current_process_message.size = size_val;
            }
            "shift" => {
                let state_val = state.expect("Missing 'state' in shift log");

                let current_parse = self
                    .parses
                    .last_mut()
                    .expect("No current parse to log process to");
                let current_process = current_parse
                    .processes
                    .get_mut(&current_parse.current_process.expect("No current process"))
                    .expect("No current process message");
                let current_process_message = current_process
                    .current_message
                    .as_mut()
                    .expect("No current process message");
                let size = current_parse
                    .current_lookahead
                    .as_ref()
                    .map_or(0, |(_, s)| *s);
                match self.state {
                    TreeSitterLogState::InParse => {
                        let new_token = ConsumedToken {
                            lr_state: state_val,
                            row: current_process_message.row,
                            column: current_process_message.column,
                            size,
                            sym: current_process_message.sym.clone(),
                        };
                        current_parse.consumed_tokens.push(new_token)
                    }
                    TreeSitterLogState::JustReduced => {
                        let last: &mut ConsumedToken =
                            current_parse.consumed_tokens.last_mut().unwrap();
                        last.lr_state = state_val;
                        self.state = TreeSitterLogState::InParse;
                        let new_token = ConsumedToken {
                            lr_state: state_val,
                            row: current_process_message.row,
                            column: current_process_message.column,
                            size,
                            sym: current_process_message.sym.clone(),
                        };
                        current_parse.consumed_tokens.push(new_token)
                    }
                    _ => {
                        eprintln!("Shouldn't be here!");
                    }
                }
            }
            "skip_token" | "recover_to_previous" => {
                // we want to mark these processes as bad, but we don't want to record the state here
                // because we only care about states we find via "detect_error"
                let current_parse = self
                    .parses
                    .last_mut()
                    .expect("No current parse to log process to");
                let current_process = current_parse
                    .processes
                    .get_mut(&current_parse.current_process.expect("No current process"))
                    .expect("No current process message");
                current_process.found_bad_message = true;
            }
            "lex_external" | "lex_internal" => {}
            "accept" => {
                let current_parse = self
                    .parses
                    .last_mut()
                    .expect("No current parse to log process to");
                let current_process = current_parse
                    .processes
                    .get_mut(&current_parse.current_process.expect("No current process"))
                    .expect("No current process message");
                current_process.found_accept = true;
            }
            _ => {
                if self.state == TreeSitterLogState::Idle {
                    return;
                }
            }
        }
        if let Some(current_parse) = self.parses.last_mut() {
            current_parse.messages.push(message.to_string());
        }
    }
}

impl Default for TreeSitterLogObserver {
    fn default() -> Self {
        TreeSitterLogObserver {
            parses: Vec::new(),
            state: TreeSitterLogState::Idle,
        }
    }
}
