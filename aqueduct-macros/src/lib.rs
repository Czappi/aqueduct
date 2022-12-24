pub(crate) mod task;
use proc_macro::TokenStream;
use task::{make_task, TaskType};

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


}
