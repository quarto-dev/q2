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

pub struct ConsumedToken {
    pub row: usize,
    pub column: usize,
    pub size: usize,
    pub lr_state: usize,
    pub sym: String,
}

pub struct TreeSitterParseLog {
    pub messages: Vec<String>,
    pub processes: HashMap<usize, ProcessMessage>,
    pub current_process: Option<usize>,
    pub current_lookahead: Option<(String, usize)>,
    pub found_accept: bool,
    pub error_states: Vec<ProcessMessage>,
    pub consumed_tokens: Vec<ConsumedToken>, // row, column, size, LR state
}

pub struct TreeSitterLogObserver {
    pub parses: Vec<TreeSitterParseLog>,
    state: TreeSitterLogState,
}

impl TreeSitterLogObserver {
    pub fn had_errors(&self) -> bool {
        !self
            .parses
            .iter()
            .all(|parse| parse.found_accept && parse.error_states.is_empty())
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
                    found_accept: false,
                    error_states: vec![],
                    consumed_tokens: vec![],
                });
            }
            "done" => {
                if self.state != TreeSitterLogState::InParse {
                    panic!("Received 'done' while not in parse");
                }
                self.state = TreeSitterLogState::Idle;
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
                current_parse.processes.insert(
                    version,
                    ProcessMessage {
                        version,
                        state,
                        row,
                        column,
                        sym: sym.clone(),
                        size: *size,
                    },
                );
                current_parse.current_process = Some(version);
            }
            "detect_error" => {
                let current_parse = self
                    .parses
                    .last_mut()
                    .expect("No current parse to log process to");
                let process = current_parse.current_process.unwrap();
                current_parse
                    .error_states
                    .push(current_parse.processes.get(&process).unwrap().clone());
            }
            "lexed_lookahead" => {
                let current_parse = self
                    .parses
                    .last_mut()
                    .expect("No current parse to log process to");
                let current_process_message = current_parse
                    .processes
                    .get_mut(&current_parse.current_process.expect("No current process"))
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
                let current_process_message = current_parse
                    .processes
                    .get_mut(&current_parse.current_process.expect("No current process"))
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
                self.parses
                    .last_mut()
                    .expect("No current parse to log process to")
                    .found_accept = true;
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
