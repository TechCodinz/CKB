use ckb_core::OtlpReceiver;
use ckb_core::types::NodeId;

#[test]
fn same_named_functions_across_services_never_collapse_without_source_paths() {
    let payload = r#"{
      "resourceSpans": [
        {"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"billing"}}]},"scopeSpans":[{"spans":[
          {"name":"work","startTimeUnixNano":"0","endTimeUnixNano":"1000000","attributes":[
            {"key":"service.name","value":{"stringValue":"billing"}},
            {"key":"code.function.name","value":{"stringValue":"work"}}
          ],"status":{"code":1}}
        ]}]},
        {"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"shipping"}}]},"scopeSpans":[{"spans":[
          {"name":"work","startTimeUnixNano":"0","endTimeUnixNano":"2000000","attributes":[
            {"key":"service.name","value":{"stringValue":"shipping"}},
            {"key":"code.function.name","value":{"stringValue":"work"}}
          ],"status":{"code":1}}
        ]}]}
      ]
    }"#;

    let metrics = OtlpReceiver::ingest_spans(payload).unwrap();
    assert_eq!(metrics.len(), 2);
    assert!(metrics.keys().all(OtlpReceiver::is_unresolved_runtime_identity));
    assert!(metrics.contains_key(&NodeId("runtime-unresolved/namespace-function/billing/work".into())));
    assert!(metrics.contains_key(&NodeId("runtime-unresolved/namespace-function/shipping/work".into())));
    assert_eq!(OtlpReceiver::summarize(&metrics).nodes_updated, 0);
    assert_eq!(OtlpReceiver::summarize(&metrics).unresolved_runtime_identities, 2);
}

#[test]
fn exact_path_and_function_are_the_only_direct_source_attachment_case() {
    let payload = r#"[
      {"name":"work","startTimeUnixNano":"0","endTimeUnixNano":"3000000","attributes":[
        {"key":"service.name","value":{"stringValue":"billing"}},
        {"key":"code.file.path","value":{"stringValue":"src/billing/worker.ts"}},
        {"key":"code.function.name","value":{"stringValue":"work"}}
      ],"status":{"code":1}},
      {"name":"work","startTimeUnixNano":"0","endTimeUnixNano":"4000000","attributes":[
        {"key":"service.name","value":{"stringValue":"shipping"}},
        {"key":"code.file.path","value":{"stringValue":"src/shipping/worker.ts"}},
        {"key":"code.function.name","value":{"stringValue":"work"}}
      ],"status":{"code":1}}
    ]"#;

    let metrics = OtlpReceiver::ingest_spans(payload).unwrap();
    let billing = NodeId("src/billing/worker.ts::work".into());
    let shipping = NodeId("src/shipping/worker.ts::work".into());
    assert!(metrics.contains_key(&billing));
    assert!(metrics.contains_key(&shipping));
    assert!(!OtlpReceiver::is_unresolved_runtime_identity(&billing));
    assert!(!OtlpReceiver::is_unresolved_runtime_identity(&shipping));
    assert_eq!(OtlpReceiver::summarize(&metrics).nodes_updated, 2);
    assert_eq!(OtlpReceiver::summarize(&metrics).unresolved_runtime_identities, 0);
}
