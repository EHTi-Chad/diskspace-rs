fn main() {
    // Embed the Windows icon resource into the exe
    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("ehti.ico");
        res.compile().expect("Failed to compile Windows resources");
    }
}
