use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

/// Attribute macro that leaves the annotated async function unchanged.
/// It exists to mirror the Kotlin-style `suspend` marker described in the spec.
#[proc_macro_attribute]
pub fn suspend(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    TokenStream::from(quote! { #input })
}

/// Macro that wraps an async block in a `Suspending` helper.
#[proc_macro]
pub fn suspend_block(input: TokenStream) -> TokenStream {
    let block: proc_macro2::TokenStream = input.into();
    TokenStream::from(quote! {
        rs_coroutine_core::suspending::Suspending::from_async_block(async move { #block })
    })
}
