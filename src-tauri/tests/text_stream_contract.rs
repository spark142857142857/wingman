use wingman_lib::text_stream::{
    encode_record_stream, RecordFrameV1, RecordStreamWriterV1, TextDecodeErrorV1,
    TextEncodeErrorV1, TextReadErrorV1, TextStreamWriteErrorV1, Utf8RecordDecoderV1,
    Utf8RecordReaderV1,
};

#[test]
fn utf8_scalars_split_at_every_byte_boundary_are_preserved() {
    let input = "ASCII 한글 🚀\n".as_bytes();
    for split in 0..=input.len() {
        let mut decoder = Utf8RecordDecoderV1::new();
        let mut records = decoder.push(&input[..split]).expect("valid prefix");
        records.extend(decoder.push(&input[split..]).expect("valid suffix"));
        records.extend(decoder.finish().expect("valid EOF"));
        assert_eq!(
            records,
            vec![RecordFrameV1 {
                text: "ASCII 한글 🚀".to_string(),
                terminated: true,
            }],
            "split at byte {split}"
        );
    }
}

#[test]
fn invalid_utf8_incomplete_eof_and_nul_are_operational_failures() {
    for bytes in [
        vec![0xc0, 0xaf],
        vec![0xed, 0xa0, 0x80],
        vec![0xf4, 0x90, 0x80, 0x80],
        vec![0xe2, 0x82],
    ] {
        let mut decoder = Utf8RecordDecoderV1::new();
        decoder.push(&bytes).expect("error is finalized at EOF");
        assert!(matches!(
            decoder.finish(),
            Err(TextDecodeErrorV1::InvalidUtf8 { .. })
        ));
    }

    let mut decoder = Utf8RecordDecoderV1::new();
    assert_eq!(
        decoder.push(b"ok\0bad"),
        Err(TextDecodeErrorV1::Nul { byte_offset: 2 })
    );
}

#[test]
fn one_initial_utf8_bom_is_removed_even_when_split_across_reads() {
    let input = b"\xef\xbb\xbfalpha\n";
    for split in 0..=3 {
        let mut decoder = Utf8RecordDecoderV1::new();
        let mut records = decoder.push(&input[..split]).expect("valid BOM prefix");
        records.extend(decoder.push(&input[split..]).expect("valid BOM suffix"));
        records.extend(decoder.finish().expect("valid EOF"));
        assert_eq!(
            records,
            vec![RecordFrameV1 {
                text: "alpha".to_string(),
                terminated: true,
            }]
        );
    }

    let mut bom_only = Utf8RecordDecoderV1::new();
    assert!(bom_only.push(b"\xef\xbb\xbf").unwrap().is_empty());
    assert!(bom_only.finish().unwrap().is_empty());

    let mut later_bom = Utf8RecordDecoderV1::new();
    let mut records = later_bom.push("a\u{feff}b".as_bytes()).unwrap();
    records.extend(later_bom.finish().unwrap());
    assert_eq!(records[0].text, "a\u{feff}b");
}

#[test]
fn lf_crlf_lone_cr_blank_and_unterminated_records_follow_the_contract() {
    let cases = [
        (b"".as_slice(), vec![]),
        (
            b"a".as_slice(),
            vec![RecordFrameV1 {
                text: "a".to_string(),
                terminated: false,
            }],
        ),
        (
            b"a\n".as_slice(),
            vec![RecordFrameV1 {
                text: "a".to_string(),
                terminated: true,
            }],
        ),
        (
            b"a\r\nb".as_slice(),
            vec![
                RecordFrameV1 {
                    text: "a".to_string(),
                    terminated: true,
                },
                RecordFrameV1 {
                    text: "b".to_string(),
                    terminated: false,
                },
            ],
        ),
        (
            b"\n".as_slice(),
            vec![RecordFrameV1 {
                text: String::new(),
                terminated: true,
            }],
        ),
        (
            b"a\n\n".as_slice(),
            vec![
                RecordFrameV1 {
                    text: "a".to_string(),
                    terminated: true,
                },
                RecordFrameV1 {
                    text: String::new(),
                    terminated: true,
                },
            ],
        ),
        (
            b"a\rb".as_slice(),
            vec![RecordFrameV1 {
                text: "a\rb".to_string(),
                terminated: false,
            }],
        ),
    ];

    for (input, expected) in cases {
        let mut decoder = Utf8RecordDecoderV1::new();
        let mut actual = decoder.push(input).unwrap();
        actual.extend(decoder.finish().unwrap());
        assert_eq!(actual, expected, "input: {input:?}");
    }
}

#[test]
fn final_sink_emits_bom_free_utf8_crlf_and_rejects_nonfinal_unterminated_frames() {
    let frames = vec![
        RecordFrameV1 {
            text: "한글".to_string(),
            terminated: true,
        },
        RecordFrameV1 {
            text: "tail".to_string(),
            terminated: false,
        },
    ];
    assert_eq!(
        encode_record_stream(&frames).unwrap(),
        "한글\r\ntail".as_bytes()
    );

    let invalid = vec![
        RecordFrameV1 {
            text: "first".to_string(),
            terminated: false,
        },
        RecordFrameV1 {
            text: "second".to_string(),
            terminated: true,
        },
    ];
    assert_eq!(
        encode_record_stream(&invalid),
        Err(TextEncodeErrorV1::NonFinalUnterminated { index: 0 })
    );
}

#[test]
fn streaming_sink_keeps_only_one_pending_record_and_never_invents_a_boundary() {
    let mut bytes = Vec::new();
    {
        let mut sink = RecordStreamWriterV1::new(&mut bytes);
        sink.push(RecordFrameV1 {
            text: "first".to_string(),
            terminated: true,
        })
        .unwrap();
        sink.push(RecordFrameV1 {
            text: "last".to_string(),
            terminated: false,
        })
        .unwrap();
        sink.finish().unwrap();
    }
    assert_eq!(bytes, b"first\r\nlast");

    let mut invalid_bytes = Vec::new();
    let mut invalid_sink = RecordStreamWriterV1::new(&mut invalid_bytes);
    invalid_sink
        .push(RecordFrameV1 {
            text: "unterminated".to_string(),
            terminated: false,
        })
        .unwrap();
    assert_eq!(
        invalid_sink.push(RecordFrameV1 {
            text: "later".to_string(),
            terminated: true,
        }),
        Err(TextStreamWriteErrorV1::Encode(
            TextEncodeErrorV1::NonFinalUnterminated { index: 0 }
        ))
    );
    assert!(invalid_bytes.is_empty());
}

#[test]
fn record_reader_does_not_decode_a_buffered_suffix_until_the_next_record_is_requested() {
    let bytes = [b"good\n".as_slice(), &[0xff, b'\n']].concat();
    let mut reader = Utf8RecordReaderV1::new(std::io::Cursor::new(bytes));

    assert_eq!(
        reader.next_record().unwrap(),
        Some(RecordFrameV1 {
            text: "good".to_string(),
            terminated: true,
        })
    );
    assert!(matches!(
        reader.next_record(),
        Err(TextReadErrorV1::Decode(TextDecodeErrorV1::InvalidUtf8 {
            byte_offset: 5
        }))
    ));
}

#[test]
fn record_reader_preserves_multibyte_scalars_from_one_byte_reads() {
    struct OneByteReader {
        bytes: std::io::Cursor<Vec<u8>>,
    }

    impl std::io::Read for OneByteReader {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            let maximum = output.len().min(1);
            self.bytes.read(&mut output[..maximum])
        }
    }

    let source = OneByteReader {
        bytes: std::io::Cursor::new("한글🚀".as_bytes().to_vec()),
    };
    let mut reader = Utf8RecordReaderV1::new(source);
    assert_eq!(
        reader.next_record().unwrap(),
        Some(RecordFrameV1 {
            text: "한글🚀".to_string(),
            terminated: false,
        })
    );
    assert_eq!(reader.next_record().unwrap(), None);
}
