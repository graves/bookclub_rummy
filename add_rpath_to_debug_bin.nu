#!/opt/homebrew/bin/nu

rm target/debug/rummy
cargo build
install_name_tool -add_rpath $env.DYLD_LIBRARY_PATH target/debug/rummy
