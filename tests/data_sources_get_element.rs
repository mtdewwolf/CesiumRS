use cesium_rs::data_sources::{get_element, ElementInput};

#[test]
fn resolves_ids_and_returns_elements_directly() {
    assert_eq!(get_element(ElementInput::from("map"), |id| (id == "map").then_some(42)).unwrap(), 42);
    assert_eq!(get_element(ElementInput::Element(7), |_| panic!("direct elements need no lookup")).unwrap(), 7);
}

#[test]
fn missing_id_reports_the_requested_element() {
    let error = get_element::<(), _>(ElementInput::from("missing"), |_| None).unwrap_err();
    assert!(error.to_string().contains("missing"));
}
