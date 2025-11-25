// Parse function signature into HandlerInfo

use crate::handler::attributes::HandlerAttributes;
use crate::handler::extractors::{identify_extractor, Extractor};
use proc_macro2::Span;
use syn::{Block, FnArg, Ident, ItemFn, Pat, ReturnType, Type};

/// Parsed handler information
pub struct HandlerInfo {
    pub fn_name: Ident,
    pub struct_name: Ident,
    pub const_name: Ident,
    pub request_param: Ident,
    pub request_type: Type,
    pub response_type: Type,
    pub error_type: Type,
    pub extractors: Vec<Extractor>,
    pub version: u32,
    pub route: Option<String>,
    pub body: Block,
}

pub fn parse_handler(input: ItemFn, attrs: HandlerAttributes) -> syn::Result<HandlerInfo> {
    // Validate function is async
    if input.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            &input.sig.fn_token,
            "Handler function must be async",
        ));
    }

    // Extract function name
    let fn_name = input.sig.ident.clone();

    // Generate struct and const names
    let struct_name = Ident::new(
        &format!("{}Handler", capitalize_first(&fn_name.to_string())),
        Span::call_site(),
    );
    let const_name = Ident::new(
        &format!("{}_HANDLER", fn_name.to_string().to_uppercase()),
        Span::call_site(),
    );

    // Parse parameters
    let mut params = input.sig.inputs.iter();

    // First parameter is always the request
    let first_param = params
        .next()
        .ok_or_else(|| syn::Error::new_spanned(&input.sig, "Handler must have at least one parameter (the request)"))?;

    let (request_param, request_type) = extract_param_info(first_param)?;

    // Remaining parameters are extractors
    let mut extractors = Vec::new();
    for param in params {
        let (name, ty) = extract_param_info(param)?;
        let extractor = identify_extractor(name, &ty)?;
        extractors.push(extractor);
    }

    // Parse return type
    let return_type = match &input.sig.output {
        ReturnType::Default => {
            return Err(syn::Error::new_spanned(
                &input.sig,
                "Handler must return Result<Response, Error>",
            ));
        }
        ReturnType::Type(_, ty) => ty.as_ref(),
    };

    let (response_type, error_type) = extract_result_types(return_type)?;

    // Get version
    let version = attrs.version.unwrap_or(1);

    Ok(HandlerInfo {
        fn_name,
        struct_name,
        const_name,
        request_param,
        request_type,
        response_type,
        error_type,
        extractors,
        version,
        route: attrs.route,
        body: *input.block,
    })
}

/// Extract parameter name and type from FnArg
fn extract_param_info(param: &FnArg) -> syn::Result<(Ident, Type)> {
    match param {
        FnArg::Typed(pat_type) => {
            let name = match pat_type.pat.as_ref() {
                Pat::Ident(ident) => ident.ident.clone(),
                _ => {
                    return Err(syn::Error::new_spanned(
                        param,
                        "Parameter must be a simple identifier",
                    ))
                }
            };
            let ty = (*pat_type.ty).clone();
            Ok((name, ty))
        }
        FnArg::Receiver(_) => Err(syn::Error::new_spanned(
            param,
            "Handler cannot have &self parameter",
        )),
    }
}

/// Extract Result<T, E> inner types
fn extract_result_types(ty: &Type) -> syn::Result<(Type, Type)> {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            if segment.ident == "Result" {
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    if args.args.len() == 2 {
                        if let (
                            syn::GenericArgument::Type(ok_ty),
                            syn::GenericArgument::Type(err_ty),
                        ) = (&args.args[0], &args.args[1])
                        {
                            return Ok((ok_ty.clone(), err_ty.clone()));
                        }
                    }
                }
            }
        }
    }

    Err(syn::Error::new_spanned(
        ty,
        "Return type must be Result<Response, Error>",
    ))
}

/// Capitalize first letter of string
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}
