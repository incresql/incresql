use crate::encoding_core::SortableEncoding;
use crate::jsonpath_utils::JsonPathExpression;
use crate::{Datum, SortOrder};
use rust_decimal::prelude::Zero;
use rust_decimal::Decimal;

impl Datum<'_> {
    pub fn as_sortable_bytes(&self, sort_order: SortOrder, buffer: &mut Vec<u8>) {
        // For datums we'll write enough info in to make them self describing, this should allow
        // for writing debug tools, data recovery tools etc that can make sense of data in
        // rocksdb files without much context.
        match self {
            // Note 0/255 is reserved here to allow easy range scans on prefixes
            Datum::Null => {
                if sort_order.is_asc() {
                    buffer.push(1)
                } else {
                    buffer.push(!1)
                }
            }
            Datum::Boolean(false) => {
                if sort_order.is_asc() {
                    buffer.push(2)
                } else {
                    buffer.push(!2)
                }
            }
            Datum::Boolean(true) => {
                if sort_order.is_asc() {
                    buffer.push(3)
                } else {
                    buffer.push(!3)
                }
            }
            Datum::Integer(i) => {
                if sort_order.is_asc() {
                    buffer.push(4)
                } else {
                    buffer.push(!4)
                }
                i.write_sortable_bytes(sort_order, buffer);
            }
            Datum::BigInt(i) => {
                if sort_order.is_asc() {
                    buffer.push(5)
                } else {
                    buffer.push(!5)
                }
                i.write_sortable_bytes(sort_order, buffer);
            }
            Datum::Decimal(d) => {
                if sort_order.is_asc() {
                    buffer.push(6)
                } else {
                    buffer.push(!6)
                }
                d.write_sortable_bytes(sort_order, buffer);
            }
            Datum::ByteAOwned(_) | Datum::ByteARef(_) | Datum::ByteAInline(..) => {
                if sort_order.is_asc() {
                    buffer.push(7)
                } else {
                    buffer.push(!7)
                }
                self.as_bytea().write_sortable_bytes(sort_order, buffer)
            }
            Datum::Jsonpath(_) | Datum::JsonpathRef(_) => {
                if sort_order.is_asc() {
                    buffer.push(8)
                } else {
                    buffer.push(!8)
                }
                self.as_jsonpath()
                    .original()
                    .as_bytes()
                    .write_sortable_bytes(sort_order, buffer)
            }
        }
    }

    pub fn from_sortable_bytes<'a>(&mut self, buffer: &'a [u8]) -> &'a [u8] {
        let rem = &buffer[1..];
        // Infer sort order based from data instead
        let sort_order = if buffer[0] < 127 {
            SortOrder::Asc
        } else {
            SortOrder::Desc
        };

        match buffer[0] {
            1 | 254 => {
                *self = Datum::Null;
                rem
            }
            2 | 253 => {
                *self = Datum::Boolean(false);
                rem
            }
            3 | 252 => {
                *self = Datum::Boolean(true);
                rem
            }
            4 | 251 => {
                let mut i = 0_i32;
                let rem = i.read_sortable_bytes(sort_order, rem);
                *self = Datum::Integer(i);
                rem
            }
            5 | 250 => {
                let mut i = 0_i64;
                let rem = i.read_sortable_bytes(sort_order, rem);
                *self = Datum::BigInt(i);
                rem
            }
            6 | 249 => {
                let mut d = Decimal::zero();
                let rem = d.read_sortable_bytes(sort_order, rem);
                *self = Datum::Decimal(d);
                rem
            }
            7 | 248 => {
                // TODO there's no need to allocate here,
                // we can pass in a single buffer that can be used for all strings/bytea's.
                // However that wont quite work due to the backing array being deallocated on a
                // resize. A "pool" of strings  or vectors might be better instead.
                let mut str_buffer = Vec::new();
                let rem = str_buffer.read_sortable_bytes(sort_order, rem);
                *self = Datum::ByteAOwned(Box::from(str_buffer));
                rem
            }
            8 | 247 => {
                // TODO there's no need to allocate here as above
                let mut str_buffer = Vec::new();
                let rem = str_buffer.read_sortable_bytes(sort_order, rem);
                *self = Datum::Jsonpath(Box::from(
                    JsonPathExpression::parse(unsafe {
                        std::str::from_utf8_unchecked(&str_buffer)
                    })
                    .unwrap(),
                ));
                rem
            }
            _ => panic!("Got unexpected datum encoding {}", buffer[0]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_datums() {
        // Already sorted into serialization order
        let datums = [
            Datum::Null,
            Datum::from(false),
            Datum::from(true),
            Datum::Integer(-10),
            Datum::Integer(0),
            Datum::Integer(1000),
            Datum::BigInt(-6876),
            Datum::BigInt(0),
            Datum::BigInt(890934467),
            Datum::from(Decimal::new(-32678, 2)),
            Datum::from(Decimal::zero()),
            Datum::from(Decimal::new(67832, 2)),
            Datum::from("abcd"),
            Datum::from("efg"),
        ];
        let mut asc_byte_arrays = vec![];
        let mut desc_byte_arrays = vec![];

        // Encode into separate buffers
        for d in &datums {
            let mut buf = vec![];
            d.as_sortable_bytes(SortOrder::Asc, &mut buf);
            asc_byte_arrays.push(buf);

            let mut buf = vec![];
            d.as_sortable_bytes(SortOrder::Desc, &mut buf);
            desc_byte_arrays.push(buf);
        }

        // Sort the buffers;
        asc_byte_arrays.sort();
        desc_byte_arrays.sort();
        desc_byte_arrays.reverse();

        assert_eq!(asc_byte_arrays.len(), datums.len());

        // Decode and make sure we're still in lex order
        for ((expected, asc_buf), desc_buf) in
            datums.iter().zip(asc_byte_arrays).zip(desc_byte_arrays)
        {
            let mut actual = Datum::Null;
            let rem = actual.from_sortable_bytes(&asc_buf);
            assert!(actual.sql_eq(expected, true));
            assert!(rem.is_empty());

            let rem = actual.from_sortable_bytes(&desc_buf);
            assert!(actual.sql_eq(expected, true));
            assert!(rem.is_empty());
        }
    }
}
