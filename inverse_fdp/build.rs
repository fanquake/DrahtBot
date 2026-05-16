// Requires:
// cargo clean --package=inverse_fdp && cargo test && cargo run
//
// But first, insert the generated code below the comment `// Insert below` in the main function
extern crate cpp_build;
fn main() {
    cpp_build::build("src/main.rs");
}
