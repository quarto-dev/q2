use proc_macro::TokenStream;
use quote::quote;
use serde::Deserialize;
use std::fs;
use std::path::Path;
use syn::{parse_macro_input, LitStr};

#[derive(Deserialize)]
struct Capture {
    column: usize,
    #[serde(rename = "lrState")]
    lr_state: usize,
    row: usize,
    size: usize,
    sym: String,
    label: String,
}

#[derive(Deserialize)]
struct Note {
    message: String,
    label: Option<String>,
    #[serde(rename = "noteType")]
    note_type: Option<String>,
    #[serde(rename = "labelBegin")]
    label_begin: Option<String>,
    #[serde(rename = "labelEnd")]
    label_end: Option<String>,
}

#[derive(Deserialize)]
struct ErrorInfo {
    title: String,
    message: String,
    captures: Vec<Capture>,
    notes: Vec<Note>,
}

#[derive(Deserialize)]
struct ErrorEntry {
    column: usize,
    row: usize,
    state: usize,
    sym: String,
    #[serde(rename = "errorInfo")]
    error_info: ErrorInfo,
    name: String,
}

#[proc_macro]
pub fn include_error_table(input: TokenStream) -> TokenStream {
    let input_path = parse_macro_input!(input as LitStr);
    let path_str = input_path.value();

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");

    let full_path = Path::new(&manifest_dir).join(&path_str);

    let json_content = fs::read_to_string(&full_path)
        .expect(&format!("Failed to read JSON file at {:?}", full_path));

    let entries: Vec<ErrorEntry> =
        serde_json::from_str(&json_content).expect("Failed to parse JSON");

    let table_entries = entries.iter().map(|entry| {
        let state = entry.state;
        let sym = &entry.sym;
        let row = entry.row;
        let column = entry.column;
        let title = &entry.error_info.title;
        let message = &entry.error_info.message;
        let name = &entry.name;

        let captures = entry.error_info.captures.iter().map(|cap| {
            let cap_column = cap.column;
            let cap_lr_state = cap.lr_state;
            let cap_row = cap.row;
            let cap_size = cap.size;
            let cap_sym = &cap.sym;
            let cap_label = &cap.label;

            quote! {
                crate::readers::qmd_error_message_table::ErrorCapture {
                    column: #cap_column,
                    lr_state: #cap_lr_state,
                    row: #cap_row,
                    size: #cap_size,
                    sym: #cap_sym,
                    label: #cap_label,
                }
            }
        });

        let notes = entry.error_info.notes.iter().map(|note| {
            let note_message = &note.message;
            let note_label = match &note.label {
                Some(label) => quote! { Some(#label) },
                None => quote! { None },
            };
            let note_type = match &note.note_type {
                Some(t) => quote! { Some(#t) },
                None => quote! { None },
            };

            quote! {
                crate::readers::qmd_error_message_table::ErrorNote {
                    message: #note_message,
                    label: #note_label,
                    note_type: #note_type,
                }
            }
        });

        quote! {
            crate::readers::qmd_error_message_table::ErrorTableEntry {
                state: #state,
                sym: #sym,
                row: #row,
                column: #column,
                error_info: crate::readers::qmd_error_message_table::ErrorInfo {
                    title: #title,
                    message: #message,
                    captures: &[#(#captures),*],
                    notes: &[#(#notes),*],
                },
                name: #name,
            }
        }
    });

    let expanded = quote! {
        &[
            #(#table_entries),*
        ]
    };

    TokenStream::from(expanded)
}
