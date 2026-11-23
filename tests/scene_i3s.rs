//! Regression and integration tests for the I3S Building Scene Layer cluster.

use cesium_rs::core::{
    HttpRequest, HttpResponse, Matrix4, RequestErrorEvent, RequestFuture, Resource, ResourceClient,
    ResourceTransport,
};
use cesium_rs::scene::{
    decode_i3s, I3SDataProvider, I3SDataProviderOptions, I3SGeometry, I3SLayer, I3SNode,
    LayerParent,
};
use futures_util::FutureExt;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::rc::Rc;

struct MapTransport {
    routes: BTreeMap<String, Vec<u8>>,
}

impl MapTransport {
    fn new() -> Self {
        Self { routes: BTreeMap::new() }
    }

    fn json(mut self, url: &str, value: Value) -> Self {
        self.routes.insert(url.to_string(), serde_json::to_vec(&value).unwrap());
        self
    }

    fn bytes(mut self, url: &str, data: Vec<u8>) -> Self {
        self.routes.insert(url.to_string(), data);
        self
    }
}

impl ResourceTransport for MapTransport {
    fn execute(&self, req: HttpRequest) -> RequestFuture<HttpResponse> {
        let url = req.url;
        let strip_query = url.split('?').next().unwrap_or(&url);
        let found = self
            .routes
            .get(&url)
            .or_else(|| self.routes.get(strip_query))
            .or_else(|| self.routes.get(strip_query.trim_end_matches('/')))
            .cloned();
        Box::pin(async move {
            match found {
                Some(body) => Ok(HttpResponse { status: 200, body, headers: vec![] }),
                None => Err(RequestErrorEvent {
                    status_code: Some(404),
                    response: Some(format!("404 not found: {url}").into_bytes()),
                    response_headers: None,
                }),
            }
        })
    }
}

/// Minimal uncompressed I3S binary geometry: 1 triangle (3 vertices), 1 feature,
/// position + normal + uv0 + color + uvRegion + faceRange.
fn sample_uncompressed_i3s_geometry() -> Vec<u8> {
    vec![
        3, 0, 0, 0, 1, 0, 0, 0, // vertexCount=3, featureCount=1
        // 3 positions (f32 x 3)
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 128, 63, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 128, 63, 0, 0, 0, 0,
    ]
}

#[test]
fn i3s_node_page_provider_preserves_rc_and_cache_and_child_indices() {
    let base = "https://i3s.test/SceneServer";
    let layer_json = json!({
        "id": 0,
        "layerType": "3DObject",
        "name": "TestLayer",
        "spatialReference": { "wkid": 4326 },
        "fullExtent": { "xmin": -122.5, "ymin": 37.7, "xmax": -122.4, "ymax": 37.8 },
        "store": { "version": "1.7" },
        "nodePages": {
            "nodesPerPage": 4,
            "rootIndex": 0,
            "lodSelectionMetricType": "maxScreenThreshold"
        }
    });
    let page0 = json!({
        "nodes": [
            {
                "index": 0,
                "children": [1],
                "obb": {
                    "center": [-122.45, 37.75, 10.0],
                    "halfSize": [50.0, 50.0, 20.0],
                    "quaternion": [0.0, 0.0, 0.0, 1.0]
                },
                "lodThreshold": 128.0
            },
            {
                "index": 1,
                "obb": {
                    "center": [-122.44, 37.76, 15.0],
                    "halfSize": [25.0, 25.0, 10.0],
                    "quaternion": [0.0, 0.0, 0.0, 1.0]
                },
                "lodThreshold": 64.0,
                "mesh": {
                    "geometry": { "definition": 0, "resource": 1 }
                }
            }
        ]
    });

    let transport = MapTransport::new()
        .json(base, layer_json)
        .json("https://i3s.test/SceneServer/layers/0/nodepages/0/", page0);
    let client = ResourceClient::new(Rc::new(transport));
    let resource = Resource::new(base).unwrap();

    let provider = I3SDataProvider::from_url(resource, I3SDataProviderOptions::default(), &client)
        .now_or_never()
        .unwrap()
        .expect("from_url should succeed without panicking");

    assert_eq!(provider.layers().len(), 1);
    let layer = provider.layers()[0].clone();

    // Provider weak references stay alive after from_url returns.
    assert!(layer.data_provider.upgrade().is_some());
    assert!(layer.root_node().as_ref().unwrap().data_provider().is_some());

    // Node-page cache hit returns the same object shape so subsequent lookups succeed.
    let cached_node1 = layer
        .get_node_in_node_pages(1, &client)
        .now_or_never()
        .unwrap()
        .expect("cached node page lookup should succeed");
    assert_eq!(cached_node1["index"].as_u64(), Some(1));

    // Numeric child references initialize node_index and load from node pages.
    {
        let mut root_guard = layer.root_node_mut();
        let root = root_guard.as_mut().unwrap();
        root.load_children(&client)
            .now_or_never()
            .unwrap()
            .expect("load_children should succeed");
        assert_eq!(root.children().len(), 1);
        assert_eq!(root.children()[0].node_index(), Some(1));
        assert_eq!(root.children()[0].data().and_then(|d| d["index"].as_u64()), Some(1));

        // Tile transform is column-major: translation lives in indices 12..15.
        let def = root.create_3d_tile_definition().expect("tile definition");
        let t: Vec<f64> = def["transform"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        assert_eq!(t.len(), 16);
        let translation_mag = (t[12] * t[12] + t[13] * t[13] + t[14] * t[14]).sqrt();
        assert!(translation_mag > 6_000_000.0, "ECEF translation must be in indices 12..14, got {t:?}");
        assert_eq!(t[3], 0.0);
        assert_eq!(t[7], 0.0);
        assert_eq!(t[11], 0.0);
        assert_eq!(t[15], 1.0);
        let _ = Matrix4::from_column_major_array([
            t[0], t[1], t[2], t[3], t[4], t[5], t[6], t[7], t[8], t[9], t[10], t[11], t[12], t[13], t[14], t[15],
        ]);
    }

    // Visibility propagation works through shared Rc without &mut provider.
    provider.set_show(false);
    assert!(!provider.show());
    assert!(!layer.tileset_mut().as_ref().unwrap().options.show);
}

#[test]
fn i3s_building_scene_layer_loads_nested_sublayers() {
    let base = "https://i3s.test/Building/SceneServer";
    let bsl_json = json!({
        "id": 0,
        "layerType": "Building",
        "name": "TestBuilding",
        "fullExtent": { "xmin": -122.5, "ymin": 37.7, "xmax": -122.4, "ymax": 37.8 },
        "sublayers": [
            {
                "id": 10,
                "layerType": "group",
                "name": "Architectural",
                "visibility": true,
                "sublayers": [
                    {
                        "id": 20,
                        "layerType": "3DObject",
                        "name": "Walls",
                        "visibility": true
                    }
                ]
            }
        ]
    });
    let sublayer_json = json!({
        "id": 20,
        "layerType": "3DObject",
        "name": "Walls",
        "spatialReference": { "wkid": 4326 },
        "store": { "version": "1.7" },
        "nodePages": { "nodesPerPage": 4, "rootIndex": 0, "lodSelectionMetricType": "maxScreenThreshold" }
    });
    let page0 = json!({
        "nodes": [
            {
                "index": 0,
                "obb": {
                    "center": [-122.45, 37.75, 10.0],
                    "halfSize": [10.0, 10.0, 10.0],
                    "quaternion": [0.0, 0.0, 0.0, 1.0]
                },
                "lodThreshold": 64.0
            }
        ]
    });

    let transport = MapTransport::new()
        .json(base, bsl_json)
        .json("https://i3s.test/Building/SceneServer/layers/0/sublayers/20/", sublayer_json)
        .json("https://i3s.test/Building/SceneServer/layers/0/sublayers/20/nodepages/0/", page0);
    let client = ResourceClient::new(Rc::new(transport));

    let provider = I3SDataProvider::from_url(Resource::new(base).unwrap(), I3SDataProviderOptions::default(), &client)
        .now_or_never()
        .unwrap()
        .expect("building scene layer should load without panicking");

    assert_eq!(provider.sublayers().len(), 1);
    let group = &provider.sublayers()[0];
    assert_eq!(group.sublayers().len(), 1);
    let walls = &group.sublayers()[0];
    assert!(walls.resource().is_some());
    assert_eq!(provider.layers().len(), 1);
}

#[test]
fn i3s_legacy_schema_decoding_and_gltf_generation() {
    let base = "https://i3s.test/Legacy/SceneServer";
    let layer_json = json!({
        "id": 0,
        "layerType": "3DObject",
        "name": "LegacyLayer",
        "spatialReference": { "wkid": 4326 },
        "store": {
            "version": "1.6",
            "rootNode": "nodes/root",
            "defaultGeometrySchema": {
                "ordering": ["position"],
                "featureAttributeOrder": []
            }
        }
    });
    let root_node_json = json!({
        "id": "root",
        "obb": {
            "center": [-122.45, 37.75, 10.0],
            "halfSize": [10.0, 10.0, 10.0],
            "quaternion": [0.0, 0.0, 0.0, 1.0]
        },
        "lodSelection": [{ "metricType": "maxScreenThreshold", "maxError": 64.0 }],
        "geometryData": [{ "href": "geometries/0" }]
    });

    let transport = MapTransport::new()
        .json(base, layer_json.clone())
        .json("https://i3s.test/Legacy/SceneServer/layers/0/nodes/root/", root_node_json)
        .bytes("https://i3s.test/Legacy/SceneServer/layers/0/nodes/root/geometries/0", sample_uncompressed_i3s_geometry());
    let client = ResourceClient::new(Rc::new(transport));

    let provider = I3SDataProvider::from_url(Resource::new(base).unwrap(), I3SDataProviderOptions::default(), &client)
        .now_or_never()
        .unwrap()
        .expect("legacy layer should load");
    let layer = provider.layers()[0].clone();

    let mut root_guard = layer.root_node_mut();
    let root = root_guard.as_mut().unwrap();
    root.load_geometry_data(&client).now_or_never().unwrap().unwrap();
    assert_eq!(root.geometry_data().len(), 1);
    assert!(root.geometry_data()[0].geometry_buffer_info.is_none());

    let schema = &layer_json["store"]["defaultGeometrySchema"];
    let mesh = decode_i3s("https://i3s.test/node/0", schema, root, None, None)
        .expect("legacy schema ordering should decode vertex attributes");
    assert!(!mesh.buffers.is_empty(), "decoded mesh must contain vertex buffers");

    let gltf = root.geometry_data()[0].generate_gltf(root, &mesh);
    assert!(gltf["scenes"].is_array(), "glTF scenes must be an array");
    let buffers = gltf["buffers"].as_array().expect("glTF buffers array");
    assert!(!buffers.is_empty());
    for buf in buffers {
        assert!(buf.is_object(), "each glTF buffer entry must be an object");
        assert!(buf["byteLength"].as_u64().unwrap_or(0) > 0);
        assert!(buf["uri"].as_str().unwrap_or_default().starts_with("data:application/octet-stream;base64,"));
    }

    let glb = I3SDataProvider::binarize_gltf(&gltf);
    assert_eq!(&glb[..4], b"glTF");
    assert_eq!(glb.len() % 4, 0, "GLB must be 4-byte aligned");

    // Direct smoke check on I3SLayer::new / I3SNode / I3SGeometry types.
    let _ = (I3SLayer::new, I3SNode::new_root, I3SGeometry::new, LayerParent::Provider);
}
