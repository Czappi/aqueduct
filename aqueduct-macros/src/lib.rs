use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::{token::Async, ReturnType};

#[proc_macro_attribute]
pub fn task(args: TokenStream, input: TokenStream) -> TokenStream {
    let _ = args;

    make_task(input, TaskType::Normal)
}

#[proc_macro_attribute]
pub fn blocking_task(args: TokenStream, input: TokenStream) -> TokenStream {
    let _ = args;

    make_task(input, TaskType::Blocking)
}

#[derive(PartialEq, Eq)]
enum TaskType {
    Normal,
    Blocking,
}

fn make_task(input: TokenStream, task_type: TaskType) -> TokenStream {
    let ast: syn::ItemFn = syn::parse(input).unwrap();

    if task_type == TaskType::Blocking {
        if let Some(asyncness) = ast.sig.asyncness {
            return syn::Error::new(asyncness.span, "Blocking task can't be async!")
                .to_compile_error()
                .into();
        }
    }

    let task_fn_name = ast.clone().sig.ident;
    let task_return_type = make_tokio_return_type(&ast.sig.output);
    let task_input = ast.sig.inputs.to_token_stream();
    let fn_block = ast.block.to_token_stream();
    let task_asyncness = make_task_asyncness(ast.sig.asyncness, &task_type);
    let runtime_method = make_runtime_method(&task_type);
    let fn_token = ast.sig.fn_token.to_token_stream();
    let visibility = ast.vis.to_token_stream();

    let output = quote! {
        #[tracing::instrument]
        #visibility #fn_token #task_fn_name (#task_input) #task_return_type {
            aqueduct::runtime::AQUEDUCT_RUNTIME. #runtime_method ( #task_asyncness
                #fn_block
            )
        }
    };

    output.into()
}

fn make_runtime_method(task_type: &TaskType) -> proc_macro2::TokenStream {
    match task_type {
        TaskType::Normal => quote!(spawn),
        TaskType::Blocking => quote!(spawn_blocking),
    }
}

fn make_tokio_return_type(return_type: &ReturnType) -> proc_macro2::TokenStream {
    match return_type {
        ReturnType::Default => quote! {
            -> tokio::task::JoinHandle<()>
        },
        ReturnType::Type(arrow, type_box) => quote! {
            #arrow tokio::task::JoinHandle<#type_box>
        },
    }
}

fn make_task_asyncness(asyncness: Option<Async>, task_type: &TaskType) -> proc_macro2::TokenStream {
    match task_type {
        TaskType::Normal => {
            if let Some(token) = asyncness {
                token.to_token_stream()
            } else {
                quote!(async)
            }
        }
        TaskType::Blocking => quote!(||),
    }
}
