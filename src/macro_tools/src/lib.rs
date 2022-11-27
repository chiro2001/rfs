extern crate proc_macro;

use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn;
use syn::*;
use syn::__private::Span;
use syn::parse_macro_input::ParseMacroInput;
use syn::spanned::Spanned;

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

#[proc_macro_derive(apply_mem, attributes(apply_mem_to))]
pub fn apply_mem_derive(input: TokenStream) -> TokenStream {
    // let ast: syn::DeriveInput = syn::parse(input.clone()).unwrap();
    let ast = parse_macro_input!(input as DeriveInput);
    let attr = &ast.attrs[0];
    let target = attr.tokens.to_string();
    // let target = &attr.into_token_stream().to_string()[1..-1];
    let target = &target.as_str()[1..(target.len() - 1)];
    let tokens = attr.into_token_stream();
    // Field::try_from("Tets");
    // let ast2 = parse_macro_input!(tokens as DeriveInput);
    // let ast2 = syn::parse(attrs.clone()).unwrap();
    let fields_punct = match ast.data {
        Data::Struct(DataStruct {
                         fields: Fields::Named(fields),
                         ..
                     }) => fields.named,
        _ => panic!("Only structs with named fields can be annotated with ToUrl"),
    };
    let fields = fields_punct.iter().map(|field| {
        let field_ident = field.ident.as_ref().unwrap();
        quote! { that.#field_ident = self.#field_ident; }
    });
    // let data = &ast.generics;
    let name = &ast.ident;
    let target_itent = &Ident::new(target, attr.span());
    let gen = quote! {
        impl #name {
            pub fn apply_mem(self: &Self, that: &mut #target_itent) {
                #(#fields)*;
            }
        }
    };
    gen.into()
}

#[proc_macro_derive(AnswerFn)]
pub fn derive_answer_fn(_item: TokenStream) -> TokenStream {
    "fn answer() -> u32 { 42 }".parse().unwrap()
}