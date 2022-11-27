extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn;
use syn::*;
use syn::spanned::Spanned;

#[proc_macro_derive(ApplyMem, attributes(ApplyMemTo))]
pub fn apply_mem_derive(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    let attr = &ast.attrs[0];
    let target = attr.tokens.to_string();
    let target = &target.as_str()[1..(target.len() - 1)];
    let fields_punct = match ast.data {
        Data::Struct(DataStruct {
                         fields: Fields::Named(fields),
                         ..
                     }) => fields.named,
        _ => panic!("Only structs with named fields can be annotated with ToUrl"),
    };
    let fields_to = fields_punct.iter().map(|field| {
        let field_ident = field.ident.as_ref().unwrap();
        quote! { that.#field_ident = self.#field_ident; }
    });
    let fields_from = fields_punct.iter().map(|field| {
        let field_ident = field.ident.as_ref().unwrap();
        quote! { self.#field_ident = that.#field_ident; }
    });
    let name = &ast.ident;
    let target_itent = &Ident::new(target, attr.span());
    let gen = quote! {
        impl #name {
            pub fn apply_to(self: &Self, that: &mut #target_itent) {
                #(#fields_to)*;
            }
            pub fn apply_from(self: &mut Self, that: &#target_itent) {
                #(#fields_from)*;
            }
        }
    };
    gen.into()
}
