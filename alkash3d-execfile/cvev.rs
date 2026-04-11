// build.rs
fn main() {
    // Указываем линковку с alkash3d_rs.lib
    println!("cargo:rustc-link-search=.");
    println!("cargo:rustc-link-lib=alkash3d_rs");
}