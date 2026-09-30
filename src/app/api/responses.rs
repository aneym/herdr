use crate::api::schema::{ErrorBody, ErrorResponse, ResponseResult, SuccessResponse};

pub(crate) fn encode_success(id: String, result: ResponseResult) -> String {
    serde_json::to_string(&SuccessResponse { id, result }).unwrap()
}

pub(crate) fn encode_error(id: String, code: &str, message: impl Into<String>) -> String {
    encode_error_body(
        id,
        ErrorBody {
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
    let position = match outcome {
        crate::terminal::polite_send::SendOutcome::Queued(position) => Some(position),
        _ => None,
    };
    value["result"]["dropped"] =
        matches!(outcome, crate::terminal::polite_send::SendOutcome::Dropped).into();
    value["result"]["queued"] = position.is_some().into();
    if let Some(position) = position {
        value["result"]["queue_position"] = position.into();
    }
    value.to_string()
}
