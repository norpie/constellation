use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

/// Marks an async function as an RPC handler
///
/// Generates handler registration code and wrapper for dependency extraction
#[proc_macro_attribute]
pub fn handler(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);

    // TODO: Implement handler macro
    // - Validate function signature
    // - Extract route name from function name
    // - Parse version from attributes
    // - Generate Handler trait impl
    // - Generate inventory registration
    // - Generate public constant for manual registration

    let output = quote! {
        #input
    };

    output.into()
}
