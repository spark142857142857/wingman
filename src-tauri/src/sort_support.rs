use std::cmp::Ordering;

pub const MAX_SORT_RECORDS: usize = 262_144;
pub const MAX_SORT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExactDecimalV1 {
    negative: bool,
    digits: Vec<u8>,
    scale: usize,
}

pub(crate) fn resource_limit_exceeded(
    record_count: usize,
    retained_bytes: usize,
    next_record_bytes: usize,
) -> bool {
    record_count >= MAX_SORT_RECORDS
        || retained_bytes.saturating_add(next_record_bytes) > MAX_SORT_BYTES
}

pub(crate) fn parse_exact_decimal(text: &str) -> Option<ExactDecimalV1> {
    let trimmed = text.trim_matches([' ', '\t']);
    if trimmed.is_empty() {
        return Some(ExactDecimalV1 {
            negative: false,
            digits: vec![b'0'],
            scale: 0,
        });
    }
    let bytes = trimmed.as_bytes();
    let (negative, payload) = match bytes.first()? {
        b'+' => (false, &bytes[1..]),
        b'-' => (true, &bytes[1..]),
        _ => (false, bytes),
    };
    if payload.is_empty() {
        return None;
    }
    let mut seen_dot = false;
    let mut digit_count = 0usize;
    let mut fraction_count = 0usize;
    let mut digits = Vec::with_capacity(payload.len());
    for &byte in payload {
        match byte {
            b'.' if !seen_dot => seen_dot = true,
            b'0'..=b'9' => {
                digit_count = digit_count.saturating_add(1);
                if seen_dot {
                    fraction_count = fraction_count.saturating_add(1);
                }
                digits.push(byte);
            }
            _ => return None,
        }
    }
    if digit_count == 0 {
        return None;
    }
    let first_nonzero = digits.iter().position(|digit| *digit != b'0');
    let Some(first_nonzero) = first_nonzero else {
        return Some(ExactDecimalV1 {
            negative: false,
            digits: vec![b'0'],
            scale: 0,
        });
    };
    digits.drain(..first_nonzero);
    while fraction_count > 0 && digits.last() == Some(&b'0') {
        digits.pop();
        fraction_count -= 1;
    }
    Some(ExactDecimalV1 {
        negative,
        digits,
        scale: fraction_count,
    })
}

pub(crate) fn compare_exact_decimal(left: &ExactDecimalV1, right: &ExactDecimalV1) -> Ordering {
    if left.negative != right.negative {
        return if left.negative {
            Ordering::Less
        } else {
            Ordering::Greater
        };
    }
    let magnitude = compare_decimal_magnitude(left, right);
    if left.negative {
        magnitude.reverse()
    } else {
        magnitude
    }
}

fn compare_decimal_magnitude(left: &ExactDecimalV1, right: &ExactDecimalV1) -> Ordering {
    let left_zero = left.digits.as_slice() == b"0";
    let right_zero = right.digits.as_slice() == b"0";
    match (left_zero, right_zero) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        (false, false) => {}
    }
    let left_integer_digits = left.digits.len() as isize - left.scale as isize;
    let right_integer_digits = right.digits.len() as isize - right.scale as isize;
    match left_integer_digits.cmp(&right_integer_digits) {
        Ordering::Equal => {}
        ordering => return ordering,
    }
    let maximum_digits = left.digits.len().max(right.digits.len());
    for index in 0..maximum_digits {
        let left_digit = left.digits.get(index).copied().unwrap_or(b'0');
        let right_digit = right.digits.get(index).copied().unwrap_or(b'0');
        match left_digit.cmp(&right_digit) {
            Ordering::Equal => {}
            ordering => return ordering,
        }
    }
    Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::{resource_limit_exceeded, MAX_SORT_BYTES};

    #[test]
    fn exact_byte_limit_is_allowed_and_one_more_byte_is_rejected() {
        assert!(!resource_limit_exceeded(64, MAX_SORT_BYTES, 0));
        assert!(!resource_limit_exceeded(63, MAX_SORT_BYTES - 1, 1));
        assert!(resource_limit_exceeded(64, MAX_SORT_BYTES, 1));
    }
}
