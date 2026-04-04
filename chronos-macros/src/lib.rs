//! Procedural macros for Chronos simulation testing.
//!
//! Provides the `#[chronos::test]` attribute for writing simulation tests.

use proc_macro::TokenStream;
use quote::{quote, format_ident};
use syn::{parse_macro_input, ItemFn, parse::Parse, parse::ParseStream, Token, LitInt, LitStr, Ident};

/// Parsed arguments for the test macro.
struct TestArgs {
    seed: Option<u64>,
    strategy: Option<String>,
    timeout: Option<u64>,
    iterations: Option<u32>,
}

impl Default for TestArgs {
    fn default() -> Self {
        Self {
            seed: None,
            strategy: None,
            timeout: None,
            iterations: None,
        }
    }
}

impl Parse for TestArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut args = TestArgs::default();
        
        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            let _: Token![=] = input.parse()?;
            
            match ident.to_string().as_str() {
                "seed" => {
                    let lit: LitInt = input.parse()?;
                    args.seed = Some(lit.base10_parse()?);
                }
                "strategy" => {
                    let lit: LitStr = input.parse()?;
                    args.strategy = Some(lit.value());
                }
                "timeout" => {
                    let lit: LitInt = input.parse()?;
                    args.timeout = Some(lit.base10_parse()?);
                }
                "iterations" => {
                    let lit: LitInt = input.parse()?;
                    args.iterations = Some(lit.base10_parse()?);
                }
                _ => {
                    return Err(syn::Error::new(ident.span(), format!("unknown attribute: {}", ident)));
                }
            }
            
            if input.peek(Token![,]) {
                let _: Token![,] = input.parse()?;
            }
        }
        
        Ok(args)
    }
}

/// Attribute macro for Chronos simulation tests.
///
/// # Example
///
/// ```ignore
/// #[chronos::test]
/// async fn test_basic() {
///     // test code
/// }
///
/// #[chronos::test(seed = 42, iterations = 100)]
/// async fn test_with_options() {
///     // test code
/// }
/// ```
#[proc_macro_attribute]
pub fn test(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as TestArgs);
    let input_fn = parse_macro_input!(input as ItemFn);

    let fn_name = &input_fn.sig.ident;
    let fn_block = &input_fn.block;
    let fn_attrs = &input_fn.attrs;
    let fn_vis = &input_fn.vis;
    let is_async = input_fn.sig.asyncness.is_some();

    let iterations = args.iterations.unwrap_or(1);
    let timeout_secs = args.timeout.unwrap_or(60);
    
    let seed_expr = match args.seed {
        Some(s) => quote! { #s },
        None => quote! {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(42)
        },
    };

    let inner_fn_name = format_ident!("__chronos_inner_{}", fn_name);
    
    let inner_fn = if is_async {
        quote! {
            async fn #inner_fn_name() {
                #fn_block
            }
        }
    } else {
        quote! {
            fn #inner_fn_name() {
                #fn_block
            }
        }
    };

    let run_inner = if is_async {
        quote! {
            let rt = ::tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to create tokio runtime");
            rt.block_on(#inner_fn_name());
        }
    } else {
        quote! {
            #inner_fn_name();
        }
    };

    let expanded = quote! {
        #(#fn_attrs)*
        #[test]
        #fn_vis fn #fn_name() {
            #inner_fn

            let base_seed: u64 = #seed_expr;
            let iterations: u32 = #iterations;
            let _timeout_secs: u64 = #timeout_secs;

            for i in 0..iterations {
                let seed = base_seed.wrapping_add(i as u64);
                let ctx = ::chronos::sim::SimContext::new(seed);
                ctx.install();

                let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                    #run_inner
                }));

                ::chronos::sim::SimContext::uninstall();

                if let Err(e) = result {
                    eprintln!("chronos::test '{}' failed at iteration {} with seed {}", 
                        stringify!(#fn_name), i, seed);
                    eprintln!("Replay: #[chronos::test(seed = {})]", seed);
                    ::std::panic::resume_unwind(e);
                }
            }

            if iterations > 1 {
                eprintln!("chronos::test '{}' passed {} iterations", stringify!(#fn_name), iterations);
            }
        }
    };

    TokenStream::from(expanded)
}
