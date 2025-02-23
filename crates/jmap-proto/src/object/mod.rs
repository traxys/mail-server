/*
 * Copyright (c) 2023 Stalwart Labs Ltd.
 *
 * This file is part of Stalwart Mail Server.
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of
 * the License, or (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU Affero General Public License for more details.
 * in the LICENSE file at the top-level directory of this distribution.
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 *
 * You can be released from the requirements of the AGPLv3 license by
 * purchasing a commercial license. Please contact licensing@stalw.art
 * for more details.
*/

pub mod email;
pub mod email_submission;
pub mod index;
pub mod mailbox;
pub mod sieve;

use std::slice::Iter;

use store::{
    write::{DeserializeFrom, SerializeInto, ToBitmaps, ValueClass},
    Deserialize, Serialize,
};
use utils::{
    codec::leb128::{Leb128Iterator, Leb128Vec},
    map::vec_map::VecMap,
};

use crate::types::{
    blob::BlobId, date::UTCDate, id::Id, keyword::Keyword, property::Property, value::Value,
};

#[derive(Debug, Clone, Default, serde::Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct Object<T> {
    pub properties: VecMap<Property, T>,
}

impl Object<Value> {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            properties: VecMap::with_capacity(capacity),
        }
    }

    pub fn set(&mut self, property: Property, value: impl Into<Value>) -> bool {
        self.properties.set(property, value.into())
    }

    pub fn append(&mut self, property: Property, value: impl Into<Value>) {
        self.properties.append(property, value.into());
    }

    pub fn with_property(mut self, property: Property, value: impl Into<Value>) -> Self {
        self.properties.append(property, value.into());
        self
    }

    pub fn remove(&mut self, property: &Property) -> Value {
        self.properties.remove(property).unwrap_or(Value::Null)
    }

    pub fn get(&self, property: &Property) -> &Value {
        self.properties.get(property).unwrap_or(&Value::Null)
    }
}

impl ToBitmaps for Value {
    fn to_bitmaps(&self, ops: &mut Vec<store::write::Operation>, field: u8, set: bool) {
        match self {
            Value::Text(text) => text.as_str().to_bitmaps(ops, field, set),
            Value::Keyword(keyword) => keyword.to_bitmaps(ops, field, set),
            Value::UnsignedInt(int) => int.to_bitmaps(ops, field, set),
            Value::List(items) => {
                for item in items {
                    match item {
                        Value::Text(text) => text.as_str().to_bitmaps(ops, field, set),
                        Value::UnsignedInt(int) => int.to_bitmaps(ops, field, set),
                        Value::Keyword(keyword) => keyword.to_bitmaps(ops, field, set),
                        _ => (),
                    }
                }
            }
            _ => (),
        }
    }
}

impl ToBitmaps for Object<Value> {
    fn to_bitmaps(&self, _ops: &mut Vec<store::write::Operation>, _field: u8, _set: bool) {
        unreachable!()
    }
}

impl ToBitmaps for &Object<Value> {
    fn to_bitmaps(&self, _ops: &mut Vec<store::write::Operation>, _field: u8, _set: bool) {
        unreachable!()
    }
}

const TEXT: u8 = 0;
const UNSIGNED_INT: u8 = 1;
const BOOL_TRUE: u8 = 2;
const BOOL_FALSE: u8 = 3;
const ID: u8 = 4;
const DATE: u8 = 5;
const BLOB_ID: u8 = 6;
const BLOB: u8 = 7;
const KEYWORD: u8 = 8;
const LIST: u8 = 9;
const OBJECT: u8 = 10;
const NULL: u8 = 11;

impl Serialize for Value {
    fn serialize(self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(1024);
        self.serialize_into(&mut buf);
        buf
    }
}

impl Deserialize for Value {
    fn deserialize(bytes: &[u8]) -> store::Result<Self> {
        Self::deserialize_from(&mut bytes.iter())
            .ok_or_else(|| store::Error::InternalError("Failed to deserialize value.".to_string()))
    }
}

impl Serialize for Object<Value> {
    fn serialize(self) -> Vec<u8> {
        (&self).serialize()
    }
}

impl Serialize for &Object<Value> {
    fn serialize(self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(1024);
        self.serialize_into(&mut buf);
        buf
    }
}

impl Deserialize for Object<Value> {
    fn deserialize(bytes: &[u8]) -> store::Result<Self> {
        Object::deserialize_from(&mut bytes.iter())
            .ok_or_else(|| store::Error::InternalError("Failed to deserialize object.".to_string()))
    }
}

impl SerializeInto for Object<Value> {
    fn serialize_into(&self, buf: &mut Vec<u8>) {
        buf.push_leb128(self.properties.len());
        for (k, v) in &self.properties {
            k.serialize_into(buf);
            v.serialize_into(buf);
        }
    }
}
impl DeserializeFrom for Object<Value> {
    fn deserialize_from(bytes: &mut Iter<'_, u8>) -> Option<Object<Value>> {
        let len = bytes.next_leb128()?;
        let mut properties = VecMap::with_capacity(len);
        for _ in 0..len {
            let key = Property::deserialize_from(bytes)?;
            let value = Value::deserialize_from(bytes)?;
            properties.append(key, value);
        }
        Some(Object { properties })
    }
}

impl SerializeInto for Value {
    fn serialize_into(&self, buf: &mut Vec<u8>) {
        match self {
            Value::Text(v) => {
                buf.push(TEXT);
                v.serialize_into(buf);
            }
            Value::UnsignedInt(v) => {
                buf.push(UNSIGNED_INT);
                v.serialize_into(buf);
            }
            Value::Bool(v) => {
                buf.push(if *v { BOOL_TRUE } else { BOOL_FALSE });
            }
            Value::Id(v) => {
                buf.push(ID);
                v.id().serialize_into(buf);
            }
            Value::Date(v) => {
                buf.push(DATE);
                (v.timestamp() as u64).serialize_into(buf);
            }
            Value::BlobId(v) => {
                buf.push(BLOB_ID);
                v.serialize_into(buf);
            }
            Value::Keyword(v) => {
                buf.push(KEYWORD);
                v.serialize_into(buf);
            }
            Value::List(v) => {
                buf.push(LIST);
                buf.push_leb128(v.len());
                for i in v {
                    i.serialize_into(buf);
                }
            }
            Value::Object(v) => {
                buf.push(OBJECT);
                v.serialize_into(buf);
            }
            Value::Blob(v) => {
                buf.push(BLOB);
                v.serialize_into(buf);
            }
            Value::Null => {
                buf.push(NULL);
            }
        }
    }
}

impl DeserializeFrom for Value {
    fn deserialize_from(bytes: &mut Iter<'_, u8>) -> Option<Self> {
        match *bytes.next()? {
            TEXT => Some(Value::Text(String::deserialize_from(bytes)?)),
            UNSIGNED_INT => Some(Value::UnsignedInt(bytes.next_leb128()?)),
            BOOL_TRUE => Some(Value::Bool(true)),
            BOOL_FALSE => Some(Value::Bool(false)),
            ID => Some(Value::Id(Id::new(bytes.next_leb128()?))),
            DATE => Some(Value::Date(UTCDate::from_timestamp(
                bytes.next_leb128::<u64>()? as i64,
            ))),
            BLOB_ID => Some(Value::BlobId(BlobId::deserialize_from(bytes)?)),
            KEYWORD => Some(Value::Keyword(Keyword::deserialize_from(bytes)?)),
            LIST => {
                let len = bytes.next_leb128()?;
                let mut items = Vec::with_capacity(len);
                for _ in 0..len {
                    items.push(Value::deserialize_from(bytes)?);
                }
                Some(Value::List(items))
            }
            OBJECT => Some(Value::Object(Object::deserialize_from(bytes)?)),
            BLOB => Some(Value::Blob(Vec::deserialize_from(bytes)?)),
            NULL => Some(Value::Null),
            _ => None,
        }
    }
}

impl From<Property> for ValueClass {
    fn from(value: Property) -> Self {
        ValueClass::Property {
            field: value.into(),
            family: 0,
        }
    }
}
