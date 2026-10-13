//! The base protocol's messages (tsgo `lsproto/jsonrpc.go`,
//! `jsonrpc/jsonrpc.go` and `UnmarshalParams`).

use serde_json::value::RawValue;
use tsc_lsp::lsproto::message::decode_result;
use tsc_lsp::lsproto::*;

fn request(params: Option<&str>) -> RequestMessage {
    RequestMessage {
        id: Some(Id::Int(1)),
        method: "m".to_owned(),
        params: params.map(|text| RawValue::from_string(text.to_owned()).unwrap()),
    }
}

/// tsgo `TestUnmarshalParamsRequiresParams`: a method without params takes
/// none; any other an object or an array.
#[test]
fn params_are_required_by_shape() {
    for (params, error) in [
        (None, None),
        (
            Some("null"),
            Some("InvalidParams: expected no params, got null"),
        ),
        (
            Some("{}"),
            Some("InvalidParams: expected no params, got {}"),
        ),
    ] {
        let result = decode_params::<NoParams>(&request(params));
        assert_eq!(
            result.err().map(|error| error.message),
            error.map(str::to_owned),
            "{params:?}"
        );
    }
    for params in [None, Some("null"), Some("5"), Some(r#""x""#)] {
        let error = decode_params::<DidChangeConfigurationParams>(&request(params)).unwrap_err();
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert_eq!(
            error.message,
            "InvalidParams: params must be an object or array"
        );
    }
    let params =
        decode_params::<DidChangeConfigurationParams>(&request(Some(r#"{"settings":{"x":1}}"#)))
            .unwrap();
    assert_eq!(params.settings, serde_json::json!({"x": 1}));
}

#[test]
fn messages_are_requests_notifications_or_responses() {
    let Message::Request(request) =
        parse_message(br#"{"jsonrpc":"2.0","id":"a","method":"m","params":[1, 2]}"#).unwrap()
    else {
        panic!("not a request");
    };
    assert_eq!(request.id, Some(Id::String("a".to_owned())));
    assert_eq!(request.params.unwrap().get(), "[1, 2]");
    assert!(matches!(
        parse_message(br#"{"jsonrpc":"2.0","method":"m","id":null}"#).unwrap(),
        Message::Notification(_)
    ));
    // Go keeps a string id and an integer id in one value: "" is 0.
    let Message::Response(response) =
        parse_message(br#"{"jsonrpc":"2.0","id":"","error":{"code":-1,"message":"no"}}"#).unwrap()
    else {
        panic!("not a response");
    };
    assert_eq!(response.id, Some(Id::Int(0)));
    assert_eq!(response.error.unwrap().to_string(), "[-1]: no");
}

#[test]
fn the_version_must_be_2_0() {
    let error = parse_message(br#"{"jsonrpc":"1.0","id":1,"method":"m"}"#).unwrap_err();
    assert_eq!(error.code, ErrorCode::INVALID_REQUEST);
    assert_eq!(
        error.message,
        "InvalidRequest: json: cannot unmarshal JSON object into Go lsproto.Message: InvalidRequest: json: cannot unmarshal JSON string into Go jsonrpc.JSONRPCVersion within \"/jsonrpc\": invalid JSON-RPC version"
    );
    assert!(parse_message(br#"{"id":1,"method":"m"}"#).is_ok());
}

#[test]
fn outbound_messages_keep_tsgos_field_order() {
    let message = RequestMessage::new(
        Some(Id::String("ts1".to_owned())),
        method::WORKSPACE_CONFIGURATION,
        &ConfigurationParams {
            items: vec![ConfigurationItem {
                section: Some("js/ts".to_owned()),
                ..ConfigurationItem::default()
            }],
        },
    )
    .unwrap();
    assert_eq!(
        serde_json::to_string(&Message::Request(message)).unwrap(),
        r#"{"jsonrpc":"2.0","id":"ts1","method":"workspace/configuration","params":{"items":[{"section":"js/ts"}]}}"#
    );
    let shutdown = RequestMessage::new(Some(Id::Int(2)), method::SHUTDOWN, &NoParams).unwrap();
    assert_eq!(
        serde_json::to_string(&Message::Request(shutdown)).unwrap(),
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#
    );
}

/// tsgo `UnmarshalResult`: a response without a result is Go's
/// end-of-input error.
#[test]
fn results_decode_as_the_response_type() {
    let raw = RawValue::from_string("null".to_owned()).unwrap();
    assert_eq!(decode_result::<Null>(Some(&raw)), Ok(Null));
    assert_eq!(
        decode_result::<Null>(None),
        Err("jsontext: unexpected EOF".to_owned())
    );
}
