use super::*;
use std::time::Duration;

fn lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        && value.bytes().any(|b| b != b'0')
}

#[test]
fn traceparent_is_w3c_version_00_with_nonzero_ids_and_sampled_flag() {
    let context = TraceContext::new();
    let header = context.traceparent();
    let parts: Vec<_> = header.to_str().unwrap().split('-').collect();
    assert_eq!(parts.len(), 4);
    assert_eq!(parts[0], "00");
    assert!(lower_hex(parts[1], 32));
    assert!(lower_hex(parts[2], 16));
    assert_eq!(parts[3], "01");
    assert_eq!(parts[1], context.trace_id());
    assert_ne!(TraceContext::new().trace_id(), context.trace_id());
}

impl Wire {
    pub(crate) fn fixture() -> Self {
        Self {
            client: crate::config::http_client(Duration::from_secs(3), Duration::from_secs(10))
                .unwrap(),
            request_limit: 16 * 1024,
            response_limit: 64 * 1024,
        }
    }
}

#[test]
fn a_batch_is_one_call_or_only_parallel_approved_calls() {
    let call = |name: &str| (name.to_owned(), Value::Null);
    let parallel = |name: &str| name == "read";
    assert_eq!(check_batch(&[], parallel), Err(Failure::Runner));
    assert_eq!(check_batch(&[call("write")], parallel), Ok(()));
    assert_eq!(check_batch(&[call("read"), call("read")], parallel), Ok(()));
    assert_eq!(
        check_batch(&[call("read"), call("write")], parallel),
        Err(Failure::Runner)
    );
}
