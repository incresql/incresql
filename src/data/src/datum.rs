use crate::json::{Json, OwnedJson};
use crate::jsonpath_utils::JsonPathExpression;
use crate::DataType;
use chrono::{Datelike, NaiveDate, NaiveDateTime};
use rust_decimal::Decimal;
use std::cmp::Ordering;
use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::ops::Deref;

/// Datum - in memory representation of sql value.
/// The same datum may be able to be interpreted as multiple different
/// datatypes. Ie bytea can is used to back json and text.
#[derive(Clone, Debug)]
pub enum Datum<'a> {
    Null,
    Boolean(bool),
    // Text type from on-disk tuple, just points back to the rocks db key/value bytes
    ByteARef(&'a [u8]),
    // On-heap text type, potentially used for function return types or where we need a static
    // lifetime, ie select max(str_col)
    ByteAOwned(Box<[u8]>),
    // Inline text type, optimization of TextOwned where the text is small enough to store inline
    // without having pay the cost of allocation/pointer chasing.
    ByteAInline(u8, [u8; 22]),
    Integer(i32),
    BigInt(i64),
    Decimal(Decimal),

    // Compiled Datum types
    Jsonpath(Box<JsonPathExpression>),
    JsonpathRef(&'a JsonPathExpression),
}

impl<'a> Datum<'a> {
    /// Like clone but instead of cloning Datum::TextOwned etc it will just take a reference
    pub fn ref_clone(&'a self) -> Datum<'a> {
        match self {
            Datum::ByteAOwned(s) => Datum::ByteARef(&s),
            Datum::Jsonpath(jp) => Datum::JsonpathRef(&jp),
            _ => self.clone(),
        }
    }

    /// As datums can reference data external to themselves they're only guaranteed to be valid
    /// for the current iteration of the iterator/loop etc. This method creates a new datum with
    /// any borrowed data now owned so it can be held onto across iterations(ie to sort them).
    pub fn as_static(&'a self) -> Datum<'static> {
        match self {
            Datum::Null => Datum::Null,
            Datum::Boolean(b) => Datum::Boolean(*b),
            Datum::Integer(i) => Datum::Integer(*i),
            Datum::BigInt(i) => Datum::BigInt(*i),
            Datum::Decimal(d) => Datum::Decimal(*d),
            Datum::ByteAOwned(s) => Datum::ByteAOwned(s.clone()),
            Datum::ByteAInline(l, bytes) => Datum::ByteAInline(*l, *bytes),
            Datum::ByteARef(s) => {
                let len = s.len();
                if len <= 22 {
                    let mut bytes = [0_u8; 22];
                    bytes.as_mut()[..len].copy_from_slice(s);
                    Datum::ByteAInline(len as u8, bytes)
                } else {
                    Datum::ByteAOwned(Box::from(*s))
                }
            }
            Datum::Jsonpath(expr) => Datum::Jsonpath(expr.clone()),
            Datum::JsonpathRef(expr) => Datum::Jsonpath(Box::new(expr.deref().clone())),
        }
    }

    /// As datums can reference data external to themselves they're only guaranteed to be valid
    /// for the current iteration of the iterator/loop etc. This method consumes a datum
    /// and returns a static equivalent, in the case where the datum already owns
    /// some data on the heap this will prevent us from making a copy where we don't need to.
    pub fn into_static(self) -> Datum<'static> {
        match self {
            Datum::Null => Datum::Null,
            Datum::Boolean(b) => Datum::Boolean(b),
            Datum::Integer(i) => Datum::Integer(i),
            Datum::BigInt(i) => Datum::BigInt(i),
            Datum::Decimal(d) => Datum::Decimal(d),
            Datum::ByteAOwned(s) => Datum::ByteAOwned(s),
            Datum::ByteAInline(l, bytes) => Datum::ByteAInline(l, bytes),
            Datum::ByteARef(s) => {
                let len = s.len();
                if len <= 22 {
                    let mut bytes = [0_u8; 22];
                    bytes.as_mut()[..len].copy_from_slice(s);
                    Datum::ByteAInline(len as u8, bytes)
                } else {
                    Datum::ByteAOwned(Box::from(s))
                }
            }
            Datum::Jsonpath(expr) => Datum::Jsonpath(expr),
            Datum::JsonpathRef(expr) => Datum::Jsonpath(Box::new(expr.deref().clone())),
        }
    }

    /// Returns true if this value is null
    pub fn is_null(&self) -> bool {
        matches!(self, Datum::Null)
    }

    /// Returns true if this value is equal to another.
    /// According to sql rules, null != null, this is the behaviour if null_safe = false,
    /// if null_safe is set to true then null == null
    pub fn sql_eq(&self, other: &Self, null_safe: bool) -> bool {
        match self {
            Datum::Null => other.is_null() && null_safe,
            Datum::Boolean(b) => other.as_maybe_boolean() == Some(*b),
            Datum::Integer(i) => other.as_maybe_integer() == Some(*i),
            Datum::BigInt(i) => other.as_maybe_bigint() == Some(*i),
            Datum::Decimal(d) => other.as_maybe_decimal() == Some(*d),
            Datum::ByteAOwned(_) | Datum::ByteAInline(..) | Datum::ByteARef(_) => {
                self.as_maybe_text() == other.as_maybe_text()
            }
            Datum::Jsonpath(_) | Datum::JsonpathRef(_) => {
                self.as_maybe_jsonpath() == other.as_maybe_jsonpath()
            }
        }
    }
}

impl PartialEq for Datum<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.sql_eq(other, true)
    }
}
impl Eq for Datum<'_> {}

impl Ord for Datum<'_> {
    /// This cmp is only valid for datums of the same type.
    /// It is "null" safe, ie it doesn't follow the standard sql rules.
    /// ie null == null
    /// and (null < foo) == true
    fn cmp(&self, other: &Self) -> Ordering {
        match self {
            Datum::Null => {
                if other.is_null() {
                    Ordering::Equal
                } else {
                    Ordering::Less
                }
            }
            Datum::Boolean(b) => {
                if let Some(o) = other.as_maybe_boolean() {
                    b.cmp(&o)
                } else {
                    Ordering::Greater
                }
            }
            Datum::Integer(i) => {
                if let Some(o) = other.as_maybe_integer() {
                    i.cmp(&o)
                } else {
                    Ordering::Greater
                }
            }
            Datum::BigInt(i) => {
                if let Some(o) = other.as_maybe_bigint() {
                    i.cmp(&o)
                } else {
                    Ordering::Greater
                }
            }
            Datum::Decimal(d) => {
                if let Some(o) = other.as_maybe_decimal() {
                    d.cmp(&o)
                } else {
                    Ordering::Greater
                }
            }
            Datum::ByteAOwned(_) | Datum::ByteAInline(..) | Datum::ByteARef(_) => {
                if let Some(t) = other.as_maybe_text() {
                    self.as_text().cmp(t)
                } else {
                    Ordering::Greater
                }
            }
            Datum::Jsonpath(_) | Datum::JsonpathRef(_) => {
                if let Some(t) = other.as_maybe_jsonpath() {
                    self.as_jsonpath().cmp(t)
                } else {
                    Ordering::Greater
                }
            }
        }
    }
}

impl PartialOrd for Datum<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// From builders to build datums from the native rust types
impl Default for Datum<'_> {
    fn default() -> Self {
        Datum::Null
    }
}

impl From<bool> for Datum<'static> {
    fn from(b: bool) -> Self {
        Datum::Boolean(b)
    }
}

impl From<i32> for Datum<'static> {
    fn from(i: i32) -> Self {
        Datum::Integer(i)
    }
}

impl From<i64> for Datum<'static> {
    fn from(i: i64) -> Self {
        Datum::BigInt(i)
    }
}

impl From<Decimal> for Datum<'static> {
    fn from(d: Decimal) -> Self {
        Datum::Decimal(d)
    }
}

impl From<NaiveDate> for Datum<'static> {
    fn from(d: NaiveDate) -> Self {
        Datum::Integer((d.year() << 9) + (d.ordinal() as i32))
    }
}

impl From<NaiveDateTime> for Datum<'static> {
    fn from(t: NaiveDateTime) -> Self {
        Datum::BigInt(t.timestamp_millis())
    }
}

impl From<String> for Datum<'static> {
    fn from(s: String) -> Self {
        Datum::ByteAOwned(s.into_boxed_str().into_boxed_bytes())
    }
}

impl<'a> From<&'a str> for Datum<'a> {
    fn from(s: &'a str) -> Self {
        Datum::ByteARef(s.as_bytes())
    }
}

impl From<Vec<u8>> for Datum<'static> {
    fn from(vec: Vec<u8>) -> Self {
        Datum::ByteAOwned(vec.into_boxed_slice())
    }
}

impl From<OwnedJson> for Datum<'static> {
    fn from(json: OwnedJson) -> Self {
        Datum::ByteAOwned(json.bytes.into_boxed_slice())
    }
}

impl<'a> From<Json<'a>> for Datum<'a> {
    fn from(json: Json) -> Datum {
        Datum::ByteARef(json.bytes)
    }
}

/// A Wrapper that can be used to temporarily associate a datum
/// with it's typing information to perform low level operations
/// where we need that extra typing
pub struct TypedDatum<'a> {
    datum: &'a Datum<'a>,
    datatype: DataType,
}

impl Datum<'_> {
    pub fn typed_with(&self, datatype: DataType) -> TypedDatum {
        TypedDatum {
            datum: self,
            datatype,
        }
    }
}

impl Display for TypedDatum<'_> {
    /// When used with the alternate flag this will format as a sql string, ie strings will be quoted
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self.datum {
            Datum::Null => f.write_str("NULL"),
            Datum::ByteARef(_) | Datum::ByteAOwned(_) | Datum::ByteAInline(..) => {
                match self.datatype {
                    DataType::Text => {
                        let str = self.datum.as_text();
                        if f.alternate() {
                            // The debug trait should quote and escape our strings for us
                            Debug::fmt(str, f)
                        } else {
                            f.write_str(str)
                        }
                    }
                    DataType::Json => {
                        let json = Json::from_bytes(self.datum.as_bytea());
                        f.write_str(&serde_json::to_string(&json).unwrap())
                    }
                    _ => {
                        let bytes = self.datum.as_bytea();
                        if f.alternate() {
                            f.write_str("\"")?;
                            for b in bytes {
                                f.write_fmt(format_args!("{:x}", b))?;
                            }
                            f.write_str("\"")
                        } else {
                            for b in bytes {
                                f.write_fmt(format_args!("{:x}", b))?;
                            }
                            Ok(())
                        }
                    }
                }
            }
            Datum::Boolean(b) => f.write_str(if *b { "TRUE" } else { "FALSE" }),
            Datum::Integer(i) => match self.datatype {
                DataType::Date => Display::fmt(&self.datum.as_date(), f),
                _ => Display::fmt(i, f),
            },
            Datum::BigInt(i) => match self.datatype {
                DataType::Timestamp => Display::fmt(&self.datum.as_timestamp(), f),
                _ => Display::fmt(i, f),
            },
            Datum::Decimal(d) => {
                if let DataType::Decimal(_p, s) = self.datatype {
                    f.write_fmt(format_args!("{:.*}", s as usize, d))
                } else {
                    Display::fmt(d, f)
                }
            }
            Datum::Jsonpath(_) | Datum::JsonpathRef(_) => Display::fmt(self.datum.as_jsonpath(), f),
        }
    }
}

// Into's to get back rust types from datums, these are just "dumb" and simply map 1-1 without any
// attempts to do any casting
impl<'a> Datum<'a> {
    pub fn as_maybe_bytea(&'a self) -> Option<&'a [u8]> {
        match self {
            Datum::ByteARef(s) => Some(s),
            Datum::ByteAInline(len, b) => Some(&b.as_ref()[..(*len as usize)]),
            Datum::ByteAOwned(s) => Some(s.as_ref()),
            _ => None,
        }
    }

    #[track_caller]
    pub fn as_bytea(&'a self) -> &'a [u8] {
        self.as_maybe_bytea().unwrap()
    }

    pub fn as_maybe_text(&'a self) -> Option<&'a str> {
        self.as_maybe_bytea()
            .map(|bytes| unsafe { std::str::from_utf8_unchecked(bytes) })
    }

    #[track_caller]
    pub fn as_text(&'a self) -> &'a str {
        self.as_maybe_text().unwrap()
    }

    pub fn as_maybe_json(&'a self) -> Option<Json<'a>> {
        self.as_maybe_bytea().map(Json::from_bytes)
    }

    #[track_caller]
    pub fn as_json(&'a self) -> Json<'a> {
        self.as_maybe_json().unwrap()
    }

    pub fn as_maybe_integer(&self) -> Option<i32> {
        if let Datum::Integer(i) = self {
            Some(*i)
        } else {
            None
        }
    }

    #[track_caller]
    pub fn as_integer(&self) -> i32 {
        self.as_maybe_integer().unwrap()
    }

    #[track_caller]
    pub fn as_integer_mut(&mut self) -> &mut i32 {
        if let Datum::Integer(i) = self {
            i
        } else {
            panic!()
        }
    }

    pub fn as_maybe_bigint(&self) -> Option<i64> {
        if let Datum::BigInt(i) = self {
            Some(*i)
        } else {
            None
        }
    }

    #[track_caller]
    pub fn as_bigint(&self) -> i64 {
        self.as_maybe_bigint().unwrap()
    }

    #[track_caller]
    pub fn as_bigint_mut(&mut self) -> &mut i64 {
        if let Datum::BigInt(i) = self {
            i
        } else {
            panic!()
        }
    }

    pub fn as_maybe_decimal(&self) -> Option<Decimal> {
        if let Datum::Decimal(d) = self {
            Some(*d)
        } else {
            None
        }
    }

    #[track_caller]
    pub fn as_decimal(&self) -> Decimal {
        self.as_maybe_decimal().unwrap()
    }

    #[track_caller]
    pub fn as_decimal_mut(&mut self) -> &mut Decimal {
        if let Datum::Decimal(d) = self {
            d
        } else {
            panic!()
        }
    }

    pub fn as_maybe_date(&self) -> Option<NaiveDate> {
        if let Datum::Integer(i) = self {
            Some(NaiveDate::from_yo(i >> 9, (i & 511) as u32))
        } else {
            None
        }
    }

    #[track_caller]
    pub fn as_date(&self) -> NaiveDate {
        self.as_maybe_date().unwrap()
    }

    pub fn as_maybe_timestamp(&self) -> Option<NaiveDateTime> {
        if let Datum::BigInt(i) = self {
            let seconds = i.div_euclid(1000);
            let millis = i.rem_euclid(1000);
            Some(NaiveDateTime::from_timestamp(
                seconds,
                millis as u32 * 1000000,
            ))
        } else {
            None
        }
    }

    pub fn as_timestamp(&self) -> NaiveDateTime {
        self.as_maybe_timestamp().unwrap()
    }

    pub fn as_maybe_boolean(&self) -> Option<bool> {
        if let Datum::Boolean(b) = self {
            Some(*b)
        } else {
            None
        }
    }

    #[track_caller]
    pub fn as_boolean(&self) -> bool {
        self.as_maybe_boolean().unwrap()
    }

    pub fn as_maybe_jsonpath(&self) -> Option<&JsonPathExpression> {
        match self {
            Datum::Jsonpath(j) => Some(&j),
            Datum::JsonpathRef(j) => Some(*j),
            _ => None,
        }
    }

    #[track_caller]
    pub fn as_jsonpath(&self) -> &JsonPathExpression {
        self.as_maybe_jsonpath().unwrap()
    }
}

/// Hash implementation on datum. Allows us to use hashmaps etc.
impl Hash for Datum<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Datum::Null => state.write_u8(0),
            Datum::Boolean(b) => b.hash(state),
            Datum::Integer(i) => i.hash(state),
            Datum::BigInt(i) => i.hash(state),
            Datum::Decimal(d) => d.hash(state),
            Datum::ByteAOwned(_) | Datum::ByteAInline(_, _) | Datum::ByteARef(_) => {
                self.as_bytea().hash(state)
            }
            Datum::Jsonpath(_) | Datum::JsonpathRef(_) => self.as_jsonpath().original().hash(state),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::JsonBuilder;
    use std::io::Write;
    use std::mem::size_of;
    use std::str::FromStr;

    #[test]
    fn test_datum_size() {
        // The decimal and &str types need to be at least 2 words aka 16bytes wide,
        // When we include the discriminator (1 byte) that makes it 17 bytes, however due to
        // word sized alignment for the &str pointers we actually end up at 24 bytes in size,
        // this means we've got enough room for 23 byte of data for short strings etc.
        assert_eq!(24, size_of::<Datum>());
    }

    #[test]
    fn test_datum_ref_clone() {
        assert_eq!(Datum::from(1).ref_clone(), Datum::Integer(1));

        if let Datum::ByteARef(b"hello") =
            Datum::ByteAOwned(Box::from(b"hello".as_ref())).ref_clone()
        {
        } else {
            panic!()
        }
    }

    #[test]
    fn test_datum_as_static() {
        if let Datum::ByteAInline(11, b) = Datum::ByteARef(b"Hello world").as_static() {
            assert_eq!(b, *b"Hello world\0\0\0\0\0\0\0\0\0\0\0")
        } else {
            panic!()
        }

        if let Datum::ByteAOwned(b) = Datum::ByteARef(b"Hello world123456789123456789").as_static()
        {
            assert_eq!(b, Box::from(b"Hello world123456789123456789".as_ref()))
        } else {
            panic!()
        }
    }

    #[test]
    fn test_datum_is_null() {
        assert_eq!(Datum::Null.is_null(), true);

        assert_eq!(Datum::from(1).is_null(), false);
    }

    #[test]
    fn test_datum_sql_eq() {
        // Nulls
        assert_eq!(Datum::Null.sql_eq(&Datum::Null, false), false);
        assert_eq!(Datum::Null.sql_eq(&Datum::Null, true), true);
        // Mixed Nulls with bools
        assert_eq!(Datum::from(true).sql_eq(&Datum::Null, true), false);
        assert_eq!(Datum::Null.sql_eq(&Datum::from(true), true), false);
        assert_eq!(Datum::from(false).sql_eq(&Datum::from(false), false), true);
        // Strings
        assert_eq!(Datum::from("abc").sql_eq(&Datum::from("abc"), false), true);
        assert_eq!(Datum::from("abc").sql_eq(&Datum::from("efg"), false), false);
        assert_eq!(
            Datum::from("abc").sql_eq(&Datum::ByteAOwned(Box::from(b"abc".as_ref())), false),
            true
        );
        assert_eq!(
            Datum::ByteAOwned(Box::from(b"abc".as_ref())).sql_eq(&Datum::from("abc"), false),
            true
        );
    }

    #[test]
    fn test_datum_from_boolean() {
        assert_eq!(Datum::from(true), Datum::Boolean(true));
        assert_eq!(Datum::from(false), Datum::Boolean(false));
    }

    #[test]
    fn test_datum_from_integer() {
        assert_eq!(Datum::from(1234), Datum::Integer(1234));
    }

    #[test]
    fn test_datum_from_bigint() {
        assert_eq!(Datum::from(1234_i64), Datum::BigInt(1234));
    }

    #[test]
    fn test_datum_from_decimal() {
        assert_eq!(
            Datum::from(Decimal::new(12345, 2)),
            Datum::Decimal(Decimal::new(12345, 2))
        );
    }

    #[test]
    fn test_datum_from_string() {
        assert_eq!(
            Datum::from(String::from("Hello world")),
            Datum::ByteAOwned(Box::from(b"Hello world".as_ref()))
        );

        assert_eq!(Datum::from("Hello world"), Datum::ByteARef(b"Hello world"));
    }

    #[test]
    fn test_datum_from_vec() {
        assert_eq!(
            Datum::from(Vec::from(b"123".as_ref())),
            Datum::ByteAOwned(Box::from(b"123".as_ref()))
        );
    }

    #[test]
    fn test_datum_as_str() {
        assert_eq!(
            Datum::ByteAOwned(Box::from(b"Hello world".as_ref())).as_maybe_text(),
            Some("Hello world")
        );

        let mut bytes = [0_u8; 22];
        bytes.as_mut().write_all("Hello world".as_bytes()).unwrap();

        assert_eq!(
            Datum::ByteAInline(11, bytes).as_maybe_text(),
            Some("Hello world")
        );

        let backing_slice = b"Hello world";
        assert_eq!(
            Datum::ByteARef(backing_slice).as_maybe_text(),
            Some("Hello world")
        );

        assert_eq!(Datum::Null.as_maybe_text(), None);
    }

    #[test]
    fn test_datum_as_ints() {
        assert_eq!(Datum::Integer(123).as_maybe_integer(), Some(123_i32));

        assert_eq!(Datum::Null.as_maybe_integer(), None);

        assert_eq!(Datum::BigInt(123).as_maybe_bigint(), Some(123_i64));

        assert_eq!(Datum::Null.as_maybe_bigint(), None);
    }

    #[test]
    fn test_datum_as_decimal() {
        assert_eq!(
            Datum::Decimal(Decimal::new(3232, 1)).as_maybe_decimal(),
            Some(Decimal::new(3232, 1))
        );

        assert_eq!(Datum::Null.as_maybe_decimal(), None);
    }

    #[test]
    fn test_datum_as_boolean() {
        assert_eq!(Datum::Boolean(true).as_maybe_boolean(), Some(true));

        assert_eq!(Datum::Null.as_maybe_decimal(), None);
    }

    #[test]
    fn test_datum_display() {
        assert_eq!(
            format!("{}", Datum::Null.typed_with(DataType::Text)),
            "NULL"
        );

        assert_eq!(
            format!("{}", Datum::Boolean(true).typed_with(DataType::Boolean)),
            "TRUE"
        );

        assert_eq!(
            format!("{}", Datum::Integer(123).typed_with(DataType::Integer)),
            "123"
        );
        assert_eq!(
            format!("{}", Datum::BigInt(123).typed_with(DataType::BigInt)),
            "123"
        );

        assert_eq!(
            format!(
                "{}",
                Datum::Decimal(Decimal::from_str("12.34").unwrap())
                    .typed_with(DataType::Decimal(10, 2))
            ),
            "12.34"
        );

        assert_eq!(
            format!(
                "{}",
                Datum::Decimal(Decimal::from_str("12.34").unwrap())
                    .typed_with(DataType::Decimal(10, 4))
            ),
            "12.3400"
        );

        assert_eq!(
            format!(
                "{}",
                Datum::from("hello".to_string()).typed_with(DataType::Text)
            ),
            "hello"
        );
        assert_eq!(
            format!(
                "{:#}",
                Datum::from("he\"llo".to_string()).typed_with(DataType::Text)
            ),
            "\"he\\\"llo\""
        );

        assert_eq!(
            format!(
                "{}",
                Datum::from("hello".to_string()).typed_with(DataType::ByteA)
            ),
            "68656c6c6f"
        );

        assert_eq!(
            format!(
                "{}",
                Datum::from(NaiveDate::from_ymd(2000, 2, 10)).typed_with(DataType::Date)
            ),
            "2000-02-10"
        );
    }

    #[test]
    fn test_datum_display_json() {
        let json = JsonBuilder::default().object(|object| {
            object.push_int("one", 1);
            object.push_int("two", 2);
        });

        assert_eq!(
            format!("{}", Datum::from(json).typed_with(DataType::Json)),
            r#"{"one":1,"two":2}"#
        );
    }
}
