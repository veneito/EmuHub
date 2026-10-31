fn main() {
    #[cfg(windows)]
    winresource::WindowsResource::new().set_icon("assets/emuhub.ico").compile().expect("embed the Windows icon");
}
