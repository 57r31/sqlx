use crate::types::chrono::FixedOffset;
use crate::value::ValueRef;
use crate::{
    decode::Decode,
    encode::{Encode, IsNull},
    error::BoxDynError,
    sqlite::{type_info::DataType, Sqlite, SqliteArgumentValue, SqliteTypeInfo, SqliteValueRef},
    types::Type,
};
use chrono::{
    DateTime, Local, NaiveDate, NaiveDateTime, NaiveTime, Offset, SecondsFormat, TimeZone, Utc,
};

impl<Tz: TimeZone> Type<Sqlite> for DateTime<Tz> {
    fn type_info() -> SqliteTypeInfo {
        SqliteTypeInfo(DataType::Datetime)
    }

    fn compatible(ty: &SqliteTypeInfo) -> bool {
        <NaiveDateTime as Type<Sqlite>>::compatible(ty)
    }
}

impl Type<Sqlite> for NaiveDateTime {
    fn type_info() -> SqliteTypeInfo {
        SqliteTypeInfo(DataType::Datetime)
    }

    fn compatible(ty: &SqliteTypeInfo) -> bool {
        matches!(
            ty.0,
            DataType::Datetime | DataType::Text | DataType::Int64 | DataType::Int | DataType::Float
        )
    }
}

impl Type<Sqlite> for NaiveDate {
    fn type_info() -> SqliteTypeInfo {
        SqliteTypeInfo(DataType::Date)
    }

    fn compatible(ty: &SqliteTypeInfo) -> bool {
        matches!(ty.0, DataType::Date | DataType::Text)
    }
}

impl Type<Sqlite> for NaiveTime {
    fn type_info() -> SqliteTypeInfo {
        SqliteTypeInfo(DataType::Time)
    }

    fn compatible(ty: &SqliteTypeInfo) -> bool {
        matches!(ty.0, DataType::Time | DataType::Text)
    }
}

impl<Tz: TimeZone> Encode<'_, Sqlite> for DateTime<Tz>
where
    Tz::Offset: core::fmt::Display,
{
    fn encode_by_ref(&self, buf: &mut Vec<SqliteArgumentValue<'_>>) -> IsNull {
        Encode::<Sqlite>::encode(self.to_rfc3339_opts(SecondsFormat::AutoSi, false), buf)
    }
}

impl Encode<'_, Sqlite> for NaiveDateTime {
    fn encode_by_ref(&self, buf: &mut Vec<SqliteArgumentValue<'_>>) -> IsNull {
        Encode::<Sqlite>::encode(self.format("%F %T%.f").to_string(), buf)
    }
}

impl Encode<'_, Sqlite> for NaiveDate {
    fn encode_by_ref(&self, buf: &mut Vec<SqliteArgumentValue<'_>>) -> IsNull {
        Encode::<Sqlite>::encode(self.format("%F").to_string(), buf)
    }
}

impl Encode<'_, Sqlite> for NaiveTime {
    fn encode_by_ref(&self, buf: &mut Vec<SqliteArgumentValue<'_>>) -> IsNull {
        Encode::<Sqlite>::encode(self.format("%T%.f").to_string(), buf)
    }
}

impl<'r> Decode<'r, Sqlite> for DateTime<Utc> {
    fn decode(value: SqliteValueRef<'r>) -> Result<Self, BoxDynError> {
        Ok(Utc.from_utc_datetime(&decode_datetime(value)?.naive_utc()))
    }
}

impl<'r> Decode<'r, Sqlite> for DateTime<Local> {
    fn decode(value: SqliteValueRef<'r>) -> Result<Self, BoxDynError> {
        Ok(Local.from_utc_datetime(&decode_datetime(value)?.naive_utc()))
    }
}

impl<'r> Decode<'r, Sqlite> for DateTime<FixedOffset> {
    fn decode(value: SqliteValueRef<'r>) -> Result<Self, BoxDynError> {
        decode_datetime(value)
    }
}

fn decode_datetime(value: SqliteValueRef<'_>) -> Result<DateTime<FixedOffset>, BoxDynError> {
    let dt = match value.type_info().0 {
        DataType::Text => decode_datetime_from_text(value.text()?),
        DataType::Int | DataType::Int64 => decode_datetime_from_int(value.int64()),
        DataType::Float => decode_datetime_from_float(value.double()),

        _ => None,
    };

    if let Some(dt) = dt {
        Ok(dt)
    } else {
        Err(format!("invalid datetime: {}", value.text()?).into())
    }
}

fn decode_datetime_from_text(value: &str) -> Option<DateTime<FixedOffset>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return Some(dt);
    }

    // Loop over common date time patterns, inspired by Diesel
    // https://github.com/diesel-rs/diesel/blob/93ab183bcb06c69c0aee4a7557b6798fd52dd0d8/diesel/src/sqlite/types/date_and_time/chrono.rs#L56-L97
    let sqlite_datetime_formats = &[
        // Most likely format
        "%F %T%.f",
        // Other formats in order of appearance in docs
        "%F %R",
        "%F %RZ",
        "%F %R%:z",
        "%F %T%.fZ",
        "%F %T%.f%:z",
        "%FT%R",
        "%FT%RZ",
        "%FT%R%:z",
        "%FT%T%.f",
        "%FT%T%.fZ",
        "%FT%T%.f%:z",
    ];

    for format in sqlite_datetime_formats {
        if let Ok(dt) = DateTime::parse_from_str(value, format) {
            return Some(dt);
        }

        if let Ok(dt) = NaiveDateTime::parse_from_str(value, format) {
            return Some(Utc.fix().from_utc_datetime(&dt));
        }
    }

    None
}

fn decode_datetime_from_int(value: i64) -> Option<DateTime<FixedOffset>> {
    NaiveDateTime::from_timestamp_opt(value, 0).map(|dt| Utc.fix().from_utc_datetime(&dt))
}

fn decode_datetime_from_float(value: f64) -> Option<DateTime<FixedOffset>> {
    let epoch_in_julian_days = 2_440_587.5;
    let seconds_in_day = 86400.0;
    let timestamp = (value - epoch_in_julian_days) * seconds_in_day;
    let seconds = timestamp as i64;
    let nanos = (timestamp.fract() * 1E9) as u32;

    NaiveDateTime::from_timestamp_opt(seconds, nanos).map(|dt| Utc.fix().from_utc_datetime(&dt))
}

impl<'r> Decode<'r, Sqlite> for NaiveDateTime {
    fn decode(value: SqliteValueRef<'r>) -> Result<Self, BoxDynError> {
        Ok(decode_datetime(value)?.naive_local())
    }
}

impl<'r> Decode<'r, Sqlite> for NaiveDate {
    fn decode(value: SqliteValueRef<'r>) -> Result<Self, BoxDynError> {
        Ok(NaiveDate::parse_from_str(value.text()?, "%F")?)
    }
}

impl<'r> Decode<'r, Sqlite> for NaiveTime {
    fn decode(value: SqliteValueRef<'r>) -> Result<Self, BoxDynError> {
        let value = value.text()?;

        // Loop over common time patterns, inspired by Diesel
        // https://github.com/diesel-rs/diesel/blob/93ab183bcb06c69c0aee4a7557b6798fd52dd0d8/diesel/src/sqlite/types/date_and_time/chrono.rs#L29-L47
        #[rustfmt::skip] // don't like how rustfmt mangles the comments
        let sqlite_time_formats = &[
            // Most likely format
            "%T.f", "%T%.f",
            // Other formats in order of appearance in docs
            "%R", "%RZ", "%T%.fZ", "%R%:z", "%T%.f%:z",
        ];

        for format in sqlite_time_formats {
            if let Ok(dt) = NaiveTime::parse_from_str(value, format) {
                return Ok(dt);
            }
        }

        Err(format!("invalid time: {}", value).into())
    }
}
