//! Print the unsigned `/webcast/im/fetch/` URL this crate builds, for cross-checking the
//! headless signer against the parameter set this crate builds.
fn main() {
    let preset = ttl_sign_core::Preset::new(
        ttl_sign_core::DevicePreset::chrome_linux(),
        ttl_sign_core::LocationPreset::us_east(),
        ttl_sign_core::ScreenPreset::FHD,
    );
    let room = std::env::args().nth(1).unwrap_or_else(|| "1".into());
    let mut params = ttl_sign_core::params::FetchParams::new(room);
    params.device_id = "7300000000000000001".into();
    println!("{}", params.url(&preset));
}
