use crate::api::schema::{ErrorBody, ErrorResponse, ResponseResult, SuccessResponse};

pub(crate) fn encode_success(id: String, result: ResponseResult) -> String {
    serde_json::to_string(&SuccessResponse { id, result }).unwrap()
}

pub(crate) fn encode_error(id: String, code: &str, message: impl Into<String>) -> String {
    encode_error_body(
        id,
        ErrorBody {
            reason: None,
            code: code.into(),
            message: message.into(),
        },
    )
}

pub(super) fn encode_error_body(id: String, error: ErrorBody) -> String {
    serde_json::to_string(&ErrorResponse { id, error }).unwrap()
}

pub(crate) fn encode_send_accepted(
    id: String,
    outcome: crate::terminal::polite_send::SendOutcome,
) -> String {
    let mut value = serde_json::to_value(SuccessResponse {
        id,
        result: ResponseResult::Ok {},
    })
    .unwrap();
    value["result"]["id"] = outcome.id.into();
    value["result"]["state"] = serde_json::to_value(&outcome.state).unwrap();
    value["result"]["dropped"] =
        (outcome.state == crate::api::schema::PaneSendState::Dropped).into();
    value["result"]["queued"] = outcome.position.is_some().into();
    if let Some(position) = outcome.position {
        value["result"]["queue_position"] = position.into();
    }
    value.to_string()
}
