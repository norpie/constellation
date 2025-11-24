// Parse #[handler(...)] attributes

use proc_macro::TokenStream;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitInt, Token};

/// Parsed attributes from #[handler(...)]
#[derive(Debug, Default)]
pub struct HandlerAttributes {
    pub version: Option<u32>,
}

impl Parse for HandlerAttributes {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut attrs = HandlerAttributes::default();

        // Empty attributes case
        if input.is_empty() {
            return Ok(attrs);
        }

        // Parse key = value pairs
        while !input.is_empty() {
            let key: Ident = input.parse()?;

            match key.to_string().as_str() {
                "version" => {
                    input.parse::<Token![=]>()?;
                    let value: LitInt = input.parse()?;
                    attrs.version = Some(value.base10_parse()?);
                }
                other => {
                    return Err(syn::Error::new_spanned(
                        &key,
                        format!("Unknown attribute: {}", other),
                    ));
                }
            }

            // Optional trailing comma
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(attrs)
    }
}

pub fn parse_attributes(attr: TokenStream) -> syn::Result<HandlerAttributes> {
    if attr.is_empty() {
        Ok(HandlerAttributes::default())
    } else {
        syn::parse(attr)
    }
}
