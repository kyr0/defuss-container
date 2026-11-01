//! Network check of the real base image, ignored by default: `cargo test --test registry --
//! --ignored`. It proves the pinned tag still resolves for this architecture and still ships Bun at
//! the path the guest uses.
use defuss_container::oci;
#[test]
#[ignore = "downloads the real pinned Bun base; explicit network verification"]
fn pinned_bun_base_is_available_and_contains_runtime() {
    let t = tempfile::tempdir().unwrap();
    let base = oci::pull(t.path()).unwrap();
    let mut found = false;
    for l in &base.layers {
        let path = t
            .path()
            .join(l["digest"].as_str().unwrap().trim_start_matches("sha256:"));
        let file = std::fs::File::open(path).unwrap();
        let reader: Box<dyn std::io::Read> = if l["mediaType"].as_str().unwrap().ends_with("gzip") {
            Box::new(flate2::read::GzDecoder::new(file))
        } else {
            Box::new(file)
        };
        for e in tar::Archive::new(reader).entries().unwrap() {
            let e = e.unwrap();
            if e.path().unwrap() == std::path::Path::new("usr/local/bin/bun") {
                found = true;
            }
        }
    }
    assert!(found, "pinned base must contain /usr/local/bin/bun");
    println!(
        "Verified {} {} {}",
        defuss_container::BASE_IMAGE,
        base.architecture,
        base.digest
    );
}
