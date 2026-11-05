//! RFC 6455 framing for the WebSocket client ([`crate::websocket`]): masked client frames,
//! inbound validation (the protocol errors that make Chrome fail the connection) and the
//! RFC 7692 `permessage-deflate` extension. Pure and synchronous; the session drives I/O.
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};

pub(crate) const TEXT: u8 = 1;
pub(crate) const BINARY: u8 = 2;
pub(crate) const CLOSE: u8 = 8;
pub(crate) const PING: u8 = 9;
pub(crate) const PONG: u8 = 10;
const CONTINUATION: u8 = 0;
const DEFLATE_TAIL: [u8; 4] = [0, 0, 0xff, 0xff];
/// Upper bound on one reassembled (or inflated) message; larger peers fail the connection.
pub(crate) const MAX_MESSAGE: usize = 256 << 20;

/// One frame as read off the wire, before reassembly.
#[derive(Debug)]
pub(crate) struct RawFrame {
    pub fin: bool,
    pub rsv1: bool,
    pub rsv23: bool,
    pub opcode: u8,
    pub masked: bool,
    pub payload: Vec<u8>,
}

/// A complete inbound unit after reassembly and validation.
#[derive(Debug, PartialEq)]
pub(crate) enum Inbound {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong,
    Close(Option<u16>, String),
}

/// The fault that fails the connection (error event, then an abnormal 1006 close).
#[derive(Debug, PartialEq)]
pub struct ProtocolError(pub &'static str);

/// Negotiated `permessage-deflate` parameters (RFC 7692 section 7).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct DeflateParams {
    pub server_no_context_takeover: bool,
    pub client_no_context_takeover: bool,
    /// The server limited our compression window below 15 bits; miniz cannot honor
    /// that, so outbound messages stay uncompressed (always permitted by RFC 7692).
    pub client_window_limited: bool,
}

/// Parses the server's `Sec-WebSocket-Extensions` answer. Only `permessage-deflate`
/// was offered, so anything else (or a malformed parameter) fails the handshake.
pub(crate) fn parse_extensions(value: &str) -> Result<Option<DeflateParams>, ProtocolError> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let mut parts = value.split(';').map(str::trim);
    if parts.next() != Some("permessage-deflate") {
        return Err(ProtocolError("unsupported extension"));
    }
    let mut params = DeflateParams::default();
    for part in parts {
        apply_deflate_param(&mut params, part)?;
    }
    Ok(Some(params))
}

fn apply_deflate_param(params: &mut DeflateParams, part: &str) -> Result<(), ProtocolError> {
    let (name, value) = part.split_once('=').map_or((part, None), |(name, value)| {
        (name.trim(), Some(value.trim()))
    });
    let bits = value.map(|value| value.trim_matches('"').parse::<u8>());
    let bits_ok = matches!(bits, Some(Ok(8..=15)));
    match name {
        "server_no_context_takeover" if value.is_none() => params.server_no_context_takeover = true,
        "client_no_context_takeover" if value.is_none() => params.client_no_context_takeover = true,
        "server_max_window_bits" if bits_ok => {}
        "client_max_window_bits" if bits_ok => {
            params.client_window_limited = !matches!(bits, Some(Ok(15)));
        }
        _ => return Err(ProtocolError("invalid permessage-deflate parameter")),
    }
    Ok(())
}

/// Encodes one masked client frame.
pub(crate) fn encode(opcode: u8, payload: &[u8], rsv1: bool, mask: [u8; 4]) -> Vec<u8> {
    let length = payload.len();
    let mut out = Vec::with_capacity(length + 14);
    out.push(0x80 | if rsv1 { 0x40 } else { 0 } | opcode);
    if length < 126 {
        out.push(0x80 | length as u8);
    } else if length < 65536 {
        out.push(0x80 | 126);
        out.extend_from_slice(&(length as u16).to_be_bytes());
    } else {
        out.push(0x80 | 127);
        out.extend_from_slice(&(length as u64).to_be_bytes());
    }
    out.extend_from_slice(&mask);
    out.extend(
        payload
            .iter()
            .enumerate()
            .map(|(at, byte)| byte ^ mask[at & 3]),
    );
    out
}

/// The close frame payload for `close(code, reason)`: empty without a code.
pub(crate) fn close_payload(code: Option<u16>, reason: &str) -> Vec<u8> {
    let Some(code) = code else {
        return Vec::new();
    };
    let mut out = code.to_be_bytes().to_vec();
    out.extend_from_slice(reason.as_bytes());
    out
}

/// Close codes a peer may put on the wire (RFC 6455 section 7.4).
fn wire_close_code(code: u16) -> bool {
    matches!(code, 1000..=1003 | 1007..=1014 | 3000..=4999)
}

fn parse_close(payload: &[u8]) -> Result<Inbound, ProtocolError> {
    match payload.len() {
        0 => Ok(Inbound::Close(None, String::new())),
        1 => Err(ProtocolError("truncated close code")),
        _ => {
            let code = u16::from_be_bytes([payload[0], payload[1]]);
            if !wire_close_code(code) {
                return Err(ProtocolError("invalid close code"));
            }
            let reason = std::str::from_utf8(&payload[2..])
                .map_err(|_| ProtocolError("invalid close reason"))?;
            Ok(Inbound::Close(Some(code), reason.to_owned()))
        }
    }
}

/// Outbound compressor for one connection.
pub(crate) struct Deflater {
    compress: Compress,
    reset_each: bool,
}

impl Deflater {
    pub fn new(params: DeflateParams) -> Option<Self> {
        (!params.client_window_limited).then(|| Self {
            compress: Compress::new(Compression::default(), false),
            reset_each: params.client_no_context_takeover,
        })
    }

    /// Compressed message body without the trailing empty stored block.
    pub fn deflate(&mut self, input: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(input.len() / 2 + 64);
        let start = self.compress.total_in();
        loop {
            // compress_vec fills spare capacity only: the flush is complete once all
            // input is consumed and the output did not run out of room.
            if out.capacity() - out.len() < 64 {
                out.reserve(out.capacity().max(64));
            }
            let consumed = (self.compress.total_in() - start) as usize;
            self.compress
                .compress_vec(&input[consumed..], &mut out, FlushCompress::Sync)
                .expect("deflate of an in-memory buffer");
            let consumed = (self.compress.total_in() - start) as usize;
            if consumed == input.len() && out.len() < out.capacity() {
                break;
            }
        }
        out.truncate(out.len() - DEFLATE_TAIL.len());
        if self.reset_each {
            self.compress.reset();
        }
        out
    }
}

/// Inbound decompressor for one connection.
struct Inflater {
    decompress: Decompress,
    reset_each: bool,
}

impl Inflater {
    fn inflate(&mut self, mut input: Vec<u8>) -> Result<Vec<u8>, ProtocolError> {
        input.extend_from_slice(&DEFLATE_TAIL);
        let mut out = Vec::with_capacity(input.len() * 3 + 64);
        let start = self.decompress.total_in();
        loop {
            if out.capacity() - out.len() < 64 {
                out.reserve(out.capacity().max(1024));
            }
            let consumed = (self.decompress.total_in() - start) as usize;
            let produced = out.len();
            let status = self
                .decompress
                .decompress_vec(&input[consumed..], &mut out, FlushDecompress::Sync)
                .map_err(|_| ProtocolError("invalid compressed data"))?;
            let now = (self.decompress.total_in() - start) as usize;
            if out.len() > MAX_MESSAGE {
                return Err(ProtocolError("message too large"));
            }
            if status == Status::StreamEnd || now == input.len() && out.len() < out.capacity() {
                break;
            }
            if now == consumed && out.len() == produced {
                return Err(ProtocolError("stalled compressed data"));
            }
        }
        if self.reset_each {
            self.decompress.reset(false);
        }
        Ok(out)
    }
}

/// Reassembles fragmented messages and validates every inbound frame.
pub(crate) struct Assembler {
    opcode: Option<u8>,
    compressed: bool,
    buffer: Vec<u8>,
    inflater: Option<Inflater>,
}

impl Assembler {
    pub fn new(deflate: Option<DeflateParams>) -> Self {
        Self {
            opcode: None,
            compressed: false,
            buffer: Vec::new(),
            inflater: deflate.map(|params| Inflater {
                decompress: Decompress::new(false),
                reset_each: params.server_no_context_takeover,
            }),
        }
    }

    /// Feeds one frame; returns a unit once a control frame or a final fragment arrives.
    pub fn accept(&mut self, frame: RawFrame) -> Result<Option<Inbound>, ProtocolError> {
        if frame.masked {
            return Err(ProtocolError("masked server frame"));
        }
        if frame.rsv23 {
            return Err(ProtocolError("reserved bits set"));
        }
        if frame.opcode >= CLOSE {
            return Self::control(frame).map(Some);
        }
        self.data(frame)
    }

    fn control(frame: RawFrame) -> Result<Inbound, ProtocolError> {
        if !frame.fin || frame.rsv1 || frame.payload.len() > 125 {
            return Err(ProtocolError("invalid control frame"));
        }
        match frame.opcode {
            CLOSE => parse_close(&frame.payload),
            PING => Ok(Inbound::Ping(frame.payload)),
            PONG => Ok(Inbound::Pong),
            _ => Err(ProtocolError("unknown control opcode")),
        }
    }

    fn data(&mut self, frame: RawFrame) -> Result<Option<Inbound>, ProtocolError> {
        match (frame.opcode, self.opcode) {
            (CONTINUATION, Some(_)) if !frame.rsv1 => {}
            (TEXT | BINARY, None) if !frame.rsv1 || self.inflater.is_some() => {
                self.opcode = Some(frame.opcode);
                self.compressed = frame.rsv1;
            }
            _ => return Err(ProtocolError("unexpected data frame")),
        }
        if self.buffer.len() + frame.payload.len() > MAX_MESSAGE {
            return Err(ProtocolError("message too large"));
        }
        self.buffer.extend_from_slice(&frame.payload);
        if !frame.fin {
            return Ok(None);
        }
        self.finish().map(Some)
    }

    fn finish(&mut self) -> Result<Inbound, ProtocolError> {
        let opcode = self.opcode.take().unwrap_or(BINARY);
        let mut payload = std::mem::take(&mut self.buffer);
        if std::mem::take(&mut self.compressed) {
            let inflater = self.inflater.as_mut().ok_or(ProtocolError("no inflater"))?;
            payload = inflater.inflate(payload)?;
        }
        if opcode == BINARY {
            return Ok(Inbound::Binary(payload));
        }
        String::from_utf8(payload)
            .map(Inbound::Text)
            .map_err(|_| ProtocolError("invalid UTF-8 text"))
    }
}

/// Parses a frame header (`head` holds the first two bytes); returns the extended
/// length size and whether a mask key follows.
pub(crate) fn header_layout(head: [u8; 2]) -> (usize, bool) {
    let extended = match head[1] & 0x7f {
        126 => 2,
        127 => 8,
        _ => 0,
    };
    (extended, head[1] & 0x80 != 0)
}

/// Builds a [`RawFrame`] from its header bytes, extended length and payload.
pub(crate) fn raw_frame(head: [u8; 2], masked: bool, payload: Vec<u8>) -> RawFrame {
    RawFrame {
        fin: head[0] & 0x80 != 0,
        rsv1: head[0] & 0x40 != 0,
        rsv23: head[0] & 0x30 != 0,
        opcode: head[0] & 0x0f,
        masked,
        payload,
    }
}

/// The payload length encoded by `head` and its extended length bytes.
pub(crate) fn payload_length(head: [u8; 2], extended: &[u8]) -> Result<usize, ProtocolError> {
    let length = match extended.len() {
        2 => u64::from(u16::from_be_bytes([extended[0], extended[1]])),
        8 => u64::from_be_bytes(extended.try_into().expect("eight length bytes")),
        _ => u64::from(head[1] & 0x7f),
    };
    usize::try_from(length)
        .ok()
        .filter(|length| *length <= MAX_MESSAGE)
        .ok_or(ProtocolError("frame too large"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(head0: u8, payload: &[u8]) -> RawFrame {
        raw_frame([head0, payload.len() as u8], false, payload.to_vec())
    }

    #[test]
    fn encodes_masked_frames_with_every_length_form() {
        let mask = [1, 2, 3, 4];
        let short = encode(TEXT, b"hi", false, mask);
        assert_eq!(short[..2], [0x81, 0x82]);
        assert_eq!(short[2..6], mask);
        assert_eq!(short[6..], [b'h' ^ 1, b'i' ^ 2]);
        let medium = encode(BINARY, &[0; 300], true, mask);
        assert_eq!(medium[..4], [0xc2, 0x80 | 126, 1, 44]);
        let long = encode(BINARY, &vec![0; 70000], false, mask);
        assert_eq!(long[1], 0x80 | 127);
        assert_eq!(payload_length([0x82, 127], &long[2..10]), Ok(70000));
        assert_eq!(header_layout([0x82, 126]), (2, false));
        assert_eq!(header_layout([0x82, 0x85]), (0, true));
        assert_eq!(payload_length([0x82, 126], &[1, 0]), Ok(256));
        assert!(payload_length([0x82, 127], &[0xff; 8]).is_err());
    }

    #[test]
    fn close_payloads_and_codes() {
        assert_eq!(close_payload(None, "ignored"), Vec::<u8>::new());
        assert_eq!(close_payload(Some(4000), "ok"), vec![15, 160, b'o', b'k']);
        assert_eq!(parse_close(&[]), Ok(Inbound::Close(None, String::new())));
        assert!(parse_close(&[3]).is_err());
        assert!(parse_close(&1005u16.to_be_bytes()).is_err());
        assert!(parse_close(&[3, 232, 0xff]).is_err());
        assert_eq!(
            parse_close(&close_payload(Some(4321), "bye")),
            Ok(Inbound::Close(Some(4321), "bye".into()))
        );
    }

    #[test]
    fn reassembles_and_validates_inbound_frames() {
        let mut assembler = Assembler::new(None);
        assert_eq!(assembler.accept(frame(0x01, b"ab")), Ok(None));
        assert_eq!(
            assembler.accept(frame(0x80, b"cd")),
            Ok(Some(Inbound::Text("abcd".into())))
        );
        assert_eq!(
            assembler.accept(frame(0x82, &[9])),
            Ok(Some(Inbound::Binary(vec![9])))
        );
        assert_eq!(
            assembler.accept(frame(0x89, b"p")),
            Ok(Some(Inbound::Ping(b"p".to_vec())))
        );
        assert_eq!(assembler.accept(frame(0x8a, b"")), Ok(Some(Inbound::Pong)));
        let failures = [
            frame(0x80, b"x"),
            frame(0x81, &[0xff, 0xfe]),
            frame(0xc1, b"x"),
            frame(0xa1, b"x"),
            frame(0x09, b"x"),
            frame(0x8b, b""),
            frame(0x83, b""),
            raw_frame([0x81, 0x81], true, b"x".to_vec()),
        ];
        for failure in failures {
            assert!(Assembler::new(None).accept(failure).is_err());
        }
        let mut open = Assembler::new(None);
        open.accept(frame(0x01, b"a")).unwrap();
        assert!(open.accept(frame(0x81, b"b")).is_err());
        let mut long = raw_frame([0x89, 126], false, vec![0; 126]);
        long.fin = true;
        assert!(Assembler::new(None).accept(long).is_err());
    }

    #[test]
    fn negotiates_permessage_deflate_parameters() {
        assert_eq!(parse_extensions(" "), Ok(None));
        let params = parse_extensions(
            "permessage-deflate; server_no_context_takeover; client_no_context_takeover; server_max_window_bits=10; client_max_window_bits=15",
        )
        .unwrap()
        .unwrap();
        assert!(params.server_no_context_takeover && params.client_no_context_takeover);
        assert!(!params.client_window_limited);
        let limited = parse_extensions("permessage-deflate; client_max_window_bits=\"9\"")
            .unwrap()
            .unwrap();
        assert!(limited.client_window_limited);
        assert!(Deflater::new(limited).is_none());
        for bad in [
            "x-webkit-deflate-frame",
            "permessage-deflate; server_max_window_bits=7",
            "permessage-deflate; client_no_context_takeover=1",
            "permessage-deflate; bogus",
        ] {
            assert!(parse_extensions(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn deflate_round_trips_with_and_without_context_takeover() {
        for reset in [false, true] {
            let params = DeflateParams {
                server_no_context_takeover: reset,
                client_no_context_takeover: reset,
                client_window_limited: false,
            };
            let mut deflater = Deflater::new(params).unwrap();
            let mut assembler = Assembler::new(Some(params));
            for text in ["squeeze squeeze squeeze", "squeeze squeeze squeeze", ""] {
                let packed = deflater.deflate(text.as_bytes());
                let unit = assembler
                    .accept(raw_frame([0xc1, 0], false, packed))
                    .unwrap();
                assert_eq!(unit, Some(Inbound::Text(text.into())));
            }
            let big = vec![7u8; 300_000];
            let packed = deflater.deflate(&big);
            assert_eq!(
                assembler.accept(raw_frame([0xc2, 0], false, packed)),
                Ok(Some(Inbound::Binary(big)))
            );
        }
        let mut plain = Assembler::new(Some(DeflateParams::default()));
        assert!(plain
            .accept(raw_frame([0xc1, 0], false, vec![0xff, 0xff]))
            .is_err());
    }
}
