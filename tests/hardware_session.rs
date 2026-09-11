use selah::device::{DetectedDevice, DeviceSession, NormalizedLevel, discover};

const PRODUCT_ID_ENV: &str = "SELAH_HARDWARE_PRODUCT_ID";

#[test]
#[ignore = "claims a physical device; set SELAH_HARDWARE_PRODUCT_ID and run explicitly"]
fn opens_and_closes_selected_safe_interface() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Tokio runtime should start")
        .block_on(check_session());
}

#[test]
#[ignore = "claims a physical device; set SELAH_HARDWARE_PRODUCT_ID and run explicitly"]
fn sends_harmless_speaker_volume_request() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Tokio runtime should start")
        .block_on(check_speaker_volume());
}

#[test]
#[ignore = "claims a physical device; set SELAH_HARDWARE_PRODUCT_ID and run explicitly"]
fn sends_harmless_headphone_volume_request() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Tokio runtime should start")
        .block_on(check_headphone_volume());
}

async fn check_session() {
    let selected = select_single_device().await;

    let session = DeviceSession::open(&selected)
        .await
        .expect("safe session should open");
    session
        .close()
        .await
        .expect("safe session should close cleanly");
}

async fn check_speaker_volume() {
    let selected = select_single_device().await;

    let mut session = DeviceSession::open(&selected)
        .await
        .expect("safe session should open");
    let level = NormalizedLevel::new(0.1).expect("0.1 is a valid mixer level");
    session
        .set_speaker_level(level)
        .await
        .expect("speaker volume request should succeed");
    session
        .close()
        .await
        .expect("safe session should close cleanly");
}

async fn check_headphone_volume() {
    let selected = select_single_device().await;

    // Optional exact level for listening tests; defaults to the harmless 0.1.
    let level = std::env::var("SELAH_HARDWARE_LEVEL")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(0.1);
    let level = NormalizedLevel::new(level).expect("probe level must be 0.0..=1.0");

    let mut session = DeviceSession::open(&selected)
        .await
        .expect("safe session should open");
    session
        .set_headphone_level(level)
        .await
        .expect("headphone volume requests should succeed");
    session
        .close()
        .await
        .expect("safe session should close cleanly");
}

async fn select_single_device() -> DetectedDevice {
    let expected_product_id = std::env::var(PRODUCT_ID_ENV).unwrap_or_else(|_| {
        panic!("set {PRODUCT_ID_ENV} to the four-digit hexadecimal USB product ID")
    });
    assert!(
        expected_product_id.len() == 4
            && expected_product_id
                .chars()
                .all(|character| character.is_ascii_hexdigit()),
        "{PRODUCT_ID_ENV} must be four hexadecimal digits"
    );
    let expected_product_id = u16::from_str_radix(&expected_product_id, 16)
        .unwrap_or_else(|_| panic!("{PRODUCT_ID_ENV} must be four hexadecimal digits"));

    let report = discover().await.expect("USB discovery should succeed");
    let matching: Vec<_> = report
        .supported
        .iter()
        .filter(|device| device.model.product_id == expected_product_id)
        .collect();

    assert_eq!(
        matching.len(),
        1,
        "expected exactly one connected cataloged device with product ID {expected_product_id:04x}"
    );

    let selected = matching[0];
    assert!(
        selected.control_interface.is_some(),
        "the selected device must expose a non-audio control interface"
    );

    selected.clone()
}
