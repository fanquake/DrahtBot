// Requires:
// cargo clean --package=inverse_fdp && cargo test
//
// But first, insert the generated code below the comment `// Insert below` in the test
extern crate cpp_build;
fn main() {
    cpp_build::build("src/main.rs");
}
