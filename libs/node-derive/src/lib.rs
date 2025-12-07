mod handler;

use proc_macro::TokenStream;

/// Marks an async function as an RPC handler
///
/// Generates handler registration code and wrapper for dependency extraction
///
/// # Example
/// ```text
/// #[handler]
/// async fn login(req: LoginRequest, db: Data<DbPool>) -> Result<LoginResponse, MyError> {
///     // Handler logic
/// }
/// ```
#[proc_macro_attribute]
pub fn handler(attr: TokenStream, item: TokenStream) -> TokenStream {
    match handler::expand(attr, item) {
        Ok(tokens) => tokens,
        Err(err) => err.to_compile_error().into(),
    }
}
