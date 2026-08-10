pub const MAX_RECORD_BYTES: usize = 1024 * 1024;

const UTF8_BOM: [u8; 3] = [0xef, 0xbb, 0xbf];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordFrameV1 {
    pub text: String,
    pub terminated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextDecodeErrorV1 {
    InvalidUtf8 { byte_offset: usize },
    Nul { byte_offset: usize },
    RecordTooLong { limit: usize, observed: usize },
    AlreadyFinished,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextEncodeErrorV1 {
    NonFinalUnterminated { index: usize },
    Nul { index: usize },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextStreamWriteErrorV1 {
    Encode(TextEncodeErrorV1),
    Io { kind: io::ErrorKind },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextReadErrorV1 {
    Decode(TextDecodeErrorV1),
    Io { kind: io::ErrorKind },
}

pub struct Utf8RecordReaderV1<R: io::Read> {
    reader: R,
    decoder: Utf8RecordDecoderV1,
    buffer: [u8; 8192],
    cursor: usize,
    length: usize,
    eof: bool,
    finished: bool,
}

impl<R: io::Read> Utf8RecordReaderV1<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            decoder: Utf8RecordDecoderV1::new(),
            buffer: [0; 8192],
            cursor: 0,
            length: 0,
            eof: false,
            finished: false,
        }
    }

    pub fn next_record(&mut self) -> Result<Option<RecordFrameV1>, TextReadErrorV1> {
        if self.finished {
            return Ok(None);
        }
        loop {
            if self.cursor < self.length {
                let byte = self.buffer[self.cursor];
                self.cursor += 1;
                let mut records = self
                    .decoder
                    .push(std::slice::from_ref(&byte))
                    .map_err(TextReadErrorV1::Decode)?;
                if let Some(record) = records.pop() {
                    return Ok(Some(record));
                }
                continue;
            }
            if self.eof {
                self.finished = true;
                let mut records = self.decoder.finish().map_err(TextReadErrorV1::Decode)?;
                return Ok(records.pop());
            }
            match self.reader.read(&mut self.buffer) {
                Ok(0) => self.eof = true,
                Ok(length) => {
                    self.cursor = 0;
                    self.length = length;
                }
                Err(error) => {
                    self.finished = true;
                    return Err(TextReadErrorV1::Io { kind: error.kind() });
                }
            }
        }
    }
}

impl<R: io::Read> Iterator for Utf8RecordReaderV1<R> {
    type Item = Result<RecordFrameV1, TextReadErrorV1>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.next_record() {
            Ok(Some(record)) => Some(Ok(record)),
            Ok(None) => None,
            Err(error) => {
                self.finished = true;
                Some(Err(error))
            }
        }
    }
}

pub struct RecordStreamWriterV1<W: Write> {
    writer: W,
    pending: Option<(usize, RecordFrameV1)>,
    next_index: usize,
}

impl<W: Write> RecordStreamWriterV1<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            pending: None,
            next_index: 0,
        }
    }

    pub fn push(&mut self, frame: RecordFrameV1) -> Result<(), TextStreamWriteErrorV1> {
        if frame.text.contains('\0') {
            return Err(TextStreamWriteErrorV1::Encode(TextEncodeErrorV1::Nul {
                index: self.next_index,
            }));
        }
        if let Some((index, pending)) = self.pending.take() {
            if !pending.terminated {
                self.pending = Some((index, pending));
                return Err(TextStreamWriteErrorV1::Encode(
                    TextEncodeErrorV1::NonFinalUnterminated { index },
                ));
            }
            write_record(&mut self.writer, &pending)
                .map_err(|error| TextStreamWriteErrorV1::Io { kind: error.kind() })?;
        }
        let index = self.next_index;
        self.next_index = self.next_index.saturating_add(1);
        self.pending = Some((index, frame));
        Ok(())
    }

    pub(crate) fn flush_terminated(&mut self) -> Result<(), TextStreamWriteErrorV1> {
        if self
            .pending
            .as_ref()
            .is_some_and(|(_, pending)| !pending.terminated)
        {
            return Ok(());
        }
        if let Some((_, pending)) = self.pending.take() {
            write_record(&mut self.writer, &pending)
                .and_then(|()| self.writer.flush())
                .map_err(|error| TextStreamWriteErrorV1::Io { kind: error.kind() })?;
        } else {
            self.writer
                .flush()
                .map_err(|error| TextStreamWriteErrorV1::Io { kind: error.kind() })?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<W, TextStreamWriteErrorV1> {
        if let Some((_, pending)) = self.pending.take() {
            write_record(&mut self.writer, &pending)
                .and_then(|()| self.writer.flush())
                .map_err(|error| TextStreamWriteErrorV1::Io { kind: error.kind() })?;
        } else {
            self.writer
                .flush()
                .map_err(|error| TextStreamWriteErrorV1::Io { kind: error.kind() })?;
        }
        Ok(self.writer)
    }
}

pub struct Utf8RecordDecoderV1 {
    initial_bytes: Vec<(u8, usize)>,
    initial_decided: bool,
    pending_record: Vec<u8>,
    pending_offset: Option<usize>,
    next_offset: usize,
    finished: bool,
    failed: bool,
}

impl Default for Utf8RecordDecoderV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl Utf8RecordDecoderV1 {
    pub fn new() -> Self {
        Self {
            initial_bytes: Vec::with_capacity(UTF8_BOM.len()),
            initial_decided: false,
            pending_record: Vec::new(),
            pending_offset: None,
            next_offset: 0,
            finished: false,
            failed: false,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<RecordFrameV1>, TextDecodeErrorV1> {
        self.ensure_writable()?;
        let mut records = Vec::new();
        for &byte in bytes {
            let offset = self.next_offset;
            self.next_offset = self.next_offset.saturating_add(1);
            let result = if self.initial_decided {
                self.push_payload_byte(byte, offset, &mut records)
            } else {
                self.push_initial_byte(byte, offset, &mut records)
            };
            if let Err(error) = result {
                self.failed = true;
                return Err(error);
            }
        }
        Ok(records)
    }

    pub fn finish(&mut self) -> Result<Vec<RecordFrameV1>, TextDecodeErrorV1> {
        self.ensure_writable()?;
        let mut records = Vec::new();
        if !self.initial_decided {
            self.initial_decided = true;
            if self.initial_bytes.as_slice()
                != UTF8_BOM
                    .iter()
                    .copied()
                    .enumerate()
                    .map(|(offset, byte)| (byte, offset))
                    .collect::<Vec<_>>()
                    .as_slice()
            {
                let initial = std::mem::take(&mut self.initial_bytes);
                for (byte, offset) in initial {
                    if let Err(error) = self.push_payload_byte(byte, offset, &mut records) {
                        self.failed = true;
                        return Err(error);
                    }
                }
            } else {
                self.initial_bytes.clear();
            }
        }

        if !self.pending_record.is_empty() {
            match self.take_record(false) {
                Ok(record) => records.push(record),
                Err(error) => {
                    self.failed = true;
                    return Err(error);
                }
            }
        }
        self.finished = true;
        Ok(records)
    }

    fn ensure_writable(&self) -> Result<(), TextDecodeErrorV1> {
        if self.failed {
            Err(TextDecodeErrorV1::Failed)
        } else if self.finished {
            Err(TextDecodeErrorV1::AlreadyFinished)
        } else {
            Ok(())
        }
    }

    fn push_initial_byte(
        &mut self,
        byte: u8,
        offset: usize,
        records: &mut Vec<RecordFrameV1>,
    ) -> Result<(), TextDecodeErrorV1> {
        self.initial_bytes.push((byte, offset));
        let matches_bom_prefix = self
            .initial_bytes
            .iter()
            .zip(UTF8_BOM)
            .all(|((received, _), expected)| *received == expected);
        if matches_bom_prefix && self.initial_bytes.len() < UTF8_BOM.len() {
            return Ok(());
        }
        self.initial_decided = true;
        if matches_bom_prefix && self.initial_bytes.len() == UTF8_BOM.len() {
            self.initial_bytes.clear();
            return Ok(());
        }

        let initial = std::mem::take(&mut self.initial_bytes);
        for (initial_byte, initial_offset) in initial {
            self.push_payload_byte(initial_byte, initial_offset, records)?;
        }
        Ok(())
    }

    fn push_payload_byte(
        &mut self,
        byte: u8,
        offset: usize,
        records: &mut Vec<RecordFrameV1>,
    ) -> Result<(), TextDecodeErrorV1> {
        if byte == 0 {
            return Err(TextDecodeErrorV1::Nul {
                byte_offset: offset,
            });
        }
        if byte == b'\n' {
            if self.pending_record.last() == Some(&b'\r') {
                self.pending_record.pop();
            }
            records.push(self.take_record(true)?);
            return Ok(());
        }
        if self.pending_record.len() >= MAX_RECORD_BYTES {
            return Err(TextDecodeErrorV1::RecordTooLong {
                limit: MAX_RECORD_BYTES,
                observed: self.pending_record.len().saturating_add(1),
            });
        }
        if self.pending_offset.is_none() {
            self.pending_offset = Some(offset);
        }
        self.pending_record.push(byte);
        Ok(())
    }

    fn take_record(&mut self, terminated: bool) -> Result<RecordFrameV1, TextDecodeErrorV1> {
        let offset = self.pending_offset.take().unwrap_or(self.next_offset);
        let bytes = std::mem::take(&mut self.pending_record);
        let text = String::from_utf8(bytes).map_err(|error| TextDecodeErrorV1::InvalidUtf8 {
            byte_offset: offset.saturating_add(error.utf8_error().valid_up_to()),
        })?;
        Ok(RecordFrameV1 { text, terminated })
    }
}

pub fn encode_record_stream(records: &[RecordFrameV1]) -> Result<Vec<u8>, TextEncodeErrorV1> {
    let mut encoded = Vec::new();
    for (index, record) in records.iter().enumerate() {
        if index + 1 < records.len() && !record.terminated {
            return Err(TextEncodeErrorV1::NonFinalUnterminated { index });
        }
        if record.text.contains('\0') {
            return Err(TextEncodeErrorV1::Nul { index });
        }
        encoded.extend_from_slice(record.text.as_bytes());
        if record.terminated {
            encoded.extend_from_slice(b"\r\n");
        }
    }
    Ok(encoded)
}

fn write_record(writer: &mut impl Write, record: &RecordFrameV1) -> io::Result<()> {
    writer.write_all(record.text.as_bytes())?;
    if record.terminated {
        writer.write_all(b"\r\n")?;
    }
    Ok(())
}
use std::io::{self, Write};
