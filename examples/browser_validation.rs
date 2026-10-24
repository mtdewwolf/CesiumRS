//! Run by tools/cesium/browser-check.mjs in Chromium.
#[cfg(target_arch = "wasm32")]
mod browser {
    use cesium_rs::core::browser_host::*;
    use cesium_rs::core::clock::ClockOptions;
    use cesium_rs::core::*;
    use js_sys::Reflect;
    use serde_json::json;
    use std::{cell::RefCell, rc::Rc};
    use wasm_bindgen::prelude::*;
    fn err(e: impl std::fmt::Display) -> JsValue {
        JsValue::from_str(&e.to_string())
    }
    #[wasm_bindgen]
    pub fn validate_property_clock() -> Result<(), JsValue> {
        use cesium_rs::data_sources::*;
        let now = JulianDate::from_date(js_sys::Date::now());
        let callback: PropertyCallback<f64> =
            Rc::new(move |time, _| Ok(Some(JulianDate::seconds_difference(time, &now))));
        let p = Rc::new(CallbackProperty::new(callback, false));
        let value = p.get_value(None, None).map_err(err)?.unwrap();
        assert!((0. ..2.).contains(&value));
        let array = PropertyArray::new(Some(vec![p.clone(), p.clone()]));
        let values = array.get_value(None, None).map_err(err)?.unwrap();
        assert_eq!(values[0], values[1]);
        let samples = SampledProperty::<f64>::new(None);
        samples.add_samples(&[now.add_seconds(-60.), now.add_seconds(60.)], &[-60., 60.], None).map_err(err)?;
        assert!((0. ..2.).contains(&samples.get_value(None, None).map_err(err)?.unwrap()));
        let intervals = TimeIntervalCollectionProperty::new();
        intervals.intervals().add_interval(TimeInterval::new(
            now.add_seconds(-60.),
            now.add_seconds(60.),
            true,
            true,
            Some(Rc::new(42.)),
        ));
        assert_eq!(intervals.get_value(None, None).map_err(err)?, Some(42.));
        let composite = CompositeProperty::new();
        composite.intervals().add_interval(TimeInterval::new(
            now.add_seconds(-60.),
            now.add_seconds(60.),
            true,
            true,
            Some(p as PropertyRef<f64>),
        ));
        assert!((0. ..2.).contains(&composite.get_value(None, None).map_err(err)?.unwrap()));
        Ok(())
    }
    #[wasm_bindgen]
    pub async fn validate_workers(base_url: String) -> Result<(), JsValue> {
        let mut processor = TaskProcessor::new(Rc::new(BrowserTaskWorker::new("echo", base_url)), Some(1));
        let init = processor.init_web_assembly_module(WebAssemblyOptions {
            wasm_binary_file: Some("tests/data/ktx2/Green4x4.ktx2".into()),
            ..Default::default()
        });
        assert_eq!(init.await.map_err(err)?, json!(true));
        let task = processor.schedule_task(json!({"value":42})).unwrap();
        assert!(processor.schedule_task(json!(1)).is_none());
        assert_eq!(task.await.map_err(err)?, json!({"value":42}));
        assert!(processor.schedule_task(json!({"fail":true})).unwrap().await.is_err());
        let dropped = processor.schedule_task(json!({"pending": true})).unwrap();
        drop(dropped);
        let pending = processor.schedule_task(json!({"pending":true})).unwrap();
        processor.destroy();
        assert!(pending.await.is_err());
        Ok(())
    }
    #[wasm_bindgen]
    pub async fn validate_dropped_io(base_url: String) -> Result<(), JsValue> {
        use cesium_rs::core::image_helpers::FetchImageOptions;
        use cesium_rs::core::resource::{HttpRequest, RequestKind, ResourceTransport};
        let transport = BrowserTransport::default();
        let response = transport.execute(HttpRequest {
            id: 99,
            kind: RequestKind::Http,
            url: format!("{base_url}slow"),
            method: "GET".into(),
            headers: vec![],
            body: None,
            with_credentials: false,
        });
        drop(response);
        let image_url = format!("{base_url}slow");
        let image = BrowserImageHost
            .load_resource_url(&image_url, false, FetchImageOptions::default())
            .unwrap();
        drop(image);
        Ok(())
    }
    #[wasm_bindgen]
    pub fn validate_video(
        video: web_sys::HtmlVideoElement,
        replacement: web_sys::HtmlVideoElement,
    ) -> Result<(), JsValue> {
        let epoch = JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap();
        let clock = Rc::new(RefCell::new(
            Clock::new_at(
                ClockOptions { current_time: Some(epoch), should_animate: Some(false), ..Default::default() },
                0.,
            )
            .unwrap(),
        ));
        let mut binding = BrowserVideoBinding::new(BrowserVideoOptions {
            clock: Some(clock.clone()),
            element: Some(video.clone()),
            epoch: Some(epoch),
            tolerance: Some(0.5),
        })
        .map_err(err)?;
        assert_eq!(clock.borrow().on_tick_state.number_of_listeners(), 1);
        let tick = |seconds| {
            let mut c = clock.borrow_mut();
            c.set_current_time(epoch.add_seconds(seconds));
            c.tick_at(seconds * 1000.);
        };
        tick(5.);
        assert_eq!(video.current_time(), 5.);
        tick(7.);
        assert_eq!(video.current_time(), 5.);
        video.dispatch_event(&web_sys::Event::new("seeked")?)?;
        tick(7.);
        assert_eq!(video.current_time(), 5.);
        tick(7.);
        assert_eq!(video.current_time(), 7.);
        binding.set_element(Some(replacement.clone())).map_err(err)?;
        // The old element's listener must no longer advance the new synchronizer.
        tick(10.);
        assert_eq!(replacement.current_time(), 10.);
        video.dispatch_event(&web_sys::Event::new("seeked")?)?;
        tick(12.);
        assert_eq!(replacement.current_time(), 10.);
        binding.set_clock(None);
        assert_eq!(clock.borrow().on_tick_state.number_of_listeners(), 0);
        binding.set_clock(Some(clock.clone()));
        binding.destroy();
        assert_eq!(clock.borrow().on_tick_state.number_of_listeners(), 0);
        let before = Reflect::get(replacement.as_ref(), &"currentTime".into())?;
        tick(14.);
        assert_eq!(Reflect::get(replacement.as_ref(), &"currentTime".into())?, before);
        Ok(())
    }
}
