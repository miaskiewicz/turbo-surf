use super::*;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use sha2::{Digest, Sha256};
use std::{
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

const ROOT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../harness/detection/fixtures/compression-dictionary/"
);
fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(format!("{ROOT}{name}")).unwrap()
}
fn dictionary() -> SelectedDictionary {
    let body = bytes("dictionary.bin");
    SelectedDictionary {
        hash: Sha256::digest(&body).into(),
        body: Arc::new(body),
        id: String::new(),
    }
}
type Source = Pin<Box<dyn Stream<Item = Result<Bytes, decode::DecodeError>> + Send>>;
fn source(chunks: Vec<Result<Bytes, decode::DecodeError>>) -> Source {
    Box::pin(futures_util::stream::iter(chunks))
}
async fn collect(
    stream: impl Stream<Item = Result<Bytes, decode::DecodeError>>,
) -> Result<Vec<u8>, decode::DecodeError> {
    futures_util::pin_mut!(stream);
    let mut output = Vec::new();
    while let Some(chunk) = stream.next().await {
        output.extend_from_slice(&chunk?);
    }
    Ok(output)
}
async fn recovered() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let first = decode::reserve().await;
        let second = decode::reserve().await;
        drop((first, second));
    })
    .await
    .expect("both workers must release after cancellation");
}
#[tokio::test]
async fn every_header_and_window_boundary_and_single_byte_transport_decode_real_fixtures() {
    let _serial = decode::TEST_SERIAL.lock().await;
    for encoding in ["dcb", "dcz"] {
        let encoded = bytes(&format!("encoded.{encoding}"));
        for split in (0..=43).chain([encoded.len() - 1, encoded.len()]) {
            let chunks = vec![
                Ok(Bytes::copy_from_slice(&encoded[..split])),
                Ok(Bytes::copy_from_slice(&encoded[split..])),
            ];
            let decoded = collect(decode::decode_stream(
                encoding.into(),
                source(chunks),
                dictionary(),
                usize::MAX,
            ))
            .await
            .unwrap();
            assert_eq!(decoded, bytes("decoded.bin"), "{encoding} split {split}");
        }
        let chunks = encoded
            .iter()
            .map(|byte| Ok(Bytes::copy_from_slice(&[*byte])))
            .collect();
        assert_eq!(
            collect(decode::decode_stream(
                encoding.into(),
                source(chunks),
                dictionary(),
                usize::MAX
            ))
            .await
            .unwrap(),
            bytes("decoded.bin")
        );
    }
}
#[tokio::test]
async fn late_transport_error_truncation_hash_and_output_caps_never_report_success() {
    let _serial = decode::TEST_SERIAL.lock().await;
    for encoding in ["dcb", "dcz"] {
        let encoded = bytes(&format!("encoded.{encoding}"));
        let late = source(vec![
            Ok(Bytes::from(encoded.clone())),
            Err(decode::DecodeError::Invalid(
                "late authored transport failure".into(),
            )),
        ]);
        let error = collect(decode::decode_stream(
            encoding.into(),
            late,
            dictionary(),
            usize::MAX,
        ))
        .await
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("late authored transport failure"));
        let short = source(vec![Ok(Bytes::copy_from_slice(
            &encoded[..encoded.len() - 1],
        ))]);
        assert!(collect(decode::decode_stream(
            encoding.into(),
            short,
            dictionary(),
            usize::MAX
        ))
        .await
        .is_err());
        let mut bad = encoded.clone();
        bad[10] ^= 1;
        let error = collect(decode::decode_stream(
            encoding.into(),
            source(vec![Ok(Bytes::from(bad))]),
            dictionary(),
            usize::MAX,
        ))
        .await
        .unwrap_err();
        assert!(error.to_string().contains("SHA-256"));
        let error = collect(decode::decode_stream(
            encoding.into(),
            source(vec![Ok(Bytes::from(encoded))]),
            dictionary(),
            1024,
        ))
        .await
        .unwrap_err();
        assert!(matches!(error, decode::DecodeError::Size));
    }
}
#[tokio::test]
async fn transport_is_not_polled_until_worker_admission_and_cancelled_waiters_release() {
    let _serial = decode::TEST_SERIAL.lock().await;
    let first = decode::reserve().await;
    let second = decode::reserve().await;
    let polls = Arc::new(AtomicUsize::new(0));
    let observed = polls.clone();
    let input = Box::pin(futures_util::stream::poll_fn(move |_| {
        observed.fetch_add(1, Ordering::SeqCst);
        std::task::Poll::Pending
    }));
    let output = decode::decode_stream("dcb".into(), input, dictionary(), usize::MAX);
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert_eq!(polls.load(Ordering::SeqCst), 0);
    drop(output);
    drop((first, second));
    recovered().await;
}
#[tokio::test]
async fn cancelling_blocked_input_and_output_recovers_workers_without_transport_roots() {
    let _serial = decode::TEST_SERIAL.lock().await;
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    let input = Box::pin(futures_util::stream::unfold(
        receiver,
        |mut receiver| async move { receiver.recv().await.map(|chunk| (chunk, receiver)) },
    ));
    let output = decode::decode_stream("dcb".into(), input, dictionary(), usize::MAX);
    sender
        .send(Ok(Bytes::copy_from_slice(&bytes("encoded.dcb")[..36])))
        .await
        .unwrap();
    let reservation = sender.reserve().await.unwrap();
    drop(reservation);
    drop(output);
    recovered().await;
    // Worker permits can be released before Tokio polls the aborted producer.
    // Observe transport teardown itself rather than its scheduling order.
    tokio::time::timeout(Duration::from_secs(10), sender.closed())
        .await
        .expect("cancelled decoder must release its transport receiver");
    assert!(sender.is_closed());
    let encoded = bytes("encoded.dcz");
    let mut wire = encoded.clone();
    for _ in 0..12 {
        wire.extend_from_slice(&encoded[40..]);
    }
    let mut output = Box::pin(decode::decode_stream(
        "dcz".into(),
        source(vec![Ok(Bytes::from(wire))]),
        dictionary(),
        usize::MAX,
    ));
    assert!(tokio::time::timeout(Duration::from_secs(10), output.next())
        .await
        .unwrap()
        .unwrap()
        .is_ok());
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    drop(output);
    recovered().await;
}
#[tokio::test]
async fn decoded_prefix_is_delivered_before_transport_eof_and_late_input_is_required_for_success() {
    let _serial = decode::TEST_SERIAL.lock().await;
    for encoding in ["dcb", "dcz"] {
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        let input = Box::pin(futures_util::stream::unfold(
            receiver,
            |mut receiver| async move { receiver.recv().await.map(|chunk| (chunk, receiver)) },
        ));
        let mut output = Box::pin(decode::decode_stream(
            encoding.into(),
            input,
            dictionary(),
            usize::MAX,
        ));
        sender
            .send(Ok(Bytes::from(bytes(&format!("encoded.{encoding}")))))
            .await
            .unwrap();
        let first = tokio::time::timeout(Duration::from_secs(10), output.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(&first[..], &bytes("decoded.bin")[..first.len()]);
        drop(sender);
        let mut decoded = first.to_vec();
        while let Some(chunk) = output.next().await {
            decoded.extend_from_slice(&chunk.unwrap());
        }
        assert_eq!(decoded, bytes("decoded.bin"));
    }
}

#[tokio::test]
async fn partial_encoded_response_yields_real_prefix_before_remaining_encoded_chunks() {
    let _serial = decode::TEST_SERIAL.lock().await;
    for encoding in ["dcb", "dcz"] {
        let mut encoded = bytes(&format!("encoded.{encoding}"));
        let mut expected = bytes("decoded.bin");
        let split = if encoding == "dcz" {
            let split = encoded.len();
            // Zstd permits concatenated frames. Both independently encoded C
            // frames require the selected raw dictionary, and the second has
            // not reached the transport when the first output is observed.
            encoded.extend_from_within(40..);
            expected.extend_from_within(..);
            split
        } else {
            encoded.len() - 1
        };
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        let input = Box::pin(futures_util::stream::unfold(
            receiver,
            |mut receiver| async move { receiver.recv().await.map(|chunk| (chunk, receiver)) },
        ));
        let mut output = Box::pin(decode::decode_stream(
            encoding.into(),
            input,
            dictionary(),
            usize::MAX,
        ));
        sender
            .send(Ok(Bytes::copy_from_slice(&encoded[..split])))
            .await
            .unwrap();
        let first = tokio::time::timeout(Duration::from_secs(10), output.next())
            .await
            .unwrap_or_else(|_| panic!("{encoding} fixture did not expose partial decoded output"))
            .unwrap()
            .unwrap();
        assert_eq!(&first[..], &bytes("decoded.bin")[..first.len()]);
        sender
            .send(Ok(Bytes::copy_from_slice(&encoded[split..])))
            .await
            .unwrap();
        drop(sender);
        let mut decoded = first.to_vec();
        while let Some(chunk) = output.next().await {
            decoded.extend_from_slice(&chunk.unwrap());
        }
        assert_eq!(decoded, expected);
    }
}

#[tokio::test]
async fn panicking_transport_after_complete_codec_frame_cannot_commit_success() {
    let _serial = decode::TEST_SERIAL.lock().await;
    for encoding in ["dcb", "dcz"] {
        let first = futures_util::stream::once(std::future::ready(Ok(Bytes::from(bytes(
            &format!("encoded.{encoding}"),
        )))));
        let panic = futures_util::stream::poll_fn(
            |_| -> std::task::Poll<Option<Result<Bytes, decode::DecodeError>>> {
                panic!("authored transport producer panic before true HTTP EOF");
            },
        );
        let stream = decode::decode_stream(
            encoding.into(),
            Box::pin(first.chain(panic)),
            dictionary(),
            usize::MAX,
        );
        let error = collect(stream).await.unwrap_err();
        assert!(error.to_string().contains("without terminal input success"));
        recovered().await;
    }
}
#[tokio::test]
async fn encoded_transport_over_32_mib_and_an_early_worker_exit_end_the_producer() {
    let _serial = decode::TEST_SERIAL.lock().await;
    // One transport chunk past the 32 MiB encoded cap fails before the codec sees it.
    let huge = source(vec![Ok(Bytes::from(vec![
        0;
        decode::MAX_ENCODED_BYTES + 1
    ]))]);
    let error = collect(decode::decode_stream(
        "dcb".into(),
        huge,
        dictionary(),
        usize::MAX,
    ))
    .await
    .unwrap_err();
    assert!(matches!(error, decode::DecodeError::Size), "{error}");
    // The worker rejects a bad header and stops reading; the producer then stops
    // forwarding an endless transport instead of buffering it.
    let endless: Source = Box::pin(futures_util::stream::repeat_with(|| {
        Ok(Bytes::from_static(&[0; 1024]))
    }));
    let error = tokio::time::timeout(
        Duration::from_secs(10),
        collect(decode::decode_stream(
            "dcz".into(),
            endless,
            dictionary(),
            usize::MAX,
        )),
    )
    .await
    .expect("the producer must stop once the worker has exited")
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Dictionary response magic or SHA-256 mismatch"
    );
    recovered().await;
}

#[tokio::test]
async fn the_encoded_cap_is_cumulative_and_inclusive() {
    let _serial = decode::TEST_SERIAL.lock().await;
    let run = |chunks: Vec<usize>| {
        let source = source(
            chunks
                .into_iter()
                .map(|length| Ok(Bytes::from(vec![0; length])))
                .collect(),
        );
        collect(decode::decode_stream(
            "dcb".into(),
            source,
            dictionary(),
            usize::MAX,
        ))
    };
    // Exactly 32 MiB passes the transport cap and reaches the (mismatching) header check.
    let error = run(vec![decode::MAX_ENCODED_BYTES]).await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "Dictionary response magic or SHA-256 mismatch"
    );
    // One byte more, split across chunks, is over the cap.
    let error = run(vec![1, decode::MAX_ENCODED_BYTES]).await.unwrap_err();
    assert!(matches!(error, decode::DecodeError::Size), "{error}");
    recovered().await;
}
