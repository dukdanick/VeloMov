#[test]
fn wgsl_validates() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/engine/shaders.wgsl")).unwrap();
    let module = match naga::front::wgsl::parse_str(&src) {
        Ok(m) => m,
        Err(e) => panic!("{}", e.emit_to_string(&src)),
    };
    let mut v = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all());
    if let Err(e) = v.validate(&module) {
        panic!("{}", e.emit_to_string(&src));
    }
}
