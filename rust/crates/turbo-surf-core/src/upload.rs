//! One-shot bounded request uploads. Queues contain native bytes only.
use crate::net::{ErrorCode, HttpError};
use bytes::Bytes;
use futures_util::Stream;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};

pub const UPLOAD_CHUNK_BYTES: usize = 64 * 1024;
pub const MAX_UPLOAD_BYTES: usize = 64 * 1024 * 1024;
type Queue = mpsc::Receiver<Bytes>;
pub type UploadStream = Pin<Box<dyn Stream<Item = Result<Bytes, HttpError>> + Send>>;

#[derive(Clone)]
struct Failure {
    message: String,
    code: ErrorCode,
}
impl Failure {
    fn error(self) -> HttpError {
        HttpError {
            message: self.message,
            code: self.code,
        }
    }
}
fn closed() -> HttpError {
    HttpError {
        message: "Request upload is closed".into(),
        code: ErrorCode::Network,
    }
}
struct Writable {
    sender: Option<mpsc::Sender<Bytes>>,
    sent: usize,
}
struct Shared {
    writable: Mutex<Writable>,
    failure: watch::Sender<Option<Failure>>,
    max_bytes: usize,
}
#[derive(Clone)]
pub struct UploadSender(Arc<Shared>);
#[derive(Clone)]
pub struct UploadBody {
    receiver: Arc<Mutex<Option<Queue>>>,
    failure: watch::Receiver<Option<Failure>>,
}
struct Reader {
    queue: Queue,
    failure: watch::Receiver<Option<Failure>>,
    failure_closed: bool,
    done: bool,
}

pub fn upload_channel(max_bytes: usize) -> (UploadSender, UploadBody) {
    let (sender, receiver) = mpsc::channel(2);
    let (failure, failed) = watch::channel(None);
    (
        UploadSender(Arc::new(Shared {
            writable: Mutex::new(Writable {
                sender: Some(sender),
                sent: 0,
            }),
            failure,
            max_bytes: max_bytes.min(MAX_UPLOAD_BYTES),
        })),
        UploadBody {
            receiver: Arc::new(Mutex::new(Some(receiver))),
            failure: failed,
        },
    )
}
impl UploadSender {
    pub async fn write(&self, bytes: Vec<u8>) -> Result<(), HttpError> {
        if bytes.len() > UPLOAD_CHUNK_BYTES {
            return Err(HttpError {
                message: "Request upload chunk exceeds native budget".into(),
                code: ErrorCode::BodyTooLarge,
            });
        }
        let sender = {
            let mut state = self.0.writable.lock().map_err(|_| closed())?;
            if let Some(failure) = self.0.failure.borrow().clone() {
                return Err(failure.error());
            }
            let sender = state.sender.as_ref().cloned().ok_or_else(closed)?;
            let sent = state.sent.checked_add(bytes.len()).ok_or_else(closed)?;
            if sent > self.0.max_bytes {
                return Err(HttpError {
                    message: "Request upload exceeds native byte budget".into(),
                    code: ErrorCode::BodyTooLarge,
                });
            }
            state.sent = sent;
            sender
        };
        if bytes.is_empty() {
            return Ok(());
        }
        sender.send(Bytes::from(bytes)).await.map_err(|_| closed())
    }

    pub fn finish(&self) {
        if let Ok(mut state) = self.0.writable.lock() {
            state.sender.take();
        }
    }

    pub fn abort(&self, error: HttpError) {
        self.0.failure.send_replace(Some(Failure {
            message: error.message,
            code: error.code,
        }));
        self.finish();
    }
}
impl UploadBody {
    pub fn take_stream(&self) -> Result<UploadStream, HttpError> {
        let queue = self
            .receiver
            .lock()
            .map_err(|_| closed())?
            .take()
            .ok_or_else(|| HttpError {
                message: "Request upload cannot be replayed".into(),
                code: ErrorCode::Network,
            })?;
        let reader = Reader {
            queue,
            failure: self.failure.clone(),
            failure_closed: false,
            done: false,
        };
        Ok(Box::pin(futures_util::stream::unfold(
            reader,
            |mut reader| async move {
                if reader.done {
                    return None;
                }
                loop {
                    let failure = reader.failure.borrow().clone();
                    if let Some(failure) = failure {
                        reader.done = true;
                        reader.queue.close();
                        return Some((Err(failure.error()), reader));
                    }
                    tokio::select! {
                        biased;
                        changed = reader.failure.changed(), if !reader.failure_closed => {
                            if changed.is_err() {
                                reader.failure_closed = true;
                            }
                        }
                        chunk = reader.queue.recv() => {
                            return chunk.map(|bytes| (Ok(bytes), reader));
                        }
                    }
                }
            },
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    #[tokio::test]
    async fn upload_is_one_shot_and_applies_backpressure_before_consumption() {
        let (sender, body) = upload_channel(3);
        sender.write(vec![1]).await.unwrap();
        sender.write(vec![2]).await.unwrap();
        let third = sender.write(vec![3]);
        tokio::pin!(third);
        assert!(futures_util::poll!(&mut third).is_pending());
        let mut stream = body.take_stream().unwrap();
        assert!(body.clone().take_stream().is_err());
        assert_eq!(stream.next().await.unwrap().unwrap(), &[1][..]);
        third.await.unwrap();
        sender.finish();
        assert_eq!(stream.next().await.unwrap().unwrap(), &[2][..]);
        assert_eq!(stream.next().await.unwrap().unwrap(), &[3][..]);
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn upload_byte_and_chunk_budgets_do_not_consume_capacity_on_failure() {
        let (sender, body) = upload_channel(2);
        assert!(matches!(
            sender
                .write(vec![0; UPLOAD_CHUNK_BYTES + 1])
                .await
                .unwrap_err()
                .code,
            ErrorCode::BodyTooLarge
        ));
        assert!(matches!(
            sender.write(vec![0; 3]).await.unwrap_err().code,
            ErrorCode::BodyTooLarge
        ));
        sender.write(vec![]).await.unwrap();
        sender.write(vec![7, 8]).await.unwrap();
        sender.finish();
        let mut stream = body.take_stream().unwrap();
        assert_eq!(stream.next().await.unwrap().unwrap(), &[7, 8][..]);
        assert!(stream.next().await.is_none());
        assert!(sender.write(vec![]).await.is_err());
    }

    #[tokio::test]
    async fn abort_preempts_queued_bytes_and_stalled_writes_release_on_receiver_drop() {
        let (sender, body) = upload_channel(3);
        sender.write(vec![1]).await.unwrap();
        sender.write(vec![2]).await.unwrap();
        let third = sender.write(vec![3]);
        tokio::pin!(third);
        assert!(futures_util::poll!(&mut third).is_pending());
        let mut stream = body.take_stream().unwrap();
        sender.abort(HttpError {
            message: "authored source failure".into(),
            code: ErrorCode::Network,
        });
        let error = stream.next().await.unwrap().unwrap_err();
        assert_eq!(error.message, "authored source failure");
        drop(stream);
        assert!(third.await.is_err());
        assert!(sender.write(vec![0]).await.is_err());
    }
}
