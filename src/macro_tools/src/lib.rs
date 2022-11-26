extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn;

// fn impl_hello_macro(ast: &syn::DeriveInput) -> TokenStream {
//     let name = &ast.ident;
//     let gen = quote! {
//         impl HelloMacro for #name {
//             fn hello_macro() {
//                 println!("Hello, Macro! My name is {}!", stringify!(#name));
//             }
//         }
//     };
//     gen.into()
// }

#[proc_macro_derive(apply_mem)]
pub fn apply_mem_derive(input: TokenStream) -> TokenStream {
    // let ast = syn::parse(input).unwrap();
    // impl_hello_macro(&ast)
    "fn answer() -> u32 { 42 }".parse().unwrap()
}

#[proc_macro_derive(AnswerFn)]
pub fn derive_answer_fn(_item: TokenStream) -> TokenStream {
    "fn answer() -> u32 { 42 }".parse().unwrap()
}