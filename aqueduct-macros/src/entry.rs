use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::ItemFn;

pub fn make_entry(input: TokenStream) -> TokenStream {
    let ast: ItemFn = syn::parse(input).unwrap();

    let task_fn_name = ast.clone().sig.ident;
    let task_return_type = ast.sig.output;
    let task_input = ast.sig.inputs.to_token_stream();
    let fn_block = ast.block.to_token_stream();
    let fn_token = ast.sig.fn_token.to_token_stream();
    let visibility = ast.vis.to_token_stream();

    let output = quote! {
        #[tracing::instrument]
        #visibility #fn_token #task_fn_name (#task_input) #task_return_type {
            aqueduct::runtime::AQUEDUCT_RUNTIME.block_on( async move
                #fn_block
            )
        }
    };

    output.into()
}
