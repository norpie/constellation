mod attributes;
mod codegen;
mod extractors;
mod parse;

use proc_macro::TokenStream;
use syn::ItemFn;

/// Main entry point for handler macro expansion
pub fn expand(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    // Parse the attributes
    let attrs = attributes::parse_attributes(attr)?;

    // Parse the function
    let input: ItemFn = syn::parse(item)?;

    // Parse handler info from function signature
    let info = parse::parse_handler(input, attrs)?;

    // Generate output code
    let output = codegen::generate(info);

    Ok(output.into())
}
