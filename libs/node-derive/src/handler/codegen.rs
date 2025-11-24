// Generate output code

use crate::handler::extractors::ExtractorType;
use crate::handler::parse::HandlerInfo;
use proc_macro2::TokenStream;
use quote::quote;

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

    quote! {
        #[::async_trait::async_trait]
        impl ::constellation_node::handler::Handler for #struct_name {
            async fn call(
                &self,
                node: &::constellation_node::Node,
                request: &::constellation_node::rpc::RpcRequest,
            ) -> ::std::result::Result<Vec<u8>, ::constellation_node::HandlerError> {
                #decode_request
                #extract_deps
                #call_fn
                #encode_response
            }
        }
    }
}

fn generate_decode(info: &HandlerInfo) -> TokenStream {
    let request_param = &info.request_param;
    let request_type = &info.request_type;

    quote! {
        use ::constellation_fabric::codec::Codec as _;
        let codec = ::constellation_fabric::codec::BincodeCodec;
        let #request_param: #request_type = codec.decode(&request.payload)
            .map_err(|e| ::constellation_node::HandlerError {
                category: ::constellation_node::ErrorCategory::ClientError,
                payload: Vec::new(), // Empty error payload - decode errors are framework-level
            })?;
    }
}

fn generate_extractors(info: &HandlerInfo) -> TokenStream {
    let extractions = info.extractors.iter().map(|ext| {
        let name = &ext.name;
        match &ext.extractor_type {
            ExtractorType::Data { inner_ty } => {
                quote! {
                    let #name: ::constellation_node::Data<#inner_ty> = node.extract()
                        .ok_or_else(|| ::constellation_node::HandlerError {
                            category: ::constellation_node::ErrorCategory::ServerError,
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
    let request_param = &info.request_param;
    let request_type = &info.request_type;
    let extractor_names: Vec<_> = info.extractors.iter().map(|ext| &ext.name).collect();
    let extractor_types: Vec<_> = info.extractors.iter().map(|ext| &ext.ty).collect();
    let body = &info.body;
    let response_type = &info.response_type;
    let error_type = &info.error_type;

    quote! {
        async fn #fn_name(
            #request_param: #request_type,
            #(#extractor_names: #extractor_types),*
        ) -> ::core::result::Result<#response_type, #error_type> {
            #body
        }

        let response = match #fn_name(#request_param, #(#extractor_names),*).await {
            Ok(resp) => resp,
            Err(e) => {
                // Get error category
                use ::constellation_node::ErrorResponder;
                let category = e.error_category();

                // Serialize the error
                let error_payload = codec.encode(&e)
                    .map_err(|_| ::constellation_node::HandlerError {
                        category: ::constellation_node::ErrorCategory::ServerError,
                        payload: Vec::new(), // Failed to serialize error - framework-level problem
                    })?;

                // Return HandlerError
                return Err(::constellation_node::HandlerError {
                    category,
                    payload: error_payload,
                });
            }
        };
    }
}

fn generate_encode(info: &HandlerInfo) -> TokenStream {
    let _response_type = &info.response_type;

    quote! {
        codec.encode(&response)
            .map_err(|_| ::constellation_node::HandlerError {
                category: ::constellation_node::ErrorCategory::ServerError,
                payload: Vec::new(), // Failed to encode response - framework-level problem
            })
    }
}

fn generate_inventory(info: &HandlerInfo) -> TokenStream {
    let struct_name = &info.struct_name;
    let fn_name_str = info.fn_name.to_string();
    let version = info.version;

    quote! {
        ::inventory::submit! {
            ::constellation_node::handler::HandlerRegistration::new(
                #fn_name_str,
                #version,
                &#struct_name
            )
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
