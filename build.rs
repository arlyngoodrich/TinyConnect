fn main() {
    println!("cargo:rerun-if-changed=assets/brand/tinyconnect.ico");

    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/brand/tinyconnect.ico");
        resource
            .compile()
            .expect("failed to embed the TinyConnect Windows icon");
    }
}
