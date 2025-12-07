// Identify and validate extractor types

use quote::ToTokens;
use syn::{GenericArgument, Ident, PathArguments, Type};

/// Type of extractor
#[derive(Clone)]
pub enum ExtractorType {
    /// Data<T> extractor
    Data { inner_ty: Type },
}

/// Information about an extractor parameter
#[derive(Clone)]
pub struct Extractor {
    pub name: Ident,
    pub ty: Type,
    pub extractor_type: ExtractorType,
}

/// Check if a type looks like an extractor (Data<T>)
pub fn is_extractor_type(ty: &Type) -> bool {
    extract_data_type(ty).is_some()
}

/// Check if a type is a recognized extractor and extract information
pub fn identify_extractor(name: Ident, ty: &Type) -> syn::Result<Extractor> {
    // Try to parse as Data<T>
    if let Some(inner_ty) = extract_data_type(ty) {
        return Ok(Extractor {
            name,
            ty: ty.clone(),
            extractor_type: ExtractorType::Data { inner_ty },
        });
    }

    // Not a recognized extractor
    Err(syn::Error::new_spanned(
        ty,
        format!(
            "Unknown extractor type: {}. Supported extractors: Data<T>",
            ty.to_token_stream()
        ),
    ))
}

/// Extract inner type from Data<T>
fn extract_data_type(ty: &Type) -> Option<Type> {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            if segment.ident == "Data" {
                if let PathArguments::AngleBracketed(args) = &segment.arguments {
                    if args.args.len() == 1 {
                        if let GenericArgument::Type(inner) = &args.args[0] {
                            return Some(inner.clone());
                        }
                    }
                }
            }
        }
    }
    None
}
