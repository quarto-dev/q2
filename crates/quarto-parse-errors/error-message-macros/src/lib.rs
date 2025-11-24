use proc_macro::TokenStream;
use quote::quote;
use serde::Deserialize;
use std::fs;
use std::path::Path;
use syn::{parse::{Parse, ParseStream}, parse_macro_input, LitStr, Token};

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
    note_type: String,
    #[serde(rename = "labelBegin")]
    label_begin: Option<String>,
    #[serde(rename = "labelEnd")]
    label_end: Option<String>,
    #[serde(rename = "trimLeadingSpace")]
    trim_leading_space: Option<bool>,
    #[serde(rename = "trimTrailingSpace")]
    trim_trailing_space: Option<bool>,
}

#[derive(Deserialize)]
struct ErrorInfo {
    code: Option<String>,
    title: String,
    message: String,
    captures: Vec<Capture>,
    notes: Vec<Note>,
    #[serde(default)]
    hints: Vec<String>,
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

struct IncludeErrorTableInput {
    path: LitStr,
    _comma: Token![,],
    module_prefix: LitStr,
}

impl Parse for IncludeErrorTableInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(IncludeErrorTableInput {
            path: input.parse()?,
            _comma: input.parse()?,
            module_prefix: input.parse()?,
        })
    }
}

#[proc_macro]
pub fn include_error_table(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as IncludeErrorTableInput);
    let path_str = input.path.value();
    let module_prefix = input.module_prefix.value();

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");

    let full_path = Path::new(&manifest_dir).join(&path_str);

    let json_content = fs::read_to_string(&full_path)
        .expect(&format!("Failed to read JSON file at {:?}", full_path));

    let entries: Vec<ErrorEntry> =
        serde_json::from_str(&json_content).expect("Failed to parse JSON");

    // Parse module prefix into tokens
    let module_tokens: proc_macro2::TokenStream = module_prefix.parse().expect("Invalid module prefix");

    let table_entries = entries.iter().map(|entry| {
        let state = entry.state;
        let sym = &entry.sym;
        let row = entry.row;
        let column = entry.column;
        let code = match &entry.error_info.code {
            Some(c) => quote! { Some(#c) },
            None => quote! { None },
        };
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
                #module_tokens::ErrorCapture {
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
            let note_type = &note.note_type;
            let note_label_begin = match &note.label_begin {
                Some(label) => quote! { Some(#label) },
                None => quote! { None },
            };
            let note_label_end = match &note.label_end {
                Some(label) => quote! { Some(#label) },
                None => quote! { None },
            };
            let trim_leading_space = match &note.trim_leading_space {
                Some(trim) => quote! { Some(#trim) },
                None => quote! { None },
            };
            let trim_trailing_space = match &note.trim_trailing_space {
                Some(trim) => quote! { Some(#trim) },
                None => quote! { None },
            };

            quote! {
                #module_tokens::ErrorNote {
                    message: #note_message,
                    label: #note_label,
                    note_type: #note_type,
                    label_begin: #note_label_begin,
                    label_end: #note_label_end,
                    trim_leading_space: #trim_leading_space,
                    trim_trailing_space: #trim_trailing_space,
                }
            }
        });

        let hints = entry.error_info.hints.iter().map(|hint| {
            quote! { #hint }
        });

        quote! {
            #module_tokens::ErrorTableEntry {
                state: #state,
                sym: #sym,
                row: #row,
                column: #column,
                error_info: #module_tokens::ErrorInfo {
                    code: #code,
                    title: #title,
                    message: #message,
                    captures: &[#(#captures),*],
                    notes: &[#(#notes),*],
                    hints: &[#(#hints),*],
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
