use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, LitStr};
use std::fs;
use std::path::Path;

#[proc_macro]
pub fn include_error_table(input: TokenStream) -> TokenStream {
    let input_path = parse_macro_input!(input as LitStr);
    let path_str = input_path.value();
    
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR not set");
    
    let full_path = Path::new(&manifest_dir)
        .join(&path_str);
    
    let json_content = fs::read_to_string(&full_path)
        .expect(&format!("Failed to read JSON file at {:?}", full_path));
    
    let entries: Vec<serde_json::Value> = serde_json::from_str(&json_content)
        .expect("Failed to parse JSON");
    
    let table_entries = entries.iter().map(|entry| {
        let state = entry["state"].as_u64().unwrap() as usize;
        let sym = entry["sym"].as_str().unwrap();
        let row = entry["row"].as_u64().unwrap() as usize;
        let column = entry["column"].as_u64().unwrap() as usize;
        let error_msg = entry["errorMsg"].as_str().unwrap();
        
        quote! {
            crate::readers::qmd_error_message_table::ErrorTableEntry {
                state: #state,
                sym: #sym,
                row: #row,
                column: #column,
                error_msg: #error_msg,
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