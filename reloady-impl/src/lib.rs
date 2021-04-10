/*
 * reloady - Simple, performant hot-reloading for Rust.
 * Copyright (C) 2021 the reloady authors
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */
use quote::{format_ident, quote};
use syn::{parse_macro_input, spanned::Spanned, FnArg, Pat, Signature};

#[cfg(feature = "enabled")]
#[proc_macro]
pub fn init(_args: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let crate_name = std::env::var("CARGO_PKG_NAME").unwrap();
    // this is probably a terrible idea, but it works!

    let name_lit = syn::Lit::Str(syn::LitStr::new(
        &crate_name,
        proc_macro2::Span::call_site(),
    ));
    let res = quote! {
        #[cfg_attr(not(target_os = "windows"), link_args = "-export-dynamic")]
        extern {}
        reloady::init2(#name_lit, env!("CARGO_MANIFEST_DIR"))
    };
    res.into()
}

#[cfg(not(feature = "enabled"))]
#[proc_macro]
pub fn init(_: proc_macro::TokenStream) -> proc_macro::TokenStream {
    (quote! {}).into()
}

#[cfg(feature = "enabled")]
#[proc_macro_attribute]
pub fn hot_reload(
    _args: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as syn::ItemFn);
    let new_sig = {
        let mut s = input.sig.clone();
        s.ident = format_ident!("__{}_fn_impl", s.ident);
        s
    };
    let fn_ty = sig_as_fn_type(input.sig.clone());
    let new_ident_lit = syn::Lit::Str(syn::LitStr::new(
        &new_sig.ident.to_string(),
        new_sig.ident.span(),
    ));
    let impl_ident = new_sig.ident.clone();
    let lock_ident = format_ident!(
        "__{}_FN_MUTEX",
        input.sig.ident.to_string().to_uppercase(),
        span = input.sig.ident.span()
    );
    let block = input.block;
    let (wrapped_sig, arg_names) = transform_argnames(input.sig.clone());
    // hash the wrapped_sig
    let sig_hash = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        wrapped_sig.hash(&mut hasher);
        hasher.finish()
    };
    let sig_hash_lit = syn::Lit::Int(syn::LitInt::new(&sig_hash.to_string(), wrapped_sig.span()));
    let sig_hash_ident = format_ident!("{}__reloady_sighash", new_sig.ident);
    #[cfg(target_os = "windows")]
    let ex_str = format!(
        "/EXPORT:{0}={0} /EXPORT:{1}={1}",
        new_sig.ident, sig_hash_ident
    );
    #[cfg(not(target_os = "windows"))]
    let ex_str = "".to_string();

    #[cfg(feature = "unstub")]
    let output = quote! {
        #[cfg_attr(target_os = "windows", link_args = #ex_str)]
        extern {}
        #[allow(non_snake_case)]
        #[cfg_attr(target_os = "windows", no_mangle)]
        #[linkage = "external"]
        #[inline(never)]
        fn #sig_hash_ident() -> u64 { #sig_hash_lit }
        #[cfg_attr(target_os = "windows", no_mangle)]
        #[linkage = "external"]
        #[inline(never)]
        #new_sig #block
        reloady::lazy_static! {
            #[allow(non_upper_case_globals)]
            static ref #lock_ident: std::sync::Mutex<#fn_ty> = std::sync::Mutex::new(#impl_ident);
        }
        #wrapped_sig {
            reloady::__update_fn(#new_ident_lit, std::module_path!(), #sig_hash_lit, &#lock_ident);
            let f = #lock_ident.lock().unwrap();
            (*f)(#arg_names)
        }
    };
    #[cfg(not(feature = "unstub"))]
    let output = quote! {
        #[cfg_attr(target_os = "windows", link_args = #ex_str)]
        extern {}
        #[allow(non_snake_case)]
        #[cfg_attr(target_os = "windows", no_mangle)]
        #[linkage = "external"]
        #[inline(never)]
        fn #sig_hash_ident() -> u64 { #sig_hash_lit }
        #[cfg_attr(target_os = "windows", no_mangle)]
        #[linkage = "external"]
        #[inline(never)]
        #new_sig #block
        #wrapped_sig {
            loop {}
        }
    };

    output.into()
}

#[cfg(not(feature = "enabled"))]
#[proc_macro_attribute]
pub fn hot_reload(
    _args: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    input
}

fn transform_argnames(mut sig: Signature) -> (Signature, proc_macro2::TokenStream) {
    let arg_names: Vec<syn::Ident> = (0..sig.inputs.len())
        .map(|x| format_ident!("_arg{}", x))
        .collect();
    for (i, arg) in sig.inputs.iter_mut().enumerate() {
        match arg {
            FnArg::Typed(typed) => match *typed.pat {
                Pat::Ident(ref mut ident) => {
                    ident.ident = arg_names[i].clone();
                }
                _ => {}
            },
            _ => {}
        }
    }
    (sig, quote! { #(#arg_names),* })
}

fn sig_as_fn_type(sig: Signature) -> proc_macro2::TokenStream {
    let (generic_params, where_clause) = (sig.generics.params, sig.generics.where_clause);
    let (sig_args, sig_async, sig_abi, sig_unsafe, sig_ret) =
        (sig.inputs, sig.asyncness, sig.abi, sig.unsafety, sig.output);
    let ret = quote! {
        for<#generic_params> #sig_unsafe #sig_abi #sig_async fn(#sig_args) #sig_ret #where_clause
    };
    // println!("{}", ret);
    ret
}
