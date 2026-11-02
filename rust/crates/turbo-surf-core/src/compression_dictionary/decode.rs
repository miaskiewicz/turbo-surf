//! Bounded native codecs; dropping a fetch cancels the worker between codec reads.
use super::SelectedDictionary;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use std::io::{self, Read};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, LazyLock,
};
use std::{
    pin::Pin,
    task::{Context, Poll},
};

pub(crate) const MAX_ENCODED_BYTES: usize = 32 * 1024 * 1024;
const MAX_DECODED_BYTES: usize = 64 * 1024 * 1024;
static WORKERS: LazyLock<Arc<tokio::sync::Semaphore>> =
    LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(2)));

#[derive(Clone, Debug)]
pub(crate) enum DecodeError {
    Size,
    Invalid(String),
    Cancelled,
}
impl std::fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Size => formatter.write_str("Dictionary body exceeds its byte limit"),
            Self::Invalid(message) => formatter.write_str(message),
            Self::Cancelled => formatter.write_str("Dictionary decoding cancelled"),
        }
    }
}
struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
impl std::error::Error for DecodeError {}
fn read_error(error: io::Error) -> DecodeError {
    error
        .get_ref()
        .and_then(|source| source.downcast_ref::<DecodeError>())
        .cloned()
        .unwrap_or_else(|| DecodeError::Invalid(error.to_string()))
}
struct Input<R> {
    reader: R,
    cancelled: Arc<AtomicBool>,
}
impl<R: Read> Read for Input<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(io::Error::other(DecodeError::Cancelled));
        }
        self.reader.read(buffer)
    }
}
fn emit_decoded(
    mut reader: impl Read,
    maximum: usize,
    cancelled: &AtomicBool,
    emit: &mut impl FnMut(&[u8]) -> Result<(), DecodeError>,
) -> Result<(), DecodeError> {
    let mut received = 0usize;
    let mut buffer = [0; 16 * 1024];
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(DecodeError::Cancelled);
        }
        let count = reader.read(&mut buffer).map_err(read_error)?;
        if count == 0 {
            return Ok(());
        }
        if count > maximum.saturating_sub(received) {
            return Err(DecodeError::Size);
        }
        received += count;
        emit(&buffer[..count])?;
    }
}
fn brotli_window_bits(bytes: &[u8]) -> Option<u8> {
    // RFC7932 WBITS is LSB-first;0x11 introduces Shared Brotli's extended window.
    let first = *bytes.first()?;
    if first & 1 == 0 {
        return Some(16);
    }
    let short = (first >> 1) & 7;
    if short != 0 {
        return Some(17 + short);
    }
    let long = (first >> 4) & 7;
    match long {
        0 => Some(17),
        1 if first & 0x80 == 0 => {
            let bits = bytes.get(1)? & 0x3f;
            (10..=30).contains(&bits).then_some(bits)
        }
        1 => None,
        value => Some(8 + value),
    }
}
fn decode_reader(
    encoding: &str,
    reader: impl Read,
    dictionary: SelectedDictionary,
    maximum: usize,
    cancelled: Arc<AtomicBool>,
    mut emit: impl FnMut(&[u8]) -> Result<(), DecodeError>,
) -> Result<(), DecodeError> {
    let magic: &[u8] = match encoding {
        "dcb" => &[0xff, 0x44, 0x43, 0x42],
        "dcz" => &[0x5e, 0x2a, 0x4d, 0x18, 0x20, 0, 0, 0],
        _ => {
            return Err(DecodeError::Invalid(
                "Unsupported dictionary content encoding".into(),
            ))
        }
    };
    let mut input = Input {
        reader,
        cancelled: cancelled.clone(),
    };
    let mut header = vec![0; magic.len() + 32];
    input.read_exact(&mut header).map_err(read_error)?;
    if !header.starts_with(magic) || header[magic.len()..] != dictionary.hash {
        return Err(DecodeError::Invalid(
            "Dictionary response magic or SHA-256 mismatch".into(),
        ));
    }
    let maximum = maximum.min(MAX_DECODED_BYTES);
    match encoding {
        "dcb" => {
            let mut prefix = vec![0];
            input.read_exact(&mut prefix).map_err(read_error)?;
            if prefix[0] == 0x11 {
                let mut second = [0];
                input.read_exact(&mut second).map_err(read_error)?;
                prefix.push(second[0]);
            }
            if brotli_window_bits(&prefix).is_none_or(|bits| bits > 24) {
                return Err(DecodeError::Invalid(
                    "Shared Brotli window exceeds 16 MiB or is malformed".into(),
                ));
            }
            let prefixed = io::Cursor::new(prefix).chain(&mut input);
            let mut decoder = brotli_decompressor::Decompressor::new(prefixed, 8192);
            if !decoder.attach_dictionary(dictionary.body.as_ref().clone().into()) {
                return Err(DecodeError::Invalid(
                    "Shared Brotli dictionary attachment failed".into(),
                ));
            }
            emit_decoded(&mut decoder, maximum, &cancelled, &mut emit)?;
        }
        "dcz" => {
            let mut decoder = zstd::stream::read::Decoder::with_dictionary(
                io::BufReader::new(&mut input),
                &dictionary.body,
            )
            .map_err(read_error)?;
            decoder.window_log_max(24).map_err(read_error)?;
            emit_decoded(&mut decoder, maximum, &cancelled, &mut emit)?;
        }
        _ => unreachable!(),
    }
    // Codec EOF alone cannot publish a cache entry: observe actual transport EOF
    // (including a late I/O error or encoded-size failure) before terminal success.
    let mut discard = [0; 16 * 1024];
    while input.read(&mut discard).map_err(read_error)? != 0 {}
    Ok(())
}
pub(crate) fn decode_sync(
    encoding: &str,
    bytes: Vec<u8>,
    dictionary: SelectedDictionary,
    maximum: usize,
    cancelled: Arc<AtomicBool>,
) -> Result<Vec<u8>, DecodeError> {
    if bytes.len() > MAX_ENCODED_BYTES {
        return Err(DecodeError::Size);
    }
    let mut output = Vec::new();
    decode_reader(
        encoding,
        io::Cursor::new(bytes),
        dictionary,
        maximum,
        cancelled,
        |chunk| {
            output.extend_from_slice(chunk);
            Ok(())
        },
    )?;
    Ok(output)
}
pub(crate) async fn reserve() -> tokio::sync::OwnedSemaphorePermit {
    // acquire_owned() fails only on a closed semaphore; WORKERS is never closed.
    WORKERS
        .clone()
        .acquire_owned()
        .await
        .expect("the dictionary worker semaphore is never closed")
}
pub(crate) async fn decode(
    encoding: String,
    bytes: Vec<u8>,
    dictionary: SelectedDictionary,
    maximum: usize,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> Result<Vec<u8>, DecodeError> {
    let guard = CancelOnDrop(Arc::new(AtomicBool::new(false)));
    let cancelled = guard.0.clone();
    let result = tokio::task::spawn_blocking(move || {
        // The permit stays with the worker even if the caller drops its future.
        let _permit = permit;
        decode_sync(&encoding, bytes, dictionary, maximum, cancelled)
    })
    .await
    .map_err(|error| DecodeError::Invalid(error.to_string()))?;
    drop(guard);
    result
}

struct ChannelInput {
    receiver: tokio::sync::mpsc::Receiver<Result<Option<Bytes>, DecodeError>>,
    pending: Bytes,
    ended: bool,
}
impl Read for ChannelInput {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() || self.ended {
            return Ok(0);
        }
        while self.pending.is_empty() {
            match self.receiver.blocking_recv() {
                Some(Ok(Some(bytes))) => self.pending = bytes,
                Some(Ok(None)) => {
                    self.ended = true;
                    return Ok(0);
                }
                Some(Err(error)) => return Err(io::Error::other(error)),
                None => {
                    return Err(io::Error::other(DecodeError::Invalid(
                        "Dictionary transport ended without terminal input success".into(),
                    )))
                }
            }
        }
        let count = buffer.len().min(self.pending.len());
        buffer[..count].copy_from_slice(&self.pending[..count]);
        self.pending = self.pending.slice(count..);
        Ok(count)
    }
}
// Only explicit Ok(None) is success. A panicking worker or dropped sender must
// surface an error instead of turning an incomplete representation into cache EOF.
struct DecodedStream {
    receiver: tokio::sync::mpsc::Receiver<Result<Option<Bytes>, DecodeError>>,
    producer: tokio::task::JoinHandle<()>,
    cancelled: Arc<AtomicBool>,
    finished: bool,
}
impl Stream for DecodedStream {
    type Item = Result<Bytes, DecodeError>;
    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.finished {
            return Poll::Ready(None);
        }
        match self.receiver.poll_recv(context) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(Ok(Some(bytes)))) => Poll::Ready(Some(Ok(bytes))),
            Poll::Ready(Some(Ok(None))) => {
                self.finished = true;
                Poll::Ready(None)
            }
            Poll::Ready(Some(Err(error))) => {
                self.finished = true;
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => {
                self.finished = true;
                Poll::Ready(Some(Err(DecodeError::Invalid(
                    "Dictionary decoder ended without terminal success".into(),
                ))))
            }
        }
    }
}
impl Drop for DecodedStream {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.producer.abort();
        self.receiver.close();
    }
}
pub(crate) fn decode_stream(
    encoding: String,
    mut transport: Pin<Box<dyn Stream<Item = Result<Bytes, DecodeError>> + Send>>,
    dictionary: SelectedDictionary,
    maximum: usize,
) -> impl Stream<Item = Result<Bytes, DecodeError>> + Send {
    let (input_send, input_recv) = tokio::sync::mpsc::channel(2);
    let (output_send, output_recv) = tokio::sync::mpsc::channel(2);
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = cancelled.clone();
    let producer = tokio::spawn(async move {
        let permit = reserve().await;
        let worker = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let emit = |chunk: &[u8]| {
                if worker_cancelled.load(Ordering::Acquire) {
                    return Err(DecodeError::Cancelled);
                }
                output_send
                    .blocking_send(Ok(Some(Bytes::copy_from_slice(chunk))))
                    .map_err(|_| DecodeError::Cancelled)
            };
            let result = decode_reader(
                &encoding,
                ChannelInput {
                    receiver: input_recv,
                    pending: Bytes::new(),
                    ended: false,
                },
                dictionary,
                maximum,
                worker_cancelled.clone(),
                emit,
            );
            if !worker_cancelled.load(Ordering::Acquire) {
                let _ = output_send.blocking_send(result.map(|()| None));
            }
        });
        let mut received = 0usize;
        loop {
            let bytes = match transport.next().await {
                None => {
                    let _ = input_send.send(Ok(None)).await;
                    break;
                }
                Some(chunk) => match chunk {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        let _ = input_send.send(Err(error)).await;
                        break;
                    }
                },
            };
            if bytes.len() > MAX_ENCODED_BYTES.saturating_sub(received) {
                let _ = input_send.send(Err(DecodeError::Size)).await;
                break;
            }
            received += bytes.len();
            for chunk in bytes.chunks(16 * 1024) {
                if input_send
                    .send(Ok(Some(Bytes::copy_from_slice(chunk))))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
        drop(input_send);
        let _ = worker.await;
    });
    DecodedStream {
        receiver: output_recv,
        producer,
        cancelled,
        finished: false,
    }
}

#[cfg(test)]
pub(crate) static TEST_SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(test)]
mod terminal_tests {
    use super::*;
    #[tokio::test]
    async fn worker_panic_or_closed_channel_cannot_publish_terminal_success() {
        for panic in [false, true] {
            let (sender, receiver) = tokio::sync::mpsc::channel(2);
            let producer = tokio::spawn(async move {
                let _ = tokio::task::spawn_blocking(move || {
                    sender
                        .blocking_send(Ok(Some(Bytes::from_static(b"partial"))))
                        .unwrap();
                    if panic {
                        panic!("authored worker panic before terminal marker");
                    }
                })
                .await;
            });
            let mut stream = DecodedStream {
                receiver,
                producer,
                cancelled: Arc::new(AtomicBool::new(false)),
                finished: false,
            };
            assert_eq!(
                stream.next().await.unwrap().unwrap(),
                Bytes::from_static(b"partial")
            );
            assert!(stream
                .next()
                .await
                .unwrap()
                .unwrap_err()
                .to_string()
                .contains("without terminal success"));
            assert!(stream.next().await.is_none());
        }
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    fn dictionary() -> SelectedDictionary {
        SelectedDictionary {
            hash: [0; 32],
            body: Arc::new(Vec::new()),
            id: String::new(),
        }
    }
    #[test]
    fn brotli_window_bits_follow_rfc_7932_wbits_and_the_large_window_header() {
        // RFC 7932 9.1: 1 bit 0 -> 16; 4 bits 0bxxx1 -> 17 + xxx; 7 bits 0b0000001 -> 17;
        // 0b0nnn0001 -> 8 + nnn for nnn in 2..=7; 0b0001_0001 (0x11) is the large-window
        // escape, whose next byte carries the bits (10..=30).
        assert_eq!(brotli_window_bits(&[]), None);
        assert_eq!(brotli_window_bits(&[0x00]), Some(16));
        assert_eq!(brotli_window_bits(&[0xfe]), Some(16));
        assert_eq!(brotli_window_bits(&[0x03]), Some(18));
        assert_eq!(brotli_window_bits(&[0x0f]), Some(24));
        assert_eq!(brotli_window_bits(&[0x01]), Some(17));
        assert_eq!(brotli_window_bits(&[0x21]), Some(10));
        assert_eq!(brotli_window_bits(&[0x71]), Some(15));
        assert_eq!(brotli_window_bits(&[0x11]), None);
        assert_eq!(brotli_window_bits(&[0x11, 10]), Some(10));
        assert_eq!(brotli_window_bits(&[0x11, 0xc0 | 30]), Some(30));
        assert_eq!(brotli_window_bits(&[0x11, 9]), None);
        assert_eq!(brotli_window_bits(&[0x11, 31]), None);
        assert_eq!(brotli_window_bits(&[0x91, 24]), None);
    }
    #[test]
    fn errors_describe_themselves() {
        assert_eq!(
            DecodeError::Size.to_string(),
            "Dictionary body exceeds its byte limit"
        );
        assert_eq!(DecodeError::Invalid("x".into()).to_string(), "x");
        assert_eq!(
            DecodeError::Cancelled.to_string(),
            "Dictionary decoding cancelled"
        );
    }
    #[test]
    fn unsupported_oversize_and_cancelled_inputs_are_rejected_before_decoding() {
        let flag = || Arc::new(AtomicBool::new(false));
        let error = decode_sync("gzip", vec![0; 64], dictionary(), 64, flag()).unwrap_err();
        assert_eq!(error.to_string(), "Unsupported dictionary content encoding");
        let error = decode_sync(
            "dcb",
            vec![0; MAX_ENCODED_BYTES + 1],
            dictionary(),
            usize::MAX,
            flag(),
        )
        .unwrap_err();
        assert!(matches!(error, DecodeError::Size));
        // A cancelled worker stops before reading the next codec block.
        let mut emitted = 0;
        let error = emit_decoded(
            io::Cursor::new(vec![1; 8]),
            usize::MAX,
            &AtomicBool::new(true),
            &mut |chunk: &[u8]| {
                emitted += chunk.len();
                Ok(())
            },
        )
        .unwrap_err();
        assert!(matches!(error, DecodeError::Cancelled));
        assert_eq!(emitted, 0);
        // Output is capped exactly at `maximum`.
        let mut out = Vec::new();
        let mut sink = |chunk: &[u8]| {
            out.extend_from_slice(chunk);
            Ok(())
        };
        let cursor = io::Cursor::new(vec![1; 8]);
        assert!(emit_decoded(cursor, 8, &AtomicBool::new(false), &mut sink).is_ok());
        let cursor = io::Cursor::new(vec![1; 9]);
        let error = emit_decoded(cursor, 8, &AtomicBool::new(false), &mut sink).unwrap_err();
        assert!(matches!(error, DecodeError::Size));
        assert_eq!(out, [1; 8]);
    }
}
