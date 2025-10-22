/*
 * tree_sitter_log_observer.rs
 * Copyright (c) 2025 Posit, PBC
 */

use std::collections::HashMap; // Still needed for TreeSitterParseLog::processes

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum TreeSitterLogState {
    Idle,
    InParse,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ProcessMessage {
    pub version: usize,
    pub state: usize,
    pub row: usize,
    pub column: usize,
    pub sym: String,
    pub size: usize,
}

#[derive(Debug)]
pub struct ConsumedToken {
    pub row: usize,
    pub column: usize,
    pub size: usize,
    pub lr_state: usize,
    pub sym: String,
}

#[derive(Debug)]
pub struct TreeSitterParseLog {
    pub messages: Vec<String>,
    pub current_process: Option<usize>,
    pub current_lookahead: Option<(String, usize)>,
    pub processes: HashMap<usize, TreeSitterProcessLog>,
    pub consumed_tokens: Vec<ConsumedToken>, // row, column, size, LR state
}

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
        // for every process, there can't be any version that reached a state
        // with error states
        for (_, process) in &self.processes {
            if !process.is_good() {
                return false;
            }
        }
        true
    }
}

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
    pub fn log(&mut self, _log_type: tree_sitter::LogType, message: &str) {
        // Implement your logging logic here
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
                });
            }
            "done" => {
                if self.state != TreeSitterLogState::InParse {
                    panic!("Received 'done' while not in parse");
                }
                self.state = TreeSitterLogState::Idle;
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
                    .map(|(_, s)| *s)
                    .unwrap_or(0);
                current_parse.consumed_tokens.push(ConsumedToken {
                    lr_state: state_val,
                    row: current_process_message.row,
                    column: current_process_message.column,
                    size,
                    sym: current_process_message.sym.clone(), // TODO would prefer not to clone here
                })
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
            "lex_external" | "lex_internal" | "reduce" => {}
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
                if self.state != TreeSitterLogState::InParse {
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
