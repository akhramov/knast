use proc_macro::TokenStream;
use syn::ItemFn;

#[proc_macro_attribute]
pub fn jailed_test(_attrs: TokenStream, item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as ItemFn);
    let fn_name = input.sig.ident;
    let block = input.block;

    let body = quote::quote! {
        use std::io::Write;
        use test_helpers::nix::{
            sys::{
                signal::Signal,
                wait::{waitpid, WaitStatus},
            },
            unistd::{fork, ForkResult},
        };
        use test_helpers::jail::StoppedJail;
        use test_helpers::memmap::MmapMut;
        use test_helpers::bincode;

        let mut mmap = MmapMut::map_anon(1024)
            .expect("failed to create a mmap");

        let jail = StoppedJail::new("/")
            .param("vnet", jail::param::Value::Int(1))
            .param("children.max", jail::param::Value::Int(100))
            .start()
            .expect("Couldn't start jail");

        match unsafe { fork() } {
            Ok(ForkResult::Child) => {
                jail.attach().unwrap();
                let result = std::panic::catch_unwind(|| {
                    #block
                });

                if let Err(err) = result {
                    err.downcast_ref::<String>()
                        .and_then(|string| {
                            bincode::serialize(&format!("{:?}", string))
                                .and_then(|serialized| {
                                    Ok((&mut mmap[..]).write_all(&serialized[..])?)
                                }).ok()
                        }).unwrap_or(());
                    std::process::abort();
                };
            },
            Ok(ForkResult::Parent { child: child }) => {
                let status = waitpid(child, None)
                    .expect("failed to wait the child process");
                jail.defer_cleanup()
                    .expect("failed to defer jail clean up");

                match status {
                    WaitStatus::Exited(_, 0) => (),
                    WaitStatus::Signaled(_, Signal::SIGABRT, _) => {
                        let error: String = bincode::deserialize(&mmap).expect(
                            "Test failed, but result couldn't be deserialized"
                        );

                        panic!("{}", error);
                    },
                    status => {
                        panic!("Unexpected jailed process status {:?}", status);
                    }
                }
            },
            _ => panic!("Failed to fork"),
        }
    };

    quote::quote!(
        #[test]
        fn #fn_name() {
            #body
        }
    )
    .into()
}
