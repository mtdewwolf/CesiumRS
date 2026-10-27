//! Parity and lifecycle tests for Scene `ImageryLayer` and `ImageryLayerCollection`.

mod common;


use cesium_rs::core::*;
use cesium_rs::renderer::*;
use cesium_rs::scene::*;
use std::sync::{Arc, Mutex};
use common::*;
use serde_json::{json, Value};

#[test]
fn imagery_layer_fixture_parity() {
    run(
        "scene",
        "ImageryLayer",
        |_| TIGHT,
        |f, a| match f {
            "defaults" => {
                let layer = ImageryLayer::new(None, ImageryLayerOptions::default());
                let rect = layer.rectangle();
                Some(Out::Ok(json!({
                    "brightness": layer.brightness,
                    "contrast": layer.contrast,
                    "hue": layer.hue,
                    "saturation": layer.saturation,
                    "gamma": layer.gamma,
                    "splitDirection": layer.split_direction as i32,
                    "minificationFilter": layer.minification_filter as i32,
                    "magnificationFilter": layer.magnification_filter as i32,
                    "show": layer.show,
                    "alpha": layer.alpha,
                    "nightAlpha": layer.night_alpha,
                    "dayAlpha": layer.day_alpha,
                    "colorToAlphaThreshold": layer.color_to_alpha_threshold,
                    "rectangle": [rect.west, rect.south, rect.east, rect.north],
                    "isBaseLayer": layer.is_base_layer(),
                })))
            }
            "options" => {
                let opt_json = &a[0];
                let mut options = ImageryLayerOptions::default();
                if let Some(alpha) = opt_json.get("alpha").and_then(Value::as_f64) {
                    options.alpha = Some(alpha);
                }
                if let Some(night_alpha) = opt_json.get("nightAlpha").and_then(Value::as_f64) {
                    options.night_alpha = Some(night_alpha);
                }
                if let Some(day_alpha) = opt_json.get("dayAlpha").and_then(Value::as_f64) {
                    options.day_alpha = Some(day_alpha);
                }
                if let Some(brightness) = opt_json.get("brightness").and_then(Value::as_f64) {
                    options.brightness = Some(brightness);
                }
                if let Some(contrast) = opt_json.get("contrast").and_then(Value::as_f64) {
                    options.contrast = Some(contrast);
                }
                if let Some(hue) = opt_json.get("hue").and_then(Value::as_f64) {
                    options.hue = Some(hue);
                }
                if let Some(saturation) = opt_json.get("saturation").and_then(Value::as_f64) {
                    options.saturation = Some(saturation);
                }
                if let Some(gamma) = opt_json.get("gamma").and_then(Value::as_f64) {
                    options.gamma = Some(gamma);
                }
                if let Some(show) = opt_json.get("show").and_then(Value::as_bool) {
                    options.show = Some(show);
                }
                if let Some(split) = opt_json.get("splitDirection").and_then(Value::as_i64) {
                    options.split_direction = match split {
                        -1 => Some(SplitDirection::Left),
                        1 => Some(SplitDirection::Right),
                        _ => Some(SplitDirection::None),
                    };
                }
                if let Some(f) = opt_json.get("minificationFilter").and_then(Value::as_i64) {
                    options.minification_filter = match f {
                        9728 => Some(TextureMinificationFilter::Nearest),
                        _ => Some(TextureMinificationFilter::Linear),
                    };
                }
                if let Some(f) = opt_json.get("magnificationFilter").and_then(Value::as_i64) {
                    options.magnification_filter = match f {
                        9728 => Some(TextureMagnificationFilter::Nearest),
                        _ => Some(TextureMagnificationFilter::Linear),
                    };
                }
                if let Some(th) = opt_json.get("colorToAlphaThreshold").and_then(Value::as_f64) {
                    options.color_to_alpha_threshold = Some(th);
                }
                if let Some(r) = opt_json.get("rectangle").and_then(Value::as_array) {
                    let w = r[0].as_f64().unwrap();
                    let s = r[1].as_f64().unwrap();
                    let e = r[2].as_f64().unwrap();
                    let n = r[3].as_f64().unwrap();
                    options.rectangle = Some(Rectangle::new(w, s, e, n));
                }
                if let Some(r) = opt_json.get("cutoutRectangle").and_then(Value::as_array) {
                    let w = r[0].as_f64().unwrap();
                    let s = r[1].as_f64().unwrap();
                    let e = r[2].as_f64().unwrap();
                    let n = r[3].as_f64().unwrap();
                    options.cutout_rectangle = Some(Rectangle::new(w, s, e, n));
                }
                if let Some(c) = opt_json.get("colorToAlpha").and_then(Value::as_array) {
                    let red = c[0].as_f64().unwrap();
                    let green = c[1].as_f64().unwrap();
                    let blue = c[2].as_f64().unwrap();
                    let alpha = c[3].as_f64().unwrap();
                    options.color_to_alpha = Some(Color::new(red, green, blue, alpha));
                }
                if let Some(min_t) = opt_json.get("minimumTerrainLevel").and_then(Value::as_i64) {
                    options.minimum_terrain_level = Some(min_t as u32);
                }
                if let Some(max_t) = opt_json.get("maximumTerrainLevel").and_then(Value::as_i64) {
                    options.maximum_terrain_level = Some(max_t as u32);
                }

                let layer = ImageryLayer::new(None, options);
                let rect = layer.rectangle();
                let cutout = layer.cutout_rectangle.map(|c| json!([c.west, c.south, c.east, c.north])).unwrap_or(Value::Null);
                let color_to_alpha = layer.color_to_alpha.map(|c| json!([c.red, c.green, c.blue, c.alpha])).unwrap_or(Value::Null);

                Some(Out::Ok(json!({
                    "alpha": layer.alpha,
                    "nightAlpha": layer.night_alpha,
                    "dayAlpha": layer.day_alpha,
                    "brightness": layer.brightness,
                    "contrast": layer.contrast,
                    "hue": layer.hue,
                    "saturation": layer.saturation,
                    "gamma": layer.gamma,
                    "show": layer.show,
                    "splitDirection": layer.split_direction as i32,
                    "minificationFilter": layer.minification_filter as i32,
                    "magnificationFilter": layer.magnification_filter as i32,
                    "colorToAlphaThreshold": layer.color_to_alpha_threshold,
                    "rectangle": [rect.west, rect.south, rect.east, rect.north],
                    "cutoutRectangle": cutout,
                    "colorToAlpha": color_to_alpha,
                    "minimumTerrainLevel": layer.minimum_terrain_level,
                    "maximumTerrainLevel": layer.maximum_terrain_level,
                })))
            }
            "getImageryRectangle" => {
                let p_arr = a[0].as_array().unwrap();
                let l_arr = a[1].as_array().unwrap();
                let provider_rect = Rectangle::new(
                    p_arr[0].as_f64().unwrap(),
                    p_arr[1].as_f64().unwrap(),
                    p_arr[2].as_f64().unwrap(),
                    p_arr[3].as_f64().unwrap(),
                );
                let layer_rect = Rectangle::new(
                    l_arr[0].as_f64().unwrap(),
                    l_arr[1].as_f64().unwrap(),
                    l_arr[2].as_f64().unwrap(),
                    l_arr[3].as_f64().unwrap(),
                );
                let r = ImageryLayer::get_imagery_rectangle_for_rectangles(&provider_rect, &layer_rect);
                Some(Out::Ok(match r {
                    Some(rect) => json!([rect.west, rect.south, rect.east, rect.north]),
                    None => Value::Null,
                }))
            }
            "calculateTextureTranslationAndScale" => {
                let t_arr = a[0].as_array().unwrap();
                let i_arr = a[1].as_array().unwrap();
                let tile_rect = Rectangle::new(
                    t_arr[0].as_f64().unwrap(),
                    t_arr[1].as_f64().unwrap(),
                    t_arr[2].as_f64().unwrap(),
                    t_arr[3].as_f64().unwrap(),
                );
                let ready_rect = Rectangle::new(
                    i_arr[0].as_f64().unwrap(),
                    i_arr[1].as_f64().unwrap(),
                    i_arr[2].as_f64().unwrap(),
                    i_arr[3].as_f64().unwrap(),
                );
                let res = ImageryLayer::calculate_texture_translation_and_scale(&tile_rect, &ready_rect);
                Some(Out::Ok(json!([res.x, res.y, res.z, res.w])))
            }
            "getLevelWithMaximumTexelSpacing" => {
                let scheme_kind = a[0].as_str().unwrap();
                let tile_width = a[1].as_f64().unwrap();
                let texel_spacing = a[2].as_f64().unwrap();
                let lat = a[3].as_f64().unwrap();

                let res = if scheme_kind == "geographic" {
                    let tiling_scheme = GeographicTilingScheme::default();
                    ImageryLayer::compute_level_with_maximum_texel_spacing(&tiling_scheme, tile_width, texel_spacing, lat)
                } else {
                    let tiling_scheme = WebMercatorTilingScheme::default();
                    ImageryLayer::compute_level_with_maximum_texel_spacing(&tiling_scheme, tile_width, texel_spacing, lat)
                };
                Some(Out::Ok(json!(res)))
            }
            _ => None,
        },
    );
}

#[test]
fn imagery_layer_collection_fixture_parity() {
    run(
        "scene",
        "ImageryLayerCollection",
        |_| TIGHT,
        |f, _a| match f {
            "lifecycle" => {
                let mut col = ImageryLayerCollection::new();
                let l0 = ImageryLayer::handle(None, ImageryLayerOptions { alpha: Some(0.1), ..Default::default() });
                let l1 = ImageryLayer::handle(None, ImageryLayerOptions { alpha: Some(0.2), ..Default::default() });
                let l2 = ImageryLayer::handle(None, ImageryLayerOptions { alpha: Some(0.3), ..Default::default() });

                let events = Arc::new(Mutex::new(Vec::<Value>::new()));

                {
                    let events_clone = events.clone();
                    col.layer_added.add_event_listener(move |evt| {
                        events_clone.lock().unwrap().push(json!(["added", evt.layer.borrow().alpha, evt.index]));
                    });
                }
                {
                    let events_clone = events.clone();
                    col.layer_removed.add_event_listener(move |evt| {
                        events_clone.lock().unwrap().push(json!(["removed", evt.layer.borrow().alpha, evt.index]));
                    });
                }
                {
                    let events_clone = events.clone();
                    col.layer_moved.add_event_listener(move |evt| {
                        events_clone.lock().unwrap().push(json!(["moved", evt.layer.borrow().alpha, evt.new_index, evt.old_index]));
                    });
                }
                {
                    let events_clone = events.clone();
                    col.layer_shown_or_hidden.add_event_listener(move |evt| {
                        events_clone.lock().unwrap().push(json!(["shownOrHidden", evt.layer.borrow().alpha, evt.index, evt.show]));
                    });
                }

                col.add(l0.clone(), None).unwrap();
                col.add(l1.clone(), None).unwrap();
                col.add(l2.clone(), None).unwrap();

                let snap_state = |col: &ImageryLayerCollection| -> Value {
                    let arr: Vec<Value> = (0..col.len())
                        .map(|i| {
                            let lyr = col.get(i).unwrap();
                            let b = lyr.borrow();
                            json!([b.alpha, b.layer_index(), b.is_base_layer()])
                        })
                        .collect();
                    Value::Array(arr)
                };

                let state1 = snap_state(&col);

                // Reordering
                col.raise(&l0).unwrap();
                let state_after_raise = snap_state(&col);

                col.raise_to_top(&l0).unwrap();
                let state_after_raise_to_top = snap_state(&col);

                col.lower_to_bottom(&l0).unwrap();
                let state_after_lower_to_bottom = snap_state(&col);

                // Visibility & base layer
                l0.borrow_mut().show = false;
                col.update();
                let state_after_l0_hide = snap_state(&col);

                l1.borrow_mut().show = false;
                col.update();
                let state_after_l1_hide = snap_state(&col);

                l0.borrow_mut().show = true;
                col.update();
                let state_after_l0_show = snap_state(&col);

                // Contains and indexOf
                let unadded = ImageryLayer::handle(None, ImageryLayerOptions::default());
                let contains_check = json!([
                    col.contains(&l0),
                    col.contains(&unadded),
                    col.index_of(&l0).map(|i| i as i32).unwrap_or(-1),
                    col.index_of(&unadded).map(|i| i as i32).unwrap_or(-1),
                ]);

                // Remove
                let removed_l1 = col.remove(&l1, Some(false));
                let state_after_remove_l1 = snap_state(&col);

                // RemoveAll
                col.remove_all(Some(false));
                let len_after_remove_all = col.len();

                let all_events = events.lock().unwrap().clone();

                Some(Out::Ok(json!({
                    "state1": state1,
                    "stateAfterRaise": state_after_raise,
                    "stateAfterRaiseToTop": state_after_raise_to_top,
                    "stateAfterLowerToBottom": state_after_lower_to_bottom,
                    "stateAfterL0Hide": state_after_l0_hide,
                    "stateAfterL1Hide": state_after_l1_hide,
                    "stateAfterL0Show": state_after_l0_show,
                    "containsCheck": contains_check,
                    "removedL1": removed_l1,
                    "stateAfterRemoveL1": state_after_remove_l1,
                    "lenAfterRemoveAll": len_after_remove_all,
                    "events": all_events,
                })))
            }
            "addWithIndex" => {
                let mut col = ImageryLayerCollection::new();
                let l_a = ImageryLayer::handle(None, ImageryLayerOptions { alpha: Some(0.1), ..Default::default() });
                let l_b = ImageryLayer::handle(None, ImageryLayerOptions { alpha: Some(0.2), ..Default::default() });
                let l_c = ImageryLayer::handle(None, ImageryLayerOptions { alpha: Some(0.3), ..Default::default() });

                col.add(l_a, None).unwrap();
                col.add(l_b, Some(0)).unwrap(); // insert at front
                col.add(l_c, Some(1)).unwrap(); // insert in middle

                let arr: Vec<Value> = (0..col.len())
                    .map(|i| {
                        let lyr = col.get(i).unwrap();
                        let b = lyr.borrow();
                        json!([b.alpha, b.layer_index(), b.is_base_layer()])
                    })
                    .collect();
                Some(Out::Ok(Value::Array(arr)))
            }
            "swapBounds" => {
                let mut col = ImageryLayerCollection::new();
                let l0 = ImageryLayer::handle(None, ImageryLayerOptions { alpha: Some(0.1), ..Default::default() });
                let l1 = ImageryLayer::handle(None, ImageryLayerOptions { alpha: Some(0.2), ..Default::default() });
                col.add(l0.clone(), None).unwrap();
                col.add(l1.clone(), None).unwrap();

                // raise top layer: should be no-op
                col.raise(&l1).unwrap();
                let state1: Vec<f64> = (0..col.len()).map(|i| col.get(i).unwrap().borrow().alpha).collect();

                // lower bottom layer: should be no-op
                col.lower(&l0).unwrap();
                let state2: Vec<f64> = (0..col.len()).map(|i| col.get(i).unwrap().borrow().alpha).collect();

                Some(Out::Ok(json!({
                    "state1": state1,
                    "state2": state2,
                })))
            }
            _ => None,
        },
    );
}
