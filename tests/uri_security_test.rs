use rustible::modules::uri::UriModule;
use rustible::modules::{Module, ModuleContext, ModuleParams};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use std::collections::HashMap;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_uri_module_max_content_length_exceeded() {
    let mock_server = MockServer::start().await;

    // Create a response body larger than the limit we'll set (100 bytes)
    // 200 bytes of 'a'
    let body = "a".repeat(200);

    Mock::given(method("GET"))
        .and(path("/large"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&mock_server)
        .await;

    let module = UriModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("url".to_string(), json!(format!("{}/large", mock_server.uri())));
    params.insert("max_content_length".to_string(), json!(100)); // Set limit to 100 bytes

    let context = ModuleContext::default();

    // Execute
    // Note: execute calls runtime.block_on internally, so we don't await it here.
    // However, since we are already in a tokio runtime (via #[tokio::test]),
    // the module's internal block_on might panic if it tries to start a new runtime.
    // Let's check implementation of execute.
    // It tries Handle::try_current() and spawns a thread if present.
    // So it should be fine.

    let result = module.execute(&params, &context);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("exceeds limit"));
}


#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_uri_module_within_limit() {
    let mock_server = MockServer::start().await;

    // Response body smaller than limit
    let body = "ok";

    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&mock_server)
        .await;

    let module = UriModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert("url".to_string(), json!(format!("{}/ok", mock_server.uri())));
    params.insert("max_content_length".to_string(), json!(100)); // Set limit to 100 bytes

    let context = ModuleContext::default();

    // Execute
    let result = module.execute(&params, &context);

    assert!(result.is_ok());
    let output = result.unwrap();
    // In strict mode, output might be JSON value, but here ModuleOutput struct
    // Check content
    let content = output.data.get("content").unwrap().as_str().unwrap();
    assert_eq!(content, "ok");
}
