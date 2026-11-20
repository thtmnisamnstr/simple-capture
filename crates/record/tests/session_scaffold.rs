use sc_record::Session;

#[test]
fn contract_types_link() {
    // Session cannot be built without a working platform yet; check the contract types link.
    let _ = std::mem::size_of::<Session>();
    assert_eq!(
        sc_record::Tuning::default().switch_first_frame_timeout_ms,
        1_500
    );
}
