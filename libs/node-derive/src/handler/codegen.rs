// Generate output code

use crate::handler::extractors::ExtractorType;
use crate::handler::parse::HandlerInfo;
use proc_macro2::{Span, TokenStream};
use proc_macro_crate::{crate_name, FoundCrate};
use quote::quote;
use syn::Ident;

/// Get the crate path for constellation-node
///
/// Returns `crate` when used inside constellation-node itself,
/// or `::constellation_node` (or renamed alias) when used externally.
fn get_crate_path() -> TokenStream {
    let found_crate = crate_name("constellation-node")
        .expect("constellation-node is present in Cargo.toml");

    match found_crate {
        FoundCrate::Itself => quote!(crate),
        FoundCrate::Name(name) => {
            let ident = Ident::new(&name, Span::call_site());
            quote!(::#ident)
        }
    }
}

pub fn generate(info: HandlerInfo) -> TokenStream {
    let struct_def = generate_struct(&info);
    let handler_impl = generate_handler_impl(&info);
    let inventory_submit = generate_inventory(&info);
    let const_def = generate_const(&info);

    quote! {
        #struct_def
        #handler_impl
        #inventory_submit
        #const_def
    }
}

fn generate_struct(info: &HandlerInfo) -> TokenStream {
    let struct_name = &info.struct_name;

    quote! {
        struct #struct_name;
    }
}

fn generate_handler_impl(info: &HandlerInfo) -> TokenStream {
    let struct_name = &info.struct_name;
    let decode_request = generate_decode(info);
    let extract_deps = generate_extractors(info);
    let call_fn = generate_call(info);
    let encode_response = generate_encode(info);
    let crate_path = get_crate_path();

    quote! {
        #[::async_trait::async_trait]
        impl #crate_path::handler::Handler for #struct_name {
            async fn call(
                &self,
                node: &#crate_path::Node,
                request: &#crate_path::rpc::RpcRequest,
            ) -> ::std::result::Result<Vec<u8>, #crate_path::HandlerError> {
                #decode_request
                #extract_deps
                #call_fn
                #encode_response
            }
        }
    }
}

fn generate_decode(info: &HandlerInfo) -> TokenStream {
    let Some((request_param, request_type)) = &info.request else {
        // No request parameter - nothing to decode
        return TokenStream::new();
    };
    let crate_path = get_crate_path();

    quote! {
        let #request_param: #request_type = {
            let _span = #crate_path::telemetry::Span::enter("deserialize");
            ::constellation_fabric::Codec::Bincode
                .decode(&request.payload)
                .map_err(|e| #crate_path::HandlerError {
                    category: #crate_path::ErrorCategory::ClientError,
                    payload: Vec::new(), // Empty error payload - decode errors are framework-level
                })?
        };
    }
}

fn generate_extractors(info: &HandlerInfo) -> TokenStream {
    let crate_path = get_crate_path();
    let extractions = info.extractors.iter().map(|ext| {
        let name = &ext.name;
        match &ext.extractor_type {
            ExtractorType::Data { inner_ty } => {
                quote! {
                    let #name: #crate_path::Data<#inner_ty> = node.extract()
                        .ok_or_else(|| #crate_path::HandlerError {
                            category: #crate_path::ErrorCategory::ServerError,
                            payload: Vec::new(), // Empty error payload - missing dependency is framework-level
                        })?;
                }
            }
        }
    });

    quote! {
        #(#extractions)*
    }
}

fn generate_call(info: &HandlerInfo) -> TokenStream {
    let fn_name = &info.fn_name;
    let extractor_names: Vec<_> = info.extractors.iter().map(|ext| &ext.name).collect();
    let extractor_types: Vec<_> = info.extractors.iter().map(|ext| &ext.ty).collect();
    let body = &info.body;
    let response_type = &info.response_type;
    let error_type = &info.error_type;
    let crate_path = get_crate_path();

    // Generate function signature and call based on whether we have a request param
    let (fn_params, fn_call) = if let Some((request_param, request_type)) = &info.request {
        (
            quote! { #request_param: #request_type, #(#extractor_names: #extractor_types),* },
            quote! { #fn_name(#request_param, #(#extractor_names),*).await },
        )
    } else {
        (
            quote! { #(#extractor_names: #extractor_types),* },
            quote! { #fn_name(#(#extractor_names),*).await },
        )
    };

    quote! {
        async fn #fn_name(
            #fn_params
        ) -> ::core::result::Result<#response_type, #error_type> {
            #body
        }

        let response = match #fn_call {
            Ok(resp) => resp,
            Err(e) => {
                // Get error category
                use #crate_path::ErrorResponder;
                let category = e.error_category();

                // Serialize the error
                let error_payload = ::constellation_fabric::Codec::Bincode.encode(&e)
                    .map_err(|_| #crate_path::HandlerError {
                        category: #crate_path::ErrorCategory::ServerError,
                        payload: Vec::new(), // Failed to serialize error - framework-level problem
                    })?;

                // Return HandlerError
                return Err(#crate_path::HandlerError {
                    category,
                    payload: error_payload,
                });
            }
        };
    }
}

fn generate_encode(info: &HandlerInfo) -> TokenStream {
    let _response_type = &info.response_type;
    let crate_path = get_crate_path();

    quote! {
        {
            let _span = #crate_path::telemetry::Span::enter("serialize");
            ::constellation_fabric::Codec::Bincode.encode(&response)
                .map_err(|_| #crate_path::HandlerError {
                    category: #crate_path::ErrorCategory::ServerError,
                    payload: Vec::new(), // Failed to encode response - framework-level problem
                })
        }
    }
}

fn generate_inventory(info: &HandlerInfo) -> TokenStream {
    let struct_name = &info.struct_name;
    let fn_name_str = info.fn_name.to_string();
    let version = info.version;
    let crate_path = get_crate_path();

    if let Some(route) = &info.route {
        quote! {
            ::inventory::submit! {
                #crate_path::handler::HandlerRegistration::with_route(
                    #fn_name_str,
                    #version,
                    #route,
                    &#struct_name
                )
            }
        }
    } else {
        quote! {
            ::inventory::submit! {
                #crate_path::handler::HandlerRegistration::new(
                    #fn_name_str,
                    #version,
                    &#struct_name
                )
            }
        }
    }
}

fn generate_const(info: &HandlerInfo) -> TokenStream {
    let const_name = &info.const_name;
    let struct_name = &info.struct_name;

    quote! {
        pub const #const_name: #struct_name = #struct_name;
    }
}
