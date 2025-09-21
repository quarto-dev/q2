/*
 * tree_sitter_log_observer.rs
 * Copyright (c) 2025 Posit, PBC
 */

use std::collections::HashMap;

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
    pub error_states: Vec<ProcessMessage>,
    pub current_message: Option<ProcessMessage>,
}

impl TreeSitterProcessLog {
    pub fn is_good(&self) -> bool {
        self.found_accept && self.error_states.is_empty()
    }
}

impl TreeSitterParseLog {
    pub fn is_good(&self) -> bool {
        // for every process, there needs to be at least one version that reached an accept state
        // with no error states
        for (_, process) in &self.processes {
            if process.is_good() {
                return true;
            }
        }
        false
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
        let str = message.to_string();

        let words: Vec<&str> = str.split_whitespace().collect();
        if words.len() == 0 {
            panic!("Empty log message from tree-sitter");
        }
        let params: HashMap<String, String> = words[1..]
            .iter()
            .filter_map(|pair| {
                let pair = pair.trim_suffix(",");
                let mut split = pair.splitn(2, ':');
                if let (Some(key), Some(value_str)) = (split.next(), split.next()) {
                    return Some((key.to_string(), value_str.to_string()));
                }
                None
            })
            .collect();
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
                let version = params
                    .get("version")
                    .expect("Missing 'version' in process log")
                    .parse::<usize>()
                    .expect("Failed to parse 'version' as usize");
                let current_parse = self
                    .parses
                    .last_mut()
                    .expect("No current parse to log process to");
                current_parse.current_process = Some(version);
            }
            "process" => {
                let version = params
                    .get("version")
                    .expect("Missing 'version' in process log")
                    .parse::<usize>()
                    .expect("Failed to parse 'version' as usize");
                let state = params
                    .get("state")
                    .expect("Missing 'state' in process log")
                    .parse::<usize>()
                    .expect("Failed to parse 'state' as usize");
                let row = params
                    .get("row")
                    .expect("Missing 'row' in process log")
                    .parse::<usize>()
                    .expect("Failed to parse 'row' as usize");
                let column = params
                    .get("col")
                    .expect("Missing 'col' in process log")
                    .parse::<usize>()
                    .expect("Failed to parse 'col' as usize");

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
            "detect_error" | "skip_token" | "recover_to_previous" => {
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
                current_parse.current_lookahead = Some((
                    params.get("sym").unwrap().to_string(),
                    params.get("size").unwrap().parse::<usize>().unwrap(),
                ));
                current_process_message.sym = params.get("sym").unwrap().to_string();
                current_process_message.size =
                    params.get("size").unwrap().parse::<usize>().unwrap();
            }
            "shift" => {
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
                let state = params
                    .get("state")
                    .expect("Missing 'state' in shift log")
                    .parse::<usize>()
                    .expect("Failed to parse 'state' as usize");
                let size = current_parse
                    .current_lookahead
                    .as_ref()
                    .map(|(_, s)| *s)
                    .unwrap_or(0);
                current_parse.consumed_tokens.push(ConsumedToken {
                    lr_state: state,
                    row: current_process_message.row,
                    column: current_process_message.column,
                    size,
                    sym: current_process_message.sym.clone(), // TODO would prefer not to clone here
                })
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
        match self.parses.last_mut() {
            Some(current_parse) => {
                current_parse.messages.push(str);
            }
            _ => {}
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
