use chrono::Duration;
use scylla::cql_to_rust::{FromCqlVal, FromCqlValError};
use scylla::frame::response::result::CqlValue;
use serde::de::Visitor;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct SerializableDuration {
    pub timestamp: Duration,
}

impl FromCqlVal<CqlValue> for SerializableDuration {
    fn from_cql(cql_val: CqlValue) -> Result<Self, scylla::cql_to_rust::FromCqlValError> {
        Ok(cql_val
            .as_duration()
            .ok_or(FromCqlValError::BadCqlType)?
            .into())
    }
}

impl Serialize for SerializableDuration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_i64(self.timestamp.num_milliseconds())
    }
}

impl From<Duration> for SerializableDuration {
    fn from(value: Duration) -> Self {
        SerializableDuration { timestamp: value }
    }
}

struct DurationVisitor;

impl<'de> Visitor<'de> for DurationVisitor {
    type Value = SerializableDuration;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("an integer between -2^63 and 2^63")
    }

    fn visit_i8<E>(self, v: i8) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Duration::milliseconds(i64::from(v)).into())
    }

    fn visit_i16<E>(self, v: i16) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Duration::milliseconds(i64::from(v)).into())
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Duration::milliseconds(i64::from(v)).into())
    }

    fn visit_u8<E>(self, v: u8) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Duration::milliseconds(i64::from(v)).into())
    }

    fn visit_u16<E>(self, v: u16) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Duration::milliseconds(i64::from(v)).into())
    }

    fn visit_u32<E>(self, v: u32) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Duration::milliseconds(i64::from(v)).into())
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let converted: i64 = v
            .try_into()
            .map_err(|_| E::custom(format!("i64 out of range: {}", v)))?;
        Ok(Duration::milliseconds(converted).into())
    }
}

impl<'de> Deserialize<'de> for SerializableDuration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_i64(DurationVisitor)
    }
}
